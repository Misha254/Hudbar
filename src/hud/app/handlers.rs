use std::sync::atomic::Ordering;

use smithay_client_toolkit::{
    compositor::CompositorHandler,
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    Connection, QueueHandle,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};

use super::render::App;
use super::ui::*;
use crate::hud::data;

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
        }
        if capability == Capability::Keyboard
            && let Some(k) = self.keyboard.take()
        {
            k.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let bar = self.layer.wl_surface().clone();
        let popup_surface = self.popup.as_ref().map(|p| p.layer.wl_surface().clone());
        let grab_surface = self.grab.as_ref().map(|g| g.layer.wl_surface().clone());
        for ev in events {
            let on_bar = ev.surface == bar;
            let on_popup = popup_surface.as_ref().is_some_and(|s| ev.surface == *s);
            let on_grab = grab_surface.as_ref().is_some_and(|s| ev.surface == *s);
            match &ev.kind {
                PointerEventKind::Enter { .. } => {
                    if on_popup {
                        self.on_panel_hover(ev.position.0 as f32, ev.position.1 as f32);
                    } else if on_bar {
                        self.set_bar_hover(ev.position.0 as f32, ev.position.1 as f32, qh);
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if on_popup {
                        if let Some(p) = self.panel.as_mut()
                            && p.hover.take().is_some()
                        {
                            self.shared.dirty.store(true, Ordering::Relaxed);
                        }
                    } else if on_bar {
                        if self.bar_hover.take().is_some() {
                            self.shared.dirty.store(true, Ordering::Relaxed);
                        }
                        self.destroy_tooltip();
                    }
                }
                PointerEventKind::Motion { .. } => {
                    if on_popup {
                        self.on_panel_hover(ev.position.0 as f32, ev.position.1 as f32);
                    } else if on_bar {
                        self.set_bar_hover(ev.position.0 as f32, ev.position.1 as f32, qh);
                    }
                }
                PointerEventKind::Press { button, .. } if *button == 0x110 => {
                    if on_bar {
                        self.on_bar_click(ev.position.0 as f32, ev.position.1 as f32, qh);
                    } else if on_popup {
                        self.on_panel_click(ev.position.0 as f32, ev.position.1 as f32);
                    } else if on_grab {
                        self.close_panel();
                    }
                }
                PointerEventKind::Press { button, .. } if *button == 0x111 => {
                    if on_bar {
                        self.on_bar_right_click(ev.position.0 as f32, ev.position.1 as f32, qh);
                    } else if on_popup || on_grab {
                        self.close_panel();
                    }
                }
                _ => {}
            }
        }
    }
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        let is_password = matches!(
            self.panel.as_ref().map(|p| &p.view),
            Some(PanelView::Password { .. })
        );
        if !is_password {
            if event.keysym == Keysym::Escape && self.panel.is_some() {
                self.close_panel();
            }
            return;
        }
        match event.keysym {
            Keysym::Escape => {
                if let Some(pn) = self.panel.as_mut() {
                    pn.view = PanelView::List;
                }
                self.shared.dirty.store(true, Ordering::Relaxed);
            }
            Keysym::Return | Keysym::KP_Enter => {
                if let Some(PanelView::Password { ssid, input }) =
                    self.panel.as_ref().map(|p| p.view.clone())
                {
                    data::wifi_connect_pass(&self.shared, &ssid, &input);
                    if let Some(pn) = self.panel.as_mut() {
                        pn.view = PanelView::List;
                    }
                }
            }
            Keysym::BackSpace => {
                if let Some(PanelView::Password { input, .. }) =
                    self.panel.as_mut().map(|p| &mut p.view)
                {
                    input.pop();
                }
                self.shared.dirty.store(true, Ordering::Relaxed);
            }
            _ => {
                if self.mods.ctrl || self.mods.alt {
                    return;
                }
                if let Some(text) = &event.utf8 {
                    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !clean.is_empty() {
                        if let Some(PanelView::Password { input, .. }) =
                            self.panel.as_mut().map(|p| &mut p.view)
                        {
                            input.push_str(&clean);
                        }
                        self.shared.dirty.store(true, Ordering::Relaxed);
                    }
                }
            }
        }
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.mods = modifiers;
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        if layer.wl_surface() == self.layer.wl_surface() {
            self.exit = true;
        } else if self
            .popup
            .as_ref()
            .is_some_and(|p| layer.wl_surface() == p.layer.wl_surface())
            || self
                .grab
                .as_ref()
                .is_some_and(|g| layer.wl_surface() == g.layer.wl_surface())
        {
            self.panel = None;
            self.popup = None;
            self.grab = None;
        } else if self
            .tooltip
            .as_ref()
            .is_some_and(|t| layer.wl_surface() == t.layer.wl_surface())
        {
            self.tooltip = None;
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if layer.wl_surface() == self.layer.wl_surface() {
            if configure.new_size.0 != 0 {
                self.width = configure.new_size.0;
            }
            if configure.new_size.1 != 0 {
                self.height = configure.new_size.1;
            }
            self.configured = true;
            self.first_draw = true;
            return;
        }
        if let Some(popup) = self.popup.as_mut()
            && layer.wl_surface() == popup.layer.wl_surface()
        {
            popup.configured = true;
            if configure.new_size.0 != 0 {
                popup.size.0 = configure.new_size.0;
            }
            if configure.new_size.1 != 0 {
                popup.size.1 = configure.new_size.1;
            }
            self.shared.dirty.store(true, Ordering::Relaxed);
        }
        if let Some(grab) = self.grab.as_mut()
            && layer.wl_surface() == grab.layer.wl_surface()
        {
            grab.configured = true;
            if configure.new_size.0 != 0 {
                grab.size.0 = configure.new_size.0;
            }
            if configure.new_size.1 != 0 {
                grab.size.1 = configure.new_size.1;
            }
            grab.region = None;
            self.shared.dirty.store(true, Ordering::Relaxed);
        }
        if let Some(tip) = self.tooltip.as_mut()
            && layer.wl_surface() == tip.layer.wl_surface()
        {
            tip.configured = true;
            if configure.new_size.0 != 0 {
                tip.size.0 = configure.new_size.0;
            }
            if configure.new_size.1 != 0 {
                tip.size.1 = configure.new_size.1;
            }
            self.shared.dirty.store(true, Ordering::Relaxed);
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(App);
smithay_client_toolkit::delegate_dispatch2!(App);
