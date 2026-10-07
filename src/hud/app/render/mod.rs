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
    pub hover_anim: Option<(Hit, f32)>,
    pub panel: Option<Panel>,
    pub popup: Option<Popup>,
    pub grab: Option<Grab>,
    pub tooltip: Option<Tooltip>,
    pub osd: Option<Osd>,
    pub osd_detector: crate::hud::osd::OsdDetector,
    pub popup_t: f32,
    pub popup_target: f32,
    pub last_tick: std::time::Instant,
    pub mods: Modifiers,
}
