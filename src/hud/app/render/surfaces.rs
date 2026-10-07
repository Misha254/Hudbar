use super::*;

impl App {
    pub fn frame(&mut self) {
        self.regions.clear();

        if self.panel.is_some() && self.popup_t <= 0.001 && self.popup_target <= 0.001 {
            self.destroy_popup();
        }

        if self.panel.is_some() {
            let desired = self.panel_height();
            if let Some(popup) = self.popup.as_mut()
                && popup.configured
                && popup.size.1 != desired
            {
                popup.size.1 = desired;
                popup.layer.set_size(popup.size.0, desired);
                popup.layer.commit();
                return;
            }
            self.draw_popup();
        }

        if self.grab.is_some() {
            self.draw_grab();
        }

        if self.configured && self.width > 0 {
            self.draw_bar();
        }

        if self.tooltip.is_some() {
            self.draw_tooltip();
        }

        if self.osd.is_some() {
            self.draw_osd();
        }
    }

    pub fn draw_grab(&mut self) {
        let Some((w, h)) = self.grab.as_ref().map(|g| g.size) else {
            return;
        };
        if w == 0 || h == 0 {
            return;
        }
        if self.grab.as_ref().is_some_and(|g| g.region.is_none())
            && let Ok(region) = Region::new(&self.compositor)
        {
            let reserve = self.settings.height as i32 + 2;
            region.add(0, reserve, w as i32, (h as i32 - reserve).max(0));
            if let Some(grab) = self.grab.as_mut() {
                grab.layer
                    .wl_surface()
                    .set_input_region(Some(region.wl_region()));
                grab.region = Some(region);
            }
        }
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(w as i32, h as i32, (w * 4) as i32, wl_shm::Format::Argb8888)
        else {
            return;
        };
        canvas.fill(0);
        if let Some(grab) = self.grab.as_ref() {
            let surface = grab.layer.wl_surface();
            surface.damage_buffer(0, 0, w as i32, h as i32);
            let _ = buffer.attach_to(surface);
            grab.layer.commit();
        }
    }

    pub fn draw_bar(&mut self) {
        let width = self.width;
        let height = self.height;
        let pw = (width as f32 * SCALE) as u32;
        let ph = (height as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        self.render(&mut pixmap);

        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
        else {
            return;
        };
        let (dst_chunks, _) = canvas.as_chunks_mut::<4>();
        let (src_chunks, _) = pixmap.data().as_chunks::<4>();
        for (dst, src) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
        let surface = self.layer.wl_surface();
        surface.set_buffer_scale(SCALE as i32);
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        let _ = buffer.attach_to(surface);
        self.layer.commit();
    }

    pub fn draw_popup(&mut self) {
        let Some((configured, size)) = self.popup.as_ref().map(|p| (p.configured, p.size)) else {
            return;
        };
        if !configured {
            return;
        }
        let pw = (size.0 as f32 * SCALE) as u32;
        let ph = (size.1 as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        self.render_panel(&mut pixmap);

        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
        else {
            return;
        };
        let a = self.popup_t.clamp(0.0, 1.0);
        let (dst_chunks, _) = canvas.as_chunks_mut::<4>();
        let (src_chunks, _) = pixmap.data().as_chunks::<4>();
        for (dst, src) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
            dst[0] = (src[2] as f32 * a) as u8;
            dst[1] = (src[1] as f32 * a) as u8;
            dst[2] = (src[0] as f32 * a) as u8;
            dst[3] = (src[3] as f32 * a) as u8;
        }
        if let Some(popup) = self.popup.as_ref() {
            let surface = popup.layer.wl_surface();
            surface.set_buffer_scale(SCALE as i32);
            surface.damage_buffer(0, 0, pw as i32, ph as i32);
            let _ = buffer.attach_to(surface);
            popup.layer.commit();
        }
    }

    // ---------- interactions ----------

    pub fn destroy_tooltip(&mut self) {
        if self.tooltip.is_some() {
            self.tooltip = None;
            self.shared.dirty.store(true, Ordering::Relaxed);
        }
    }

    pub fn tooltip_want(&mut self) -> Option<(String, f32)> {
        let hit = self.bar_hover?;
        let idx = match hit {
            Hit::TrayItem(i) => i,
            _ => return None,
        };
        let sys = crate::hud::lock::mutex(&self.shared.sys).clone();
        let items: Vec<tray::TrayItem> = sys.tray.iter().filter(|t| t.visible()).cloned().collect();
        let item = items.get(idx)?;
        let name = item.display_name();
        if name.is_empty() {
            return None;
        }
        let cx = self
            .regions
            .iter()
            .rev()
            .find(|(h, _)| *h == hit)
            .map(|(_, r)| r.x + r.w / 2.0)?;
        Some((name, cx))
    }

    pub fn sync_tooltip(&mut self, qh: &QueueHandle<Self>) {
        let want = self.tooltip_want();
        match (&self.tooltip, &want) {
            (Some(cur), Some((text, _))) if cur.text == *text => {}
            (None, None) => {}
            (_, want) => {
                self.tooltip = None;
                if let Some((text, cx)) = want.clone() {
                    self.create_tooltip(qh, &text, cx);
                }
                self.shared.dirty.store(true, Ordering::Relaxed);
            }
        }
    }

    pub fn create_tooltip(&mut self, qh: &QueueHandle<Self>, text: &str, cx: f32) {
        let tw = self.text_width(text, SIZE) / SCALE;
        let w = (tw + TOOLTIP_PAD_X * 2.0).max(40.0);
        let h = TOOLTIP_H;
        let max_x = (self.width as f32 - w - 4.0).max(0.0);
        let px = (cx - w / 2.0).min(max_x).max(4.0) as i32;
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Top,
            Some("hudbar-tooltip"),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(w as u32, h as u32);
        layer.set_margin(self.settings.height as i32 + 3, 0, 0, px);
        layer.set_exclusive_zone(-1);
        // Пустой input-region: тултип прозрачен для мыши, ховер бара не рвётся.
        if let Ok(region) = Region::new(&self.compositor) {
            layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        layer.commit();
        self.tooltip = Some(Tooltip {
            layer,
            configured: false,
            size: (w as u32, h as u32),
            text: text.to_string(),
        });
    }

    pub fn draw_tooltip(&mut self) {
        let Some((configured, size, text)) = self
            .tooltip
            .as_ref()
            .map(|t| (t.configured, t.size, t.text.clone()))
        else {
            return;
        };
        if !configured || size.0 == 0 || size.1 == 0 {
            return;
        }
        let p = self.palette;
        let pw = (size.0 as f32 * SCALE) as u32;
        let ph = (size.1 as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        let bw = 1.0 * SCALE;
        let border = p.primary.with_a(0.35);
        pixmap.fill(p.base.with_a(0.98).to_tiny());
        fill_rect(&mut pixmap, 0.0, 0.0, pw as f32, bw, border);
        fill_rect(&mut pixmap, 0.0, ph as f32 - bw, pw as f32, bw, border);
        fill_rect(&mut pixmap, 0.0, 0.0, bw, ph as f32, border);
        fill_rect(&mut pixmap, pw as f32 - bw, 0.0, bw, ph as f32, border);
        let tw = self.text_width(&text, SIZE) / SCALE;
        let tx = ((size.0 as f32 * SCALE - tw * SCALE) / 2.0).max(0.0);
        self.draw_text(&mut pixmap, &text, SIZE, p.text, tx, 0.0, TOOLTIP_H);
        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
        else {
            return;
        };
        let (dst_chunks, _) = canvas.as_chunks_mut::<4>();
        let (src_chunks, _) = pixmap.data().as_chunks::<4>();
        for (dst, src) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
        if let Some(tip) = self.tooltip.as_ref() {
            let surface = tip.layer.wl_surface();
            surface.set_buffer_scale(SCALE as i32);
            surface.damage_buffer(0, 0, pw as i32, ph as i32);
            let _ = buffer.attach_to(surface);
            tip.layer.commit();
        }
    }

    pub fn close_panel(&mut self) {
        if self.panel.is_none() {
            return;
        }
        self.popup_target = 0.0;
        self.shared.dirty.store(true, Ordering::Relaxed);
    }

    pub fn destroy_popup(&mut self) {
        self.panel = None;
        self.popup = None;
        self.grab = None;
        self.popup_t = 0.0;
        self.regions.retain(|(h, _)| !matches!(h, Hit::PanelRow(_)));
        self.shared.dirty.store(true, Ordering::Relaxed);
    }

    pub fn open_panel(&mut self, kind: PanelKind, qh: &QueueHandle<Self>) {
        let view = match kind {
            PanelKind::Clock => {
                let now = chrono::Local::now();
                PanelView::Calendar {
                    year: now.year(),
                    month: now.month(),
                }
            }
            _ => PanelView::List,
        };
        self.panel = Some(Panel {
            kind,
            hover: None,
            hover_toggle: false,
            view,
        });
        self.popup_t = 0.0;
        self.popup_target = 1.0;
        self.popup = None;
        self.grab = None;
        match kind {
            PanelKind::Wifi => data::refresh_wifi(&self.shared),
            PanelKind::Bt => data::refresh_bt(&self.shared),
            PanelKind::Weather => data::refresh_weather(&self.shared),
            PanelKind::Control => {
                data::refresh_wifi(&self.shared);
                data::refresh_bt(&self.shared);
                data::refresh_audio(&self.shared);
            }
            PanelKind::Volume | PanelKind::Mic => data::refresh_audio(&self.shared),
            PanelKind::Clock | PanelKind::TrayMenu { .. } => {}
        }
        let mut cx = self
            .regions
            .iter()
            .find_map(|(h, r)| {
                if matches!(
                    (h, kind),
                    (Hit::WifiChip, PanelKind::Wifi)
                        | (Hit::BtChip, PanelKind::Bt)
                        | (Hit::ClockChip, PanelKind::Clock)
                        | (Hit::WeatherChip, PanelKind::Weather)
                        | (Hit::VolChip, PanelKind::Volume)
                        | (Hit::MicChip, PanelKind::Mic)
                        | (Hit::ControlChip, PanelKind::Control)
                ) {
                    Some(r.x + r.w / 2.0)
                } else {
                    None
                }
            })
            .unwrap_or(self.width as f32 / 2.0);
        if let PanelKind::TrayMenu { idx } = kind
            && let Some((_, r)) = self
                .regions
                .iter()
                .rev()
                .find(|(h, _)| *h == Hit::TrayItem(idx))
        {
            cx = r.x + r.w / 2.0;
        }
        let base_w = match kind {
            PanelKind::Weather => PANEL_W_WIDE,
            PanelKind::Clock => PANEL_W_CLOCK,
            PanelKind::Volume | PanelKind::Mic => PANEL_W_AV,
            PanelKind::Control => PANEL_W,
            _ => PANEL_W,
        };
        let w = base_w.min(self.width as f32 - 8.0).max(200.0);
        let max_x = (self.width as f32 - w - 4.0).max(0.0);
        let px = (cx - w / 2.0).min(max_x).max(4.0) as i32;
        let h = self.panel_height();

        let grab_surface = self.compositor.create_surface(qh);
        let grab_layer = self.layer_shell.create_layer_surface(
            qh,
            grab_surface,
            Layer::Top,
            Some("hudbar-grab"),
            None,
        );
        grab_layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        grab_layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        grab_layer.set_size(0, 0);
        grab_layer.set_exclusive_zone(-1);
        grab_layer.commit();
        self.grab = Some(Grab {
            layer: grab_layer,
            configured: false,
            size: (0, 0),
            region: None,
        });

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Top,
            Some("hudbar-popup"),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        layer.set_size(w as u32, h);
        layer.set_margin(self.settings.height as i32 + 2, 0, 0, px);
        layer.set_exclusive_zone(-1);
        layer.commit();
        self.popup = Some(Popup {
            layer,
            configured: false,
            size: (w as u32, h),
        });
        self.shared.dirty.store(true, Ordering::Relaxed);
    }

    pub fn toggle_panel(&mut self, kind: PanelKind, qh: &QueueHandle<Self>) {
        let is_open =
            self.panel.as_ref().is_some_and(|p| p.kind == kind) && self.popup_target > 0.5;
        if is_open {
            self.close_panel();
        } else {
            self.open_panel(kind, qh);
        }
    }
}
