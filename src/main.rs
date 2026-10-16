mod hud;
// Меню обращается к профилю питания как `crate::power_profile`: в бинарях это
// плоский модуль у корня, а панель держит всё в `hud`. Поэтому и здесь он
// объявлен рядом с `hud`, а не внутри него.
#[allow(dead_code)]
#[path = "hud/power_profile.rs"]
mod power_profile;

use std::os::fd::AsFd;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use cosmic_text::{FontSystem, SwashCache};
use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    registry::RegistryState,
    seat::{SeatState, keyboard::Modifiers},
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface},
    },
    shm::{Shm, slot::SlotPool},
};
use wayland_client::{Connection, QueueHandle, globals::registry_queue_init};

use hud::App;
use hud::ui::*;

/// Создаёт layer-surface панели.
///
/// Вынесено отдельно, потому что surface приходится пересоздавать: niri
/// закрывает его вместе с выходом, когда гасит внутреннюю панель по крышке.
fn create_panel(
    compositor: &CompositorState,
    layer_shell: &LayerShell,
    qh: &QueueHandle<App>,
    height: u32,
) -> LayerSurface {
    let surface = compositor.create_surface(qh);
    let layer = layer_shell.create_layer_surface(qh, surface, Layer::Top, Some("hudbar"), None);
    let reserve = height as i32 + 2;
    layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(0, height);
    layer.set_exclusive_zone(reserve);
    layer.set_margin(1, 0, 1, 0);
    layer.commit();
    // Слой создаётся и заново после отключения вывода: строка в журнале —
    // единственный след того, что панель вернулась.
    hud::log::trace(format!("слой создан: панель, высота {height}"));
    layer
}

fn main() {
    let hud_settings = hud::settings::load();
    let mut settings_stamp = hud::settings::modified();
    let mut palette_stamp = hud::palette::modified();
    // Один момент на весь App: анимации считаются от него, а не от собственных
    // вызовов `Instant::now()` — иначе разные значения читались бы в разные
    // микро instants и кадр дёргался бы.
    let started = std::time::Instant::now();
    // На энергосбережении анимации гаснут сами, настройку не трогаем.
    let mut anim_on = hud_settings.animations && power_profile::anim_allowed();
    let anim = hud::anim::AnimSettings::new(anim_on, hud_settings.animation_speed);
    let conn = Connection::connect_to_env().expect("wayland connection");
    let (globals, mut event_queue) = registry_queue_init(&conn).expect("registry");
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("wlr_layer_shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm");

    let layer = create_panel(&compositor, &layer_shell, &qh, hud_settings.height);

    let pool = SlotPool::new(12_000_000, &shm).expect("shm pool");
    let (wake_rx, wake_tx) = hud::data::wake_pipe();
    let shared = Arc::new(hud::data::Shared {
        sys: std::sync::Mutex::new(hud::data::Sys::default()),
        dirty: std::sync::atomic::AtomicBool::new(true),
        wake_tx,
    });
    hud::data::spawn(shared.clone());

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        pool,
        compositor,
        layer_shell,
        layer,
        configured: false,
        first_draw: false,
        recreate_panel: false,
        width: 0,
        height: hud_settings.height,
        exit: false,
        shared,
        palette: hud::palette::load(),
        settings: hud_settings,
        font: configured_font(),
        font_system: FontSystem::new(),
        swash: SwashCache::new(),
        clock: String::new(),
        clock_secs: String::new(),
        pointer: None,
        keyboard: None,
        regions: Vec::new(),
        bar_hover: None,
        hover_anim: hud::anim::AnimSet::new(started, HOVER_DURATION, anim),
        panel: None,
        popup: None,
        grab: None,
        tooltip: None,
        osd: None,
        osd_detector: hud::osd::OsdDetector::new(),
        osd_anim: hud::anim::Tween::settled(0.0, started),
        osd_fill: hud::anim::Tween::settled(0.0, started),
        popup_anim: hud::anim::Tween::settled(0.0, started),
        now: started,
        anim,
        mods: Modifiers::default(),
    };

    let mut buf = [0u8; 64];
    // Момент последней сверки профиля питания: чаще раза в 15 секунд файл
    // в `/sys` дёргать незачем.
    let mut power_check = started;
    while !app.exit {
        let current_settings_stamp = hud::settings::modified();
        if current_settings_stamp != settings_stamp {
            let new_settings = hud::settings::load();
            if new_settings.height != app.settings.height {
                app.layer.set_size(0, new_settings.height);
                app.layer.set_exclusive_zone(new_settings.height as i32 + 2);
                app.layer.commit();
            }
            app.settings = new_settings;
            // Настройки анимаций применяются сразу: выключение должно убрать
            // движение, а не оставить текущий твин идти до конца.
            // Энергосбережение учитывается и здесь, иначе правка настроек
            // включила бы анимации обратно в обход профиля.
            anim_on = app.settings.animations && power_profile::anim_allowed();
            app.anim = hud::anim::AnimSettings::new(anim_on, app.settings.animation_speed);
            app.hover_anim.set_settings(app.anim);
            app.popup_anim.duration = app.anim.scaled(app.popup_anim.duration);
            settings_stamp = current_settings_stamp;
            app.shared.dirty.store(true, Ordering::Relaxed);
        }
        // Профиль питания перечитывается редко и без настроек: переключение в
        // энергосбережение на ходу тоже должно гасить анимации, а возврат —
        // возвращать их без перезапуска панели.
        if app.now.duration_since(power_check) >= std::time::Duration::from_secs(15) {
            power_check = app.now;
            let on = app.settings.animations && power_profile::anim_allowed();
            if on != anim_on {
                anim_on = on;
                app.anim = hud::anim::AnimSettings::new(on, app.settings.animation_speed);
                app.hover_anim.set_settings(app.anim);
                app.popup_anim.duration = app.anim.scaled(app.popup_anim.duration);
                app.shared.dirty.store(true, Ordering::Relaxed);
            }
        }
        // Один вызов `Local::now()` на оба формата: часы на баре и секунды
        // в панели — одна и та же секунда, а раньше на каждый проход цикла
        // приходилось два чтения системных часов и два форматирования.
        let now_local = chrono::Local::now();
        let now = now_local.format("%a, %d %b %H:%M").to_string();
        if now != app.clock {
            app.clock = now;
            app.shared.dirty.store(true, Ordering::Relaxed);
        }
        let secs = now_local.format("%H:%M:%S").to_string();
        if secs != app.clock_secs {
            app.clock_secs = secs;
            if app
                .panel
                .as_ref()
                .is_some_and(|p| p.kind == PanelKind::Clock)
            {
                app.shared.dirty.store(true, Ordering::Relaxed);
            }
        }
        // Палитра перечитывается только при изменении mtime `colors.css`.
        // Полный разбор на каждом проходе цикла — на десятки stat'ов в секунду
        // во время анимаций — заменён stat'ом, образец тот же, что у настроек.
        // matugen перезаписывает файл и обновляет mtime, поэтому подхват
        // новых цветов по-прежнему без перезапуска.
        let current_palette_stamp = hud::palette::modified();
        if current_palette_stamp != palette_stamp {
            let pal = hud::palette::load();
            if pal != app.palette {
                app.palette = pal;
                app.shared.dirty.store(true, Ordering::Relaxed);
            }
            palette_stamp = current_palette_stamp;
        }

        let _ = event_queue.flush();

        // Момент для всего кадра: анимации и отрисовка читают одно время.
        app.now = std::time::Instant::now();
        // Анимации сами решают, что изменилось: пока хоть одно значение движется,
        // кадр нужен, иначе движение встало бы на паузе до следующего события.
        // Это же значение уходит в таймаут ожидания события ниже.
        let animating = app.tick_animations();
        if animating {
            app.shared.dirty.store(true, Ordering::Relaxed);
        }
        // OSD сверяется каждый тик: окно живёт по своему сроку и не связано с
        // обновлениями панели, поэтому пока оно живо, ждать приходится недолго.
        app.sync_osd(&qh);
        let osd_alive = app.osd_alive();
        if osd_alive {
            app.shared.dirty.store(true, Ordering::Relaxed);
        }

        // Окно OSD не анимируется, поэтому ему хватает редкого тика: 50 мс
        // добавляет задержку скрытия, которой на глаз не видно.
        let timeout = if animating {
            16u16
        } else if osd_alive {
            50
        } else {
            500
        };

        if let Some(guard) = event_queue.prepare_read() {
            let mut fds = vec![nix::poll::PollFd::new(
                wake_rx.as_fd(),
                nix::poll::PollFlags::POLLIN,
            )];
            fds.push(nix::poll::PollFd::new(
                conn.as_fd(),
                nix::poll::PollFlags::POLLIN,
            ));
            let _ = nix::poll::poll(&mut fds, timeout);

            let pipe_ready = fds[0]
                .revents()
                .is_some_and(|r| r.intersects(nix::poll::PollFlags::POLLIN));
            if pipe_ready {
                while let Ok(n) = nix::unistd::read(&wake_rx, &mut buf) {
                    if n == 0 {
                        break;
                    }
                }
            }
            let wl_ready = fds
                .get(1)
                .and_then(|p| p.revents())
                .is_some_and(|r| r.intersects(nix::poll::PollFlags::POLLIN));
            if wl_ready {
                let _ = guard.read();
            } else {
                drop(guard);
            }
        }

        // Потеря соединения с композитором — единственный настоящий повод
        // завершиться. Раньше этим поводом считалось и закрытие surface
        // панели, из-за чего процесс уходил при гашении экрана по крышке.
        if event_queue.dispatch_pending(&mut app).is_err() {
            app.exit = true;
        }

        // Панель пересоздаётся после возврата выхода: пока выхода нет,
        // `configure` не придёт, и отрисовка просто не начнётся.
        if app.recreate_panel {
            app.layer = create_panel(&app.compositor, &app.layer_shell, &qh, app.settings.height);
            app.recreate_panel = false;
            app.configured = false;
            app.first_draw = true;
            app.width = 0;
        }

        let dirty = app.shared.dirty.swap(false, Ordering::Relaxed);
        if dirty || app.first_draw {
            app.first_draw = false;
            app.frame();
        }
    }
}
