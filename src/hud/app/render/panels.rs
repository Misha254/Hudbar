use super::*;

fn level_label(level: u8, muted: bool) -> String {
    if muted {
        "выкл".to_string()
    } else {
        format!("{level}%")
    }
}

impl App {
    pub fn control_rows(&self) -> Vec<(String, String, bool)> {
        let sys = self.shared.sys.lock().unwrap().clone();
        vec![
            (
                "Wi-Fi".to_string(),
                toggle_label(sys.wifi_enabled),
                sys.wifi_enabled,
            ),
            ("Bluetooth".to_string(), toggle_label(sys.bt_on), sys.bt_on),
            ("DND".to_string(), toggle_label(sys.dnd), sys.dnd),
            (
                "Звук".to_string(),
                level_label(sys.vol, sys.vol_muted),
                !sys.vol_muted,
            ),
            (
                "Микрофон".to_string(),
                level_label(sys.mic, sys.mic_muted),
                !sys.mic_muted,
            ),
            ("Настройки HUD".to_string(), "открыть".to_string(), false),
            ("Обои".to_string(), "выбрать".to_string(), false),
            ("Питание".to_string(), "меню".to_string(), false),
            ("Блокировка".to_string(), "запустить".to_string(), false),
        ]
    }

    pub fn panel_rows(&self) -> Vec<(String, String, bool)> {
        if matches!(
            self.panel.as_ref().map(|p| &p.view),
            Some(PanelView::Password { .. })
        ) {
            return Vec::new();
        }
        match self.panel.as_ref().map(|p| p.kind) {
            Some(PanelKind::Control) => self.control_rows(),
            _ => {
                let sys = self.shared.sys.lock().unwrap().clone();
                self.panel_rows_for_system(sys)
            }
        }
    }

    fn panel_rows_for_system(&self, sys: data::Sys) -> Vec<(String, String, bool)> {
        match self.panel.as_ref().map(|p| p.kind) {
            Some(PanelKind::Wifi) => {
                if !sys.wifi_enabled {
                    vec![("Выключено".to_string(), String::new(), false)]
                } else if let Some(error) = sys.wifi_error {
                    vec![("Ошибка Wi-Fi".to_string(), error, false)]
                } else if sys.wifi_list.is_empty() {
                    vec![(
                        if sys.wifi_loading {
                            "Поиск…"
                        } else {
                            "Нет сетей"
                        }
                        .to_string(),
                        String::new(),
                        false,
                    )]
                } else {
                    sys.wifi_list
                        .iter()
                        .map(|n| {
                            let info = if n.security.is_empty() || n.security == "--" {
                                format!("{}%", n.signal)
                            } else {
                                format!("{I_LOCK}{}{}%", self.icon_gap(), n.signal)
                            };
                            (n.ssid.clone(), info, n.in_use)
                        })
                        .collect()
                }
            }
            Some(PanelKind::Bt) => {
                if !sys.bt_on {
                    vec![("Выключено".to_string(), String::new(), false)]
                } else if let Some(error) = sys.bt_error {
                    vec![("Ошибка Bluetooth".to_string(), error, false)]
                } else if sys.bt_list.is_empty() {
                    vec![(
                        if sys.bt_loading {
                            "Поиск…"
                        } else {
                            "Нет устройств"
                        }
                        .to_string(),
                        String::new(),
                        false,
                    )]
                } else {
                    sys.bt_list
                        .iter()
                        .map(|d| {
                            let info = if d.connected {
                                format!("{I_CHECK}{}подключено", self.icon_gap())
                            } else {
                                String::new()
                            };
                            (d.name.clone(), info, d.connected)
                        })
                        .collect()
                }
            }
            Some(PanelKind::TrayMenu { .. }) => {
                if sys.tray_menu.is_empty() {
                    vec![(
                        if sys.tray_menu_loading {
                            "Поиск…"
                        } else {
                            "Нет меню"
                        }
                        .to_string(),
                        String::new(),
                        false,
                    )]
                } else {
                    sys.tray_menu
                        .iter()
                        .map(|e| (e.label.clone(), e.parent.clone(), false))
                        .collect()
                }
            }
            _ => Vec::new(),
        }
    }

    pub fn av_devices(&self) -> Vec<(String, String, bool, String)> {
        let sys = self.shared.sys.lock().unwrap().clone();
        let is_mic = self
            .panel
            .as_ref()
            .is_some_and(|p| p.kind == PanelKind::Mic);
        let (list, follow, target, empty_hint) = if is_mic {
            (
                sys.sources.clone(),
                sys.ee_follow_in,
                sys.ee_in_target.clone(),
                "Нет входов",
            )
        } else {
            (
                sys.sinks.clone(),
                sys.ee_follow_out,
                sys.ee_out_target.clone(),
                "Нет выходов",
            )
        };
        if let Some(error) = sys.audio_error {
            return vec![("Ошибка аудио".to_string(), error, false, String::new())];
        }
        if list.is_empty() {
            let hint = if sys.audio_loading {
                "Поиск…"
            } else {
                empty_hint
            }
            .to_string();
            return vec![(hint, String::new(), false, String::new())];
        }
        list.iter()
            .map(|d| {
                let eq_here = sys.ee_running
                    && (if follow {
                        d.is_default
                    } else {
                        d.name == target
                    });
                let info = match (d.is_default, eq_here) {
                    (true, true) => format!("{I_CHECK}{}EQ", self.icon_gap()),
                    (true, false) => I_CHECK.to_string(),
                    (false, true) => "EQ".to_string(),
                    _ => String::new(),
                };
                (d.label.clone(), info, d.is_default, d.name.clone())
            })
            .collect()
    }

    pub fn panel_height(&self) -> u32 {
        match self.panel.as_ref() {
            Some(p) if p.kind == PanelKind::Clock => {
                let (year, month) = match p.view {
                    PanelView::Calendar { year, month } => (year, month),
                    _ => {
                        let n = chrono::Local::now();
                        (n.year(), n.month())
                    }
                };
                (CLOCK_TITLE_H
                    + CLOCK_DATE_H
                    + CAL_WD_H
                    + cal_rows(year, month) as f32 * CAL_ROW_H
                    + PANEL_PAD_BOTTOM) as u32
            }
            Some(p) if p.kind == PanelKind::Control => {
                (PANEL_HEADER + self.control_rows().len() as f32 * PANEL_ROW + PANEL_PAD_BOTTOM)
                    as u32
            }
            Some(p) if p.kind == PanelKind::Weather => WX_H,
            Some(p) if matches!(p.kind, PanelKind::Volume | PanelKind::Mic) => {
                let rows = self.av_devices().len().max(1) as f32;
                (PANEL_HEADER
                    + AV_PCT_H
                    + AV_SLIDER_H
                    + AV_SECTION_H
                    + rows * PANEL_ROW
                    + PANEL_PAD_BOTTOM) as u32
            }
            _ => match self.panel.as_ref().map(|p| &p.view) {
                Some(PanelView::Password { .. }) => 96,
                _ => {
                    let rows = self.panel_rows().len().max(1) as f32;
                    (PANEL_HEADER + rows * PANEL_ROW + PANEL_PAD_BOTTOM) as u32
                }
            },
        }
    }

    pub fn render_panel(&mut self, pixmap: &mut tiny_skia::Pixmap) {
        let p = self.palette;
        let k = SCALE;
        let w = pixmap.width() as f32;
        let Some(panel) = self.panel.clone() else {
            return;
        };
        let t = self.popup_t.clamp(0.0, 1.0);
        let content_h = self.panel_height() as f32 * k;
        let dy = -(1.0 - t) * content_h;

        let bw = 1.0 * k;
        let border = p.primary.with_a(0.35);
        fill_rect(pixmap, 0.0, dy, w, content_h, p.base.with_a(0.98));
        fill_rect(pixmap, 0.0, dy, w, bw, border);
        fill_rect(pixmap, 0.0, dy + content_h - bw, w, bw, border);
        fill_rect(pixmap, 0.0, dy, bw, content_h, border);
        fill_rect(pixmap, w - bw, dy, bw, content_h, border);

        let pad = PANEL_PAD * k;
        match &panel.view {
            PanelView::List if panel.kind == PanelKind::Clock => {
                self.render_clock(pixmap, &panel, dy);
            }
            PanelView::List if panel.kind == PanelKind::Weather => {
                self.render_weather(pixmap, &panel, dy);
            }
            PanelView::List if matches!(panel.kind, PanelKind::Volume | PanelKind::Mic) => {
                self.render_av(pixmap, &panel, dy);
            }
            PanelView::Calendar { .. } => {
                self.render_clock(pixmap, &panel, dy);
            }
            PanelView::List => {
                let hover = panel.hover;
                let rows = self.panel_rows();
                let is_tray_menu = matches!(panel.kind, PanelKind::TrayMenu { .. });
                let menu_enabled: Option<Vec<bool>> = is_tray_menu.then(|| {
                    self.shared
                        .sys
                        .lock()
                        .unwrap()
                        .tray_menu
                        .iter()
                        .map(|e| e.enabled)
                        .collect()
                });
                let title: String = match panel.kind {
                    PanelKind::Wifi => "Wi-Fi".to_string(),
                    PanelKind::Bt => "Bluetooth".to_string(),
                    PanelKind::Control => "Control center".to_string(),
                    PanelKind::TrayMenu { idx } => self
                        .shared
                        .sys
                        .lock()
                        .unwrap()
                        .tray
                        .iter()
                        .filter(|t| t.visible())
                        .nth(idx)
                        .map(|t| t.display_name())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                self.draw_text(pixmap, &title, SIZE, p.primary, pad, dy, PANEL_HEADER);

                if matches!(panel.kind, PanelKind::Wifi | PanelKind::Bt) {
                    let on = self.radio_state(panel.kind);
                    self.draw_switch(pixmap, w, dy, on, panel.hover_toggle);
                }

                let mut y = dy + PANEL_HEADER * k;
                let row_h = PANEL_ROW * k;
                for (i, (label, info, current)) in rows.iter().enumerate() {
                    let enabled = menu_enabled
                        .as_ref()
                        .map(|v| v.get(i).copied().unwrap_or(true))
                        .unwrap_or(true);
                    if hover == Some(i) && !is_hint(label) && enabled {
                        fill_rect(pixmap, bw, y, w - 2.0 * bw, row_h, p.primary.with_a(0.18));
                    }
                    let color = if !enabled {
                        p.text.with_a(0.35)
                    } else if *current {
                        p.primary
                    } else {
                        p.text
                    };
                    let info_w = self.text_width(info, SIZE);
                    self.draw_text_boxed(
                        pixmap,
                        label,
                        SIZE,
                        color,
                        w - pad * 2.0 - info_w - 8.0 * k,
                        pad,
                        y,
                        PANEL_ROW,
                    );
                    if !info.is_empty() {
                        self.draw_text(
                            pixmap,
                            info,
                            SIZE,
                            p.text.with_a(0.75),
                            w - pad - info_w,
                            y,
                            PANEL_ROW,
                        );
                    }
                    if self.popup_target > 0.5 && !is_hint(label) {
                        self.regions.push((
                            Hit::PanelRow(i),
                            Rect {
                                x: 0.0,
                                y: dy / k + PANEL_HEADER + i as f32 * PANEL_ROW,
                                w: w / k,
                                h: PANEL_ROW,
                            },
                        ));
                    }
                    y += row_h;
                }
            }
            PanelView::Password { ssid, input } => {
                let title = format!("Wi-Fi: {ssid}");
                if self.text_width(&title, SIZE) <= w - pad * 2.0 {
                    self.draw_text(pixmap, &title, SIZE, p.primary, pad, dy, PANEL_HEADER);
                } else {
                    self.draw_text_boxed(
                        pixmap,
                        &title,
                        SIZE,
                        p.primary,
                        w - pad * 2.0,
                        pad,
                        dy,
                        PANEL_HEADER,
                    );
                }
                let field_y = dy + (PANEL_HEADER + 6.0) * k;
                let field_h = 26.0 * k;
                fill_rect(
                    pixmap,
                    pad,
                    field_y,
                    w - pad * 2.0,
                    field_h,
                    p.secondary.with_a(0.12),
                );
                fill_rect(pixmap, pad, field_y, w - pad * 2.0, bw, border);
                fill_rect(
                    pixmap,
                    pad,
                    field_y + field_h - bw,
                    w - pad * 2.0,
                    bw,
                    border,
                );
                fill_rect(pixmap, pad, field_y, bw, field_h, border);
                fill_rect(pixmap, w - pad - bw, field_y, bw, field_h, border);
                let masked = "\u{2022}".repeat(input.chars().count());
                let shown = format!("{masked}\u{258f}");
                self.draw_text(pixmap, &shown, SIZE, p.text, pad + 6.0 * k, field_y, 26.0);
                let hint = "Enter — подключить · Esc — назад";
                self.draw_text(
                    pixmap,
                    hint,
                    SIZE,
                    p.text.with_a(0.6),
                    pad,
                    field_y + field_h + 6.0 * k,
                    22.0,
                );
            }
        }
    }

    pub fn draw_segment_bar(
        &self,
        pixmap: &mut tiny_skia::Pixmap,
        x: f32,
        y: f32,
        h: f32,
        pct: u8,
        _color: palette::Rgba,
    ) {
        let k = SCALE;
        let sw = PIXEL_SEGMENT_W * k;
        let gap = PIXEL_SEGMENT_GAP * k;
        let sh = PIXEL_SEGMENT_H * k;
        let total_h = PIXEL_SEGMENT_COUNT as f32 * sh + (PIXEL_SEGMENT_COUNT as f32 - 1.0) * gap;
        let y0 = y + (h - total_h) / 2.0;
        let filled = ((pct.min(100) as f32 / 100.0) * PIXEL_SEGMENT_COUNT as f32).round() as usize;
        for i in 0..PIXEL_SEGMENT_COUNT {
            let sy = y0 + i as f32 * (sh + gap);
            let color = if i >= PIXEL_SEGMENT_COUNT - filled {
                if pct <= 15 {
                    PIXEL_WARNING
                } else {
                    PIXEL_ACTIVE
                }
            } else {
                PIXEL_MUTED.with_a(0.38)
            };
            fill_rect(pixmap, x, sy, sw, sh, color);
        }
    }

    pub fn draw_battery(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        x: f32,
        y: f32,
        h: f32,
        pct: u8,
        color: palette::Rgba,
    ) {
        let k = SCALE;
        let bw = 17.0 * k;
        let bh = 10.0 * k;
        let y0 = y + (h - bh) / 2.0;
        if self.pixel_mode() {
            let m = 1.0 * k;
            let inner_w = bw - 2.0 * m;
            let level = (inner_w * (pct.min(100) as f32 / 100.0)).max(inner_w.min(2.0 * m));
            let ih = bh - 2.0 * m;
            fill_rect(pixmap, x, y0, bw, bh, PIXEL_MUTED);
            fill_rect(pixmap, x + m, y0 + m, inner_w, ih, PIXEL_PANEL_ALT);
            fill_rect(
                pixmap,
                x + m,
                y0 + m,
                level.min(inner_w),
                ih,
                if pct <= 15 {
                    PIXEL_WARNING
                } else {
                    PIXEL_ACTIVE
                },
            );
            fill_rect(
                pixmap,
                x + bw + 1.0 * k,
                y0 + bh * 0.3,
                2.0 * k,
                bh * 0.4,
                PIXEL_MUTED,
            );
            return;
        }
        fill_round_rect(pixmap, x, y0, bw, bh, 3.0 * k, color.with_a(0.28));
        let m = 1.6 * k;
        let inner_w = bw - 2.0 * m;
        let level = (inner_w * (pct.min(100) as f32 / 100.0)).max(inner_w.min(m * 2.0));
        let ih = bh - 2.0 * m;
        fill_round_rect(
            pixmap,
            x + m,
            y0 + m,
            level.min(inner_w),
            ih,
            (ih / 2.0).min(3.0 * k - m),
            color,
        );
        fill_round_rect(
            pixmap,
            x + bw + 1.4 * k,
            y0 + bh * 0.3,
            2.4 * k,
            bh * 0.4,
            1.2 * k,
            color.with_a(0.6),
        );
    }

    pub fn draw_switch(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        w: f32,
        dy: f32,
        on: bool,
        hover: bool,
    ) {
        let p = self.palette;
        let k = SCALE;
        let tw = PANEL_TOGGLE_W * k;
        let th = PANEL_TOGGLE_H * k;
        let tx = w - PANEL_PAD * k - tw;
        let ty = dy + (PANEL_HEADER * k - th) / 2.0;
        let track = if on {
            p.primary
        } else {
            p.secondary.with_a(if hover { 0.35 } else { 0.2 })
        };
        fill_round_rect(pixmap, tx, ty, tw, th, th / 2.0, track);
        let kr = th / 2.0 - 2.0 * k;
        let kx = if on {
            tx + tw - th / 2.0
        } else {
            tx + th / 2.0
        };
        let knob = if on {
            p.base
        } else {
            p.text.with_a(if hover { 0.85 } else { 0.55 })
        };
        fill_round_rect(
            pixmap,
            kx - kr,
            ty + th / 2.0 - kr,
            kr * 2.0,
            kr * 2.0,
            kr,
            knob,
        );
    }

    pub fn render_av(&mut self, pixmap: &mut tiny_skia::Pixmap, panel: &Panel, dy: f32) {
        let p = self.palette;
        let k = SCALE;
        let w = pixmap.width() as f32;
        let is_mic = panel.kind == PanelKind::Mic;
        let (title, pct, muted) = {
            let sys = self.shared.sys.lock().unwrap();
            if is_mic {
                ("Микрофон", sys.mic, sys.mic_muted)
            } else {
                ("Звук", sys.vol, sys.vol_muted)
            }
        };
        self.draw_text(
            pixmap,
            title,
            SIZE,
            p.primary,
            PANEL_PAD * k,
            dy,
            PANEL_HEADER,
        );
        self.draw_switch(pixmap, w, dy, !muted, panel.hover_toggle);

        let pct_y = dy + PANEL_HEADER * k;
        if muted {
            let s = "Заглушено";
            let tw = self.text_width(s, 14.0);
            self.draw_text(
                pixmap,
                s,
                14.0,
                p.text.with_a(0.45),
                (w - tw) / 2.0,
                pct_y,
                AV_PCT_H,
            );
        } else {
            let s = format!("{pct}%");
            let tw = self.text_width(&s, 20.0);
            self.draw_text(pixmap, &s, 20.0, p.primary, (w - tw) / 2.0, pct_y, AV_PCT_H);
        }

        let row_y = PANEL_HEADER + AV_PCT_H;
        let sh = 8.0 * k;
        let sx = (PANEL_PAD + 6.0) * k;
        let sw = w - 2.0 * sx;
        let sy = dy + row_y * k + (AV_SLIDER_H * k - sh) / 2.0;
        let frac = if muted { 0.0 } else { pct as f32 / 100.0 };
        fill_round_rect(pixmap, sx, sy, sw, sh, sh / 2.0, p.secondary.with_a(0.15));
        if frac > 0.01 {
            let fw = (sw * frac).max(sh);
            fill_round_rect(pixmap, sx, sy, fw, sh, sh / 2.0, p.primary);
        }
        let kr = 7.0 * k;
        let kx = sx + sw * frac;
        let knob = if muted { p.text.with_a(0.5) } else { p.text };
        fill_round_rect(
            pixmap,
            kx - kr,
            sy + sh / 2.0 - kr,
            kr * 2.0,
            kr * 2.0,
            kr,
            knob,
        );

        if self.popup_target > 0.5 {
            self.regions.push((
                Hit::PanelSlider,
                Rect {
                    x: PANEL_PAD,
                    y: row_y,
                    w: w / k - 2.0 * PANEL_PAD,
                    h: AV_SLIDER_H,
                },
            ));
        }

        let section_y = dy + (PANEL_HEADER + AV_PCT_H + AV_SLIDER_H) * k;
        let section_label = if is_mic { "Вход" } else { "Выход" };
        self.draw_text(
            pixmap,
            section_label,
            11.0,
            p.text.with_a(0.5),
            PANEL_PAD * k,
            section_y,
            AV_SECTION_H,
        );

        let rows = self.av_devices();
        let hover = panel.hover;
        let bw = 1.0 * k;
        let mut y = section_y + AV_SECTION_H * k;
        let row_h = PANEL_ROW * k;
        for (i, (label, info, current, _)) in rows.iter().enumerate() {
            if hover == Some(i) && !is_hint(label) {
                fill_rect(pixmap, bw, y, w - 2.0 * bw, row_h, p.primary.with_a(0.18));
            }
            let color = if *current { p.primary } else { p.text };
            let info_w = self.text_width(info, SIZE);
            self.draw_text_boxed(
                pixmap,
                label,
                SIZE,
                color,
                w - PANEL_PAD * 2.0 * k - info_w - 8.0 * k,
                PANEL_PAD * k,
                y,
                PANEL_ROW,
            );
            if !info.is_empty() {
                self.draw_text(
                    pixmap,
                    info,
                    SIZE,
                    p.text.with_a(0.75),
                    w - PANEL_PAD * k - info_w,
                    y,
                    PANEL_ROW,
                );
            }
            if self.popup_target > 0.5 && !is_hint(label) {
                self.regions.push((
                    Hit::PanelRow(i),
                    Rect {
                        x: 0.0,
                        y: PANEL_HEADER
                            + AV_PCT_H
                            + AV_SLIDER_H
                            + AV_SECTION_H
                            + i as f32 * PANEL_ROW,
                        w: w / k,
                        h: PANEL_ROW,
                    },
                ));
            }
            y += row_h;
        }
    }

    pub fn render_weather(&mut self, pixmap: &mut tiny_skia::Pixmap, _panel: &Panel, dy: f32) {
        let p = self.palette;
        let k = SCALE;
        let w = pixmap.width() as f32;
        let pad = PANEL_PAD * k;
        let sys = self.shared.sys.lock().unwrap().clone();

        let Some(wx) = &sys.weather_data else {
            let msg = if sys.weather_loading {
                "Обновление…"
            } else if sys.weather_error.is_some() {
                "Ошибка погоды"
            } else {
                "Нет данных"
            };
            self.draw_text_boxed(
                pixmap,
                msg,
                SIZE,
                p.text.with_a(0.7),
                w - pad * 2.0,
                pad,
                dy + 8.0 * k,
                20.0,
            );
            if let Some(error) = sys.weather_error.as_deref() {
                self.draw_text_boxed(
                    pixmap,
                    error,
                    10.0,
                    p.text.with_a(0.5),
                    w - pad * 2.0,
                    pad,
                    dy + 34.0 * k,
                    16.0,
                );
            }
            return;
        };

        self.draw_text(pixmap, &wx.icon, 26.0, p.primary, pad, dy + 2.0 * k, 32.0);
        let icon_w = self.text_width(&wx.icon, 26.0);
        let temp = format!("{}°C", wx.temp);
        let temp_x = pad + icon_w + 6.0 * k;
        self.draw_text(pixmap, &temp, 22.0, p.text, temp_x, dy + 4.0 * k, 32.0);
        let temp_w = self.text_width(&temp, 22.0);
        self.draw_text(
            pixmap,
            &wx.label,
            SIZE,
            p.text.with_a(0.7),
            temp_x + temp_w + 8.0 * k,
            dy + 12.0 * k,
            20.0,
        );

        let texts = [
            format!("ощущается {}°", wx.feels),
            format!("влажность {}%", wx.humidity),
            format!("ветер {:.1} м/с", wx.wind),
        ];
        let widths: Vec<f32> = texts.iter().map(|t| self.text_width(t, SIZE)).collect();
        let sep = self.text_width("·", SIZE);
        let total: f32 =
            widths.iter().sum::<f32>() + (sep + 8.0 * k) * (texts.len() - 1) as f32 + 12.0 * k;
        fill_round_rect(
            pixmap,
            pad,
            dy + 40.0 * k,
            total,
            22.0 * k,
            5.0 * k,
            p.secondary.with_a(0.1),
        );
        let mut x = pad + 6.0 * k;
        for (i, txt) in texts.iter().enumerate() {
            if i > 0 {
                self.draw_text(
                    pixmap,
                    "·",
                    SIZE,
                    p.text.with_a(0.35),
                    x,
                    dy + 41.0 * k,
                    22.0,
                );
                x += sep + 8.0 * k;
            }
            self.draw_text(
                pixmap,
                txt,
                SIZE,
                p.text.with_a(0.85),
                x,
                dy + 41.0 * k,
                22.0,
            );
            x += widths[i];
        }

        self.draw_text(
            pixmap,
            "По часам",
            11.0,
            p.text.with_a(0.5),
            pad,
            dy + 70.0 * k,
            14.0,
        );
        let cols = 4;
        let cell_w = (w - 2.0 * pad) / cols as f32;
        let cell_h = 26.0 * k;
        for (i, h) in wx.hourly.iter().take(8).enumerate() {
            let cx = pad + (i % cols) as f32 * cell_w;
            let cy = dy + 86.0 * k + (i / cols) as f32 * cell_h;
            let label = format!("{:02} {}", h.t, h.icon);
            let temp = format!("{}°", h.temp);
            let tw_label = self.text_width(&label, SIZE);
            let tw_temp = self.text_width(&temp, SIZE);
            let gap = 4.0 * k;
            let x0 = cx + (cell_w - tw_label - gap - tw_temp) / 2.0;
            self.draw_text(pixmap, &label, SIZE, p.text.with_a(0.65), x0, cy, 26.0);
            self.draw_text(pixmap, &temp, SIZE, p.text, x0 + tw_label + gap, cy, 26.0);
        }

        self.draw_text(
            pixmap,
            "Прогноз",
            11.0,
            p.text.with_a(0.5),
            pad,
            dy + 144.0 * k,
            14.0,
        );
        let mut ry = dy + 160.0 * k;
        for d in wx.daily.iter().take(3) {
            let day = NaiveDate::parse_from_str(&d.date, "%Y-%m-%d")
                .map(|dt| {
                    format!(
                        "{} {}",
                        WEEKDAYS_SHORT[dt.weekday().num_days_from_monday() as usize],
                        dt.format("%d.%m")
                    )
                })
                .unwrap_or_else(|_| d.date.clone());
            self.draw_text(pixmap, &day, SIZE, p.text.with_a(0.75), pad, ry, 22.0);
            self.draw_text(pixmap, &d.icon, SIZE, p.primary, pad + 78.0 * k, ry, 22.0);
            self.draw_text(
                pixmap,
                &format!("{}° / {}°", d.max, d.min),
                SIZE,
                p.text,
                pad + 104.0 * k,
                ry,
                22.0,
            );
            let lw = self.text_width(&d.label, SIZE);
            self.draw_text(
                pixmap,
                &d.label,
                SIZE,
                p.text.with_a(0.55),
                w - pad - lw,
                ry,
                22.0,
            );
            ry += 22.0 * k;
        }

        let foot = if sys.weather_loading {
            "обновление…".to_string()
        } else if wx.stale {
            format!("офлайн · кэш {}", wx.updated)
        } else {
            format!("обновлено {}", wx.updated)
        };
        self.draw_text(
            pixmap,
            &foot,
            10.0,
            p.text.with_a(0.4),
            pad,
            ry + 2.0 * k,
            12.0,
        );
    }

    // ---------- frames ----------
}

fn toggle_label(on: bool) -> String {
    if on { "вкл" } else { "выкл" }.to_string()
}

#[cfg(test)]
mod tests {
    use super::{level_label, toggle_label};

    #[test]
    fn control_labels_show_state_and_level() {
        assert_eq!(toggle_label(true), "вкл");
        assert_eq!(toggle_label(false), "выкл");
        assert_eq!(level_label(42, false), "42%");
        assert_eq!(level_label(42, true), "выкл");
    }
}
