use super::ui::*;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chrono::{Datelike, NaiveDate};
use cosmic_text::{FontSystem, SwashCache};
use smithay_client_toolkit::{
    compositor::{CompositorState, Region},
    output::OutputState,
    registry::RegistryState,
    seat::{SeatState, keyboard::Modifiers},
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface},
    },
    shm::{Shm, slot::SlotPool},
};
use wayland_client::{
    QueueHandle,
    protocol::{wl_keyboard, wl_pointer, wl_shm},
};

use crate::hud::anim;
use crate::hud::data;
use crate::hud::palette;
use crate::hud::settings;
use crate::hud::tray;

use super::calendar::CalendarView;
use super::text::TextRenderer;

mod bar;
mod input;
mod osd;
mod panels;
mod surfaces;

pub struct App {
    pub registry_state: RegistryState,
    pub output_state: OutputState,
    pub seat_state: SeatState,
    pub shm: Shm,
    pub pool: SlotPool,
    pub compositor: CompositorState,
    pub layer_shell: LayerShell,
    pub layer: LayerSurface,
    pub configured: bool,
    pub first_draw: bool,
    /// Панель закрыта композитором и её надо пересоздать.
    ///
    /// Обычно это происходит при отключении вывода: niri отключает внутреннюю
    /// панель ноутбука при закрытии крышки и закрывает layer-surface вместе с
    /// ней. Раньше это считалось концом сессии, и процесс просто уходил, а
    /// `spawn-at-startup` в niri повторно его не запускал — панель пропадала
    /// навсегда. Теперь surface пересоздаётся, и когда выход вернётся,
    /// композитор пришлёт `configure`, и отрисовка продолжится.
    pub recreate_panel: bool,
    pub width: u32,
    pub height: u32,
    pub exit: bool,
    pub shared: Arc<data::Shared>,
    pub palette: palette::Palette,
    pub settings: settings::Settings,
    pub font: String,
    pub font_system: FontSystem,
    pub swash: SwashCache,
    pub clock: String,
    pub clock_secs: String,
    pub pointer: Option<wl_pointer::WlPointer>,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
    pub regions: Vec<(Hit, Rect)>,
    pub bar_hover: Option<Hit>,
    /// Подсветки чипов: у каждого своя цель 0 или 1 и своё движение.
    pub hover_anim: anim::AnimSet<Hit>,
    pub panel: Option<Panel>,
    pub popup: Option<Popup>,
    pub grab: Option<Grab>,
    pub tooltip: Option<Tooltip>,
    pub osd: Option<Osd>,
    pub osd_detector: crate::hud::osd::OsdDetector,
    /// Проявление карточки OSD: 0 — закрыта, 1 — открыта.
    pub osd_anim: anim::Tween,
    /// Заполнение полосы уровня OSD, доля 0..1.
    pub osd_fill: anim::Tween,
    /// Выезд панели: 0 — закрыта, 1 — открыта. Отдельная цель
    /// `popup_target` ушла внутрь твина, но читается прежним способом.
    pub popup_anim: anim::Tween,
    /// Момент, на котором считаются все анимации. Обновляется главным циклом;
    /// внутри модуля анимаций `Instant::now()` не вызывается.
    pub now: std::time::Instant,
    /// Настройки анимаций из `settings.json`.
    pub anim: anim::AnimSettings,
    pub mods: Modifiers,
}

impl App {
    /// Степень открытия панели: 0 — закрыта, 1 — открыта.
    pub fn popup_t(&self) -> f32 {
        self.popup_anim.value(self.now)
    }

    /// Куда панель идёт: 0 — закрываться, 1 — открыта. Значение нужно
    /// отрисовке, чтобы не показывать элементы управления ещё не открывшейся
    /// панели.
    pub fn popup_target(&self) -> f32 {
        self.popup_anim.to
    }

    /// Продвинуть анимации на текущий момент. `true`, если что-то ещё
    /// двигается: значит, кадр нужен и ждать события дольше нельзя.
    pub fn tick_animations(&mut self) -> bool {
        let now = self.now;
        self.hover_anim.tick(now);
        self.animating()
    }

    /// Есть ли незаконченные движения. Пока они идут, цикл событий не должен
    /// засыпать надолго — иначе анимация идёт рывками.
    pub fn animating(&self) -> bool {
        self.hover_anim.active()
            || !self.popup_anim.done(self.now)
            || !self.osd_anim.done(self.now)
            || !self.osd_fill.done(self.now)
    }
}
