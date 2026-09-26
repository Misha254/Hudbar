mod hud;

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
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell},
    },
    shm::{Shm, slot::SlotPool},
};
use wayland_client::{Connection, globals::registry_queue_init};

use hud::App;
use hud::ui::*;

fn main() {
    let hud_settings = hud::settings::load();
    let mut settings_stamp = hud::settings::modified();
    let conn = Connection::connect_to_env().expect("wayland connection");
    let (globals, mut event_queue) = registry_queue_init(&conn).expect("registry");
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("wlr-layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm");

    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some("hudbar"), None);
    let reserve = hud_settings.height as i32 + 2;
    layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(0, hud_settings.height);
    layer.set_exclusive_zone(reserve);
    layer.set_margin(1, 0, 1, 0);
    layer.commit();

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
        hover_anim: None,
        panel: None,
        popup: None,
        grab: None,
        tooltip: None,
        popup_t: 0.0,
        popup_target: 0.0,
        last_tick: std::time::Instant::now(),
        mods: Modifiers::default(),
    };

    let mut buf = [0u8; 64];
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
            settings_stamp = current_settings_stamp;
            app.shared.dirty.store(true, Ordering::Relaxed);
        }
        let now = chrono::Local::now().format("%a, %d %b %H:%M").to_string();
        if now != app.clock {
            app.clock = now;
            app.shared.dirty.store(true, Ordering::Relaxed);
        }
        let secs = chrono::Local::now().format("%H:%M:%S").to_string();
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
        let pal = hud::palette::load();
        if pal != app.palette {
            app.palette = pal;
            app.shared.dirty.store(true, Ordering::Relaxed);
        }

        let _ = event_queue.flush();

        let dt = {
            let t = std::time::Instant::now();
            let dt = t.duration_since(app.last_tick).as_secs_f32();
            app.last_tick = t;
            dt
        };
        if (app.popup_t - app.popup_target).abs() > 0.001 {
            let dir = (app.popup_target - app.popup_t).signum();
            app.popup_t = (app.popup_t + dir * dt / 0.16).clamp(0.0, 1.0);
            if (app.popup_target - app.popup_t).abs() < 0.02 {
                app.popup_t = app.popup_target;
            }
            app.shared.dirty.store(true, Ordering::Relaxed);
        }
        if let Some((hit, t)) = app.hover_anim {
            let target = if app.bar_hover == Some(hit) { 1.0 } else { 0.0 };
            let speed = dt / 0.12;
            let nt = if target > t {
                (t + speed).min(target)
            } else {
                (t - speed).max(target)
            };
            if (nt - t).abs() > 0.0001 {
                app.hover_anim = Some((hit, nt));
                app.shared.dirty.store(true, Ordering::Relaxed);
            }
            if nt <= 0.0 && target == 0.0 {
                app.hover_anim = None;
            }
        }
        let animating = (app.popup_t - app.popup_target).abs() > 0.001
            || app.hover_anim.is_some_and(|(h, t)| {
                (t - if app.bar_hover == Some(h) { 1.0 } else { 0.0 }).abs() > 0.001
            });

        if let Some(guard) = event_queue.prepare_read() {
            let mut fds = vec![nix::poll::PollFd::new(
                wake_rx.as_fd(),
                nix::poll::PollFlags::POLLIN,
            )];
            fds.push(nix::poll::PollFd::new(
                conn.as_fd(),
                nix::poll::PollFlags::POLLIN,
            ));
            let _ = nix::poll::poll(&mut fds, if animating { 16u16 } else { 500u16 });

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

        let _ = event_queue.dispatch_pending(&mut app);

        let dirty = app.shared.dirty.swap(false, Ordering::Relaxed);
        if dirty || app.first_draw {
            app.first_draw = false;
            app.frame();
        }
    }
}
