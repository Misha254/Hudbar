use super::*;

impl App {
    pub fn set_bar_hover(&mut self, x: f32, y: f32, qh: &QueueHandle<Self>) {
        let hit = self
            .regions
            .iter()
            .rev()
            .find(|(_, r)| r.contains(x, y))
            .map(|(h, _)| *h);
        let hit = match hit {
            Some(
                Hit::PanelRow(_) | Hit::PanelSlider | Hit::CalPrev | Hit::CalNext | Hit::CalToday,
            ) => None,
            other => other,
        };
        if hit != self.bar_hover {
            self.bar_hover = hit;
            if let Some(h) = hit {
                self.hover_anim = Some((h, 0.0));
            }
            self.sync_tooltip(qh);
            self.shared.dirty.store(true, Ordering::Relaxed);
        }
    }

    pub fn hover_alpha(&self, h: Hit) -> f32 {
        match self.hover_anim {
            Some((ah, t)) if ah == h => t,
            _ => 0.0,
        }
    }

    pub fn on_bar_click(&mut self, x: f32, y: f32, qh: &QueueHandle<Self>) {
        self.destroy_tooltip();
        let hit = self
            .regions
            .iter()
            .rev()
            .find(|(_, r)| r.contains(x, y))
            .map(|(h, _)| *h);
        match hit {
            Some(Hit::WifiChip) => self.toggle_panel(PanelKind::Wifi, qh),
            Some(Hit::BtChip) => self.toggle_panel(PanelKind::Bt, qh),
            Some(Hit::ClockChip) => self.toggle_panel(PanelKind::Clock, qh),
            Some(Hit::WeatherChip) => self.toggle_panel(PanelKind::Weather, qh),
            Some(Hit::VolChip) => self.toggle_panel(PanelKind::Volume, qh),
            Some(Hit::MicChip) => self.toggle_panel(PanelKind::Mic, qh),
            Some(Hit::Workspace(i)) => {
                let sys = self.shared.sys.lock().unwrap().clone();
                if let Some(w) = sys.workspaces.get(i) {
                    crate::hud::actions::focus_workspace(&w.idx.to_string());
                }
            }
            Some(Hit::TrayItem(i)) => {
                let sys = self.shared.sys.lock().unwrap().clone();
                let items: Vec<tray::TrayItem> =
                    sys.tray.iter().filter(|t| t.visible()).cloned().collect();
                if let Some(item) = items.get(i) {
                    // Окно есть — фокусим (niri перекинет на его стол), нет — Activate.
                    let mut keys = vec![
                        item.sni_id.clone(),
                        item.icon_name.clone(),
                        item.title.clone(),
                    ];
                    if !item.service.starts_with(':') {
                        keys.extend(
                            item.service
                                .split('.')
                                .filter(|t| {
                                    let t = t.to_lowercase();
                                    !matches!(
                                        t.as_str(),
                                        "org"
                                            | "com"
                                            | "io"
                                            | "net"
                                            | "kde"
                                            | "gnome"
                                            | "freedesktop"
                                            | "desktop"
                                            | "app"
                                            | "daemon"
                                            | "indicator"
                                            | "status"
                                            | "notifier"
                                            | "github"
                                    )
                                })
                                .map(str::to_string),
                        );
                    }
                    if !crate::hud::actions::focus_tray_window(&keys) {
                        tray::activate_item(item.service.clone(), item.path.clone());
                    }
                }
            }
            _ => {
                if self.panel.is_some() {
                    self.close_panel();
                }
            }
        }
    }

    pub fn on_bar_right_click(&mut self, x: f32, y: f32, qh: &QueueHandle<Self>) {
        self.destroy_tooltip();
        let hit = self
            .regions
            .iter()
            .rev()
            .find(|(_, r)| r.contains(x, y))
            .map(|(h, _)| *h);
        match hit {
            Some(Hit::TrayItem(i)) => {
                let sys = self.shared.sys.lock().unwrap().clone();
                let items: Vec<tray::TrayItem> =
                    sys.tray.iter().filter(|t| t.visible()).cloned().collect();
                if let Some(item) = items.get(i) {
                    self.open_panel(PanelKind::TrayMenu { idx: i }, qh);
                    data::refresh_tray_menu(&self.shared, &item.service, &item.menu);
                }
            }
            _ => {
                if self.panel.is_some() {
                    self.close_panel();
                }
            }
        }
    }

    pub fn radio_state(&self, kind: PanelKind) -> bool {
        let sys = self.shared.sys.lock().unwrap();
        match kind {
            PanelKind::Wifi => sys.wifi_enabled,
            PanelKind::Bt => sys.bt_on,
            PanelKind::Volume => !sys.vol_muted,
            PanelKind::Mic => !sys.mic_muted,
            _ => false,
        }
    }

    pub fn toggle_hit(&self, x: f32, y: f32) -> Option<PanelKind> {
        let panel = self.panel.as_ref()?;
        if !matches!(
            panel.kind,
            PanelKind::Wifi | PanelKind::Bt | PanelKind::Volume | PanelKind::Mic
        ) {
            return None;
        }
        if matches!(panel.view, PanelView::Password { .. }) {
            return None;
        }
        let w = self.popup.as_ref()?.size.0 as f32;
        let tx = w - PANEL_PAD - PANEL_TOGGLE_W;
        let ty = (PANEL_HEADER - PANEL_TOGGLE_H) / 2.0;
        (x >= tx && x <= tx + PANEL_TOGGLE_W && y >= ty && y <= ty + PANEL_TOGGLE_H)
            .then_some(panel.kind)
    }

    pub fn on_panel_click(&mut self, x: f32, y: f32) {
        let Some(kind) = self.panel.as_ref().map(|p| p.kind) else {
            return;
        };
        if matches!(
            self.panel.as_ref().map(|p| &p.view),
            Some(PanelView::Password { .. })
        ) {
            return;
        }
        match kind {
            PanelKind::Clock => {
                self.on_clock_click(x, y);
                return;
            }
            PanelKind::Weather => return,
            PanelKind::Volume | PanelKind::Mic => {
                if self.toggle_hit(x, y).is_some() {
                    match kind {
                        PanelKind::Volume => crate::hud::actions::vol_mute_toggle(&self.shared),
                        PanelKind::Mic => crate::hud::actions::mic_mute_toggle(&self.shared),
                        _ => {}
                    }
                    return;
                }
                let slider = self
                    .regions
                    .iter()
                    .rev()
                    .find(|(h, _)| matches!(h, Hit::PanelSlider))
                    .map(|(_, r)| *r);
                if let Some(r) = slider
                    && r.contains(x, y)
                {
                    let frac = ((x - r.x) / r.w).clamp(0.0, 1.0);
                    let pct = (frac * 100.0).round() as u8;
                    match kind {
                        PanelKind::Volume => crate::hud::actions::vol_set(&self.shared, pct),
                        PanelKind::Mic => crate::hud::actions::mic_set(&self.shared, pct),
                        _ => {}
                    }
                    return;
                }
                let y0 = PANEL_HEADER + AV_PCT_H + AV_SLIDER_H + AV_SECTION_H;
                if y >= y0 {
                    let idx = ((y - y0) / PANEL_ROW) as usize;
                    let rows = self.av_devices();
                    if let Some((label, _, _, node)) = rows.get(idx)
                        && !is_hint(label)
                        && !node.is_empty()
                    {
                        let is_sink = matches!(kind, PanelKind::Volume);
                        data::audio_set_default(&self.shared, node, is_sink);
                    }
                }
                return;
            }
            _ => {}
        }
        if self.toggle_hit(x, y).is_some() {
            let on = self.radio_state(kind);
            match kind {
                PanelKind::Wifi => data::wifi_radio(&self.shared, !on),
                PanelKind::Bt => data::bt_power(&self.shared, !on),
                _ => {}
            }
            return;
        }
        if y < PANEL_HEADER {
            return;
        }
        let idx = ((y - PANEL_HEADER) / PANEL_ROW) as usize;
        let rows = self.panel_rows();
        let Some((label, _, _)) = rows.get(idx) else {
            return;
        };
        if is_hint(label) {
            return;
        }
        let sys = self.shared.sys.lock().unwrap().clone();
        match kind {
            PanelKind::Wifi => {
                if let Some(n) = sys.wifi_list.get(idx) {
                    if n.in_use {
                        data::wifi_disconnect(&self.shared);
                    } else if n.security.is_empty() || n.security == "--" {
                        data::wifi_connect(&self.shared, &n.ssid, false);
                    } else if let Some(pn) = self.panel.as_mut() {
                        pn.view = PanelView::Password {
                            ssid: n.ssid.clone(),
                            input: String::new(),
                        };
                        self.shared.dirty.store(true, Ordering::Relaxed);
                    }
                }
            }
            PanelKind::Bt => {
                if let Some(d) = sys.bt_list.get(idx) {
                    crate::hud::actions::bt_toggle(&self.shared, &d.mac, d.connected);
                }
            }
            PanelKind::TrayMenu { .. } => {
                if let Some(e) = sys.tray_menu.get(idx)
                    && e.enabled
                {
                    data::send_tray_menu_click(&self.shared, e.id);
                    self.close_panel();
                }
            }
            _ => {}
        }
    }

    pub fn on_panel_hover(&mut self, x: f32, y: f32) {
        let hover_toggle = self.toggle_hit(x, y).is_some();
        let kind = self.panel.as_ref().map(|p| p.kind);
        let hover = if !hover_toggle {
            if matches!(kind, Some(PanelKind::Volume | PanelKind::Mic)) {
                let y0 = PANEL_HEADER + AV_PCT_H + AV_SLIDER_H + AV_SECTION_H;
                if y >= y0 {
                    let i = ((y - y0) / PANEL_ROW) as usize;
                    if i < self.av_devices().len() {
                        Some(i)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else if y >= PANEL_HEADER {
                let rows = self.panel_rows().len();
                let i = ((y - PANEL_HEADER) / PANEL_ROW) as usize;
                if i < rows { Some(i) } else { None }
            } else {
                None
            }
        } else {
            None
        };
        if let Some(p) = self.panel.as_mut()
            && (p.hover != hover || p.hover_toggle != hover_toggle)
        {
            p.hover = hover;
            p.hover_toggle = hover_toggle;
            self.shared.dirty.store(true, Ordering::Relaxed);
        }
    }
}
