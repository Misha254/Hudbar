use super::*;

fn low_battery_style(
    palette: palette::Palette,
    chip_bg: palette::Rgba,
) -> (palette::Rgba, palette::Rgba) {
    (palette.error, chip_bg)
}

/// Группа панели вместе с её местом в `module_order`.
struct Slot {
    order: usize,
    group: Group,
}
/// Раскладывает собранные группы по порядку из настроек. Сортировка
/// устойчивая, а неизвестные ключи уходят в конец, поэтому модуль, которого
/// нет в `module_order`, не потеряется и не встанет в начало.
/// Слот шестерёнки Control Center или `None`, если кнопка выключена в
/// `settings.json`. Единственное место, где решается судьба кнопки: и
/// отрисовка, и тесты берут её отсюда.
fn control_slot(
    settings: &crate::hud::settings::Settings,
    color: palette::Rgba,
    chip_bg: palette::Rgba,
) -> Option<Slot> {
    if !settings.control_button {
        return None;
    }
    let mut control = Cell::new("⚙".to_string(), color);
    control.pad_l = 4.0;
    control.pad_r = 4.0;
    control.hit = Some(Hit::ControlChip);
    Some(Slot {
        order: usize::MAX,
        group: Group {
            cells: vec![control],
            chip: Some((4.0, 4.0, chip_bg)),
        },
    })
}

fn order_slots(mut slots: Vec<Slot>) -> Vec<Slot> {
    slots.sort_by_key(|slot| slot.order);
    slots
}

impl App {
    pub fn group_width(&mut self, g: &Group) -> f32 {
        let k = SCALE;
        let mut w = 0.0;
        for c in &g.cells {
            w += (c.pad_l + c.pad_r + c.gap_r) * k + self.text_width(&c.text, c.size);
            if c.bat.is_some() {
                w += BAT_ICON_W * k;
            }
            if c.bar.is_some() {
                w += PIXEL_SEGMENT_W * k;
            }
            if c.ram {
                w += RAM_ICON_W * k;
            }
        }
        if let Some((pl, pr, _)) = g.chip {
            w += (pl + pr) * k;
        }
        w
    }

    pub fn draw_group(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        g: &Group,
        x: f32,
        y: f32,
        height: f32,
    ) -> f32 {
        let k = SCALE;
        let p = self.bar_palette();
        let w = self.group_width(g);
        if self.pixel_mode() {
            draw_pixel_frame(
                pixmap,
                x,
                y,
                w,
                height,
                PIXEL_PANEL,
                PIXEL_BORDER,
                PIXEL_HIGHLIGHT,
            );
        } else if let Some((_, _, bg)) = g.chip {
            fill_rect(pixmap, x, y, w, height, bg);
        }
        let mut cx = x + g.chip.map(|(pl, _, _)| pl * k).unwrap_or(0.0);
        for c in &g.cells {
            let iw = if c.bat.is_some() { BAT_ICON_W * k } else { 0.0 };
            let ram_w = if c.ram { RAM_ICON_W * k } else { 0.0 };
            let text_w = self.text_width(&c.text, c.size);
            let bar_w = if c.bar.is_some() {
                PIXEL_SEGMENT_W * k
            } else {
                0.0
            };
            let cw = (c.pad_l + c.pad_r) * k + text_w + iw + ram_w + bar_w;
            let text_x = cx + iw + ram_w + c.pad_l * k;
            if let Some(bg) = c.bg {
                fill_rect(pixmap, cx, y, cw, height, bg);
            }
            if let Some(h) = c.hit {
                let t = self.hover_alpha(h);
                if t > 0.001 {
                    if self.pixel_mode() {
                        let theme = self.theme();
                        let (hx, hw, hy, hh) = if theme.full_hover
                            && matches!(
                                h,
                                Hit::VolChip | Hit::MicChip | Hit::ClockChip | Hit::WeatherChip
                            ) {
                            (cx, cw, y, height)
                        } else {
                            let side = 16.0 * k;
                            let extra = if matches!(h, Hit::WifiChip) {
                                theme.wifi_extra * k
                            } else {
                                0.0
                            };
                            (
                                text_x + text_w / 2.0 - side / 2.0,
                                side + extra,
                                y + (height - side) / 2.0,
                                side,
                            )
                        };
                        draw_pixel_outline(pixmap, hx, hy, hw, hh, p.primary.with_a(t));
                    } else {
                        fill_rect(pixmap, cx, y, cw, height, p.primary.with_a(0.22 * t));
                    }
                }
            }
            if let Some(pct) = c.bat {
                self.draw_battery(pixmap, cx, y, height, pct, c.color);
            }
            if c.ram
                && let Some(icon) = ram_icon()
            {
                let icon_x = cx + iw + (ram_w - RAM_ICON_DRAW_W * k) / 2.0;
                let icon_y = y + (height - RAM_ICON_H * k) / 2.0;
                blit_argb_tinted(
                    pixmap,
                    icon,
                    icon_x,
                    icon_y,
                    RAM_ICON_DRAW_W,
                    RAM_ICON_H,
                    c.color,
                );
            }
            let lh = self.height as f32;
            self.draw_text(pixmap, &c.text, c.size, c.color, text_x, 0.0, lh);
            if let Some(pct) = c.bar {
                self.draw_segment_bar(pixmap, text_x + text_w + 2.0 * k, y, height, pct, c.color);
            }
            if let Some(hit) = c.hit {
                self.regions.push((
                    hit,
                    Rect {
                        x: cx / k,
                        y: y / k,
                        w: cw / k,
                        h: height / k,
                    },
                ));
            }
            cx += cw + c.gap_r * k;
        }
        w
    }

    // ---------- bar render ----------

    pub fn render(&mut self, pixmap: &mut tiny_skia::Pixmap) {
        let p = self.bar_palette();
        let k = SCALE;
        let h = self.height as f32 * k;
        let width = self.width as f32 * k;
        let sys = crate::hud::lock::mutex(&self.shared.sys).clone();

        let pixel = self.pixel_mode();
        if pixel {
            pixmap.fill(PIXEL_BASE.to_tiny());
            draw_pixel_frame(
                pixmap,
                0.0,
                0.0,
                width,
                h,
                PIXEL_PANEL_ALT,
                PIXEL_BORDER,
                PIXEL_HIGHLIGHT,
            );
        } else {
            pixmap.fill(p.base.with_a(0.9).to_tiny());
        }

        let theme = self.theme();
        let chip_y = theme.chip_inset * k;
        let chip_h = h - theme.chip_inset * 2.0 * k;
        let gap = theme.chip_gap * k;
        let chip_bg = if pixel {
            PIXEL_PANEL
        } else {
            p.secondary.with_a(0.1)
        };

        let mut lx = 0.0;
        if pixel {
            lx = 2.0 * k;
            for (i, ws) in sys.workspaces.iter().enumerate() {
                let label = ws.name.clone().unwrap_or_else(|| ws.idx.to_string());
                let tw = self.text_width(&label, SIZE);
                let w = (tw + 10.0 * k).max(20.0 * k);
                let (fill, border, text, highlight) = if ws.focused {
                    (PIXEL_ACTIVE, PIXEL_ACTIVE, PIXEL_ACTIVE_TEXT, PIXEL_ACTIVE)
                } else if ws.urgent {
                    (PIXEL_PANEL, PIXEL_WARNING, PIXEL_WARNING, PIXEL_WARNING)
                } else if ws.has_windows {
                    (PIXEL_PANEL, PIXEL_BORDER, PIXEL_TEXT, PIXEL_HIGHLIGHT)
                } else {
                    (PIXEL_PANEL_ALT, PIXEL_BORDER, PIXEL_MUTED, PIXEL_BORDER)
                };
                draw_pixel_frame(pixmap, lx, chip_y, w, chip_h, fill, border, highlight);
                self.draw_text(
                    pixmap,
                    &label,
                    SIZE,
                    text,
                    lx + (w - tw) / 2.0,
                    0.0,
                    self.height as f32,
                );
                self.regions.push((
                    Hit::Workspace(i),
                    Rect {
                        x: lx / k,
                        y: chip_y / k,
                        w: w / k,
                        h: chip_h / k,
                    },
                ));
                lx += w + 2.0 * k;
            }
            lx = (lx - 2.0 * k).max(0.0);
        } else {
            let mut ws_cells: Vec<Cell> = Vec::new();
            for (i, ws) in sys.workspaces.iter().enumerate() {
                let label = ws.name.clone().unwrap_or_else(|| ws.idx.to_string());
                if ws.focused {
                    let mut c = Cell::new(label, p.on_primary);
                    c.pad_l = 6.0;
                    c.pad_r = 6.0;
                    c.bg = Some(p.primary);
                    c.hit = Some(Hit::Workspace(i));
                    ws_cells.push(c);
                } else {
                    let color = if ws.urgent {
                        p.error
                    } else if ws.has_windows {
                        p.text
                    } else {
                        p.text.with_a(0.4)
                    };
                    let mut c = Cell::new(label, color);
                    c.pad_l = 6.0;
                    c.pad_r = 6.0;
                    c.hit = Some(Hit::Workspace(i));
                    ws_cells.push(c);
                }
            }
            if !ws_cells.is_empty() {
                let g = Group {
                    cells: ws_cells,
                    chip: Some((5.0, 5.0, chip_bg)),
                };
                lx = self.draw_group(pixmap, &g, 0.0, chip_y, chip_h);
            }
        }

        let tray_items: Vec<tray::TrayItem> =
            sys.tray.iter().filter(|t| t.visible()).cloned().collect();
        if self.settings.tray && !tray_items.is_empty() {
            let icon_s = theme.tray_icon * k;
            let ispace = theme.tray_space * k;
            let tpad = theme.tray_pad * k;
            let total = tpad * 2.0
                + tray_items.len() as f32 * icon_s
                + ispace * (tray_items.len() - 1) as f32;
            let tx = lx + gap;
            if pixel {
                draw_pixel_frame(
                    pixmap,
                    tx,
                    chip_y,
                    total,
                    chip_h,
                    PIXEL_PANEL,
                    PIXEL_BORDER,
                    PIXEL_HIGHLIGHT,
                );
            } else {
                fill_rect(pixmap, tx, chip_y, total, chip_h, chip_bg);
            }
            let mut cx = tx + tpad;
            let iy = chip_y + (chip_h - icon_s) / 2.0;
            for (i, item) in tray_items.iter().enumerate() {
                let ht = self.hover_alpha(Hit::TrayItem(i));
                if ht > 0.001 {
                    let hp = 5.0 * k;
                    fill_rect(
                        pixmap,
                        cx - hp,
                        iy - hp,
                        icon_s + hp * 2.0,
                        icon_s + hp * 2.0,
                        p.primary.with_a(0.22 * ht),
                    );
                }
                if let Some(icon) = &item.icon {
                    if pixel {
                        blit_argb_nearest(pixmap, icon, cx, iy, icon_s / k);
                    } else {
                        blit_argb(pixmap, icon, cx, iy, icon_s / k);
                    }
                }
                let rw = icon_s
                    + if i + 1 == tray_items.len() {
                        0.0
                    } else {
                        ispace
                    };
                self.regions.push((
                    Hit::TrayItem(i),
                    Rect {
                        x: cx / k,
                        y: chip_y / k,
                        w: rw / k,
                        h: chip_h / k,
                    },
                ));
                cx += icon_s + ispace;
            }
        }

        let mut center: Vec<Slot> = Vec::new();
        if self.settings.weather
            && let Some(w) = &sys.weather
        {
            let weather = w.replacen(' ', self.icon_gap(), 1);
            let mut c = Cell::new(format!("{weather}\u{b0}C"), p.text);
            c.pad_l = 2.0;
            c.pad_r = 2.0;
            c.hit = Some(Hit::WeatherChip);
            center.push(Slot {
                order: self.settings.order_index("weather"),
                group: Group {
                    cells: vec![c],
                    chip: Some((2.0, 2.0, chip_bg)),
                },
            });
        }
        if self.settings.webcam {
            let (webcam_color, webcam_bg) = if sys.webcam_active {
                (p.error, p.error.with_a(0.25))
            } else {
                (p.text, chip_bg)
            };
            let mut c = Cell::new(I_WEBCAM.to_string(), webcam_color);
            c.pad_l = 2.0;
            c.pad_r = 2.0;
            center.push(Slot {
                order: self.settings.order_index("webcam"),
                group: Group {
                    cells: vec![c],
                    chip: Some((2.0, 2.0, webcam_bg)),
                },
            });
        }
        if self.settings.clock {
            let mut c = Cell::new(self.clock.clone(), p.text);
            c.pad_l = 3.0;
            c.pad_r = 3.0;
            c.hit = Some(Hit::ClockChip);
            center.push(Slot {
                order: self.settings.order_index("clock"),
                group: Group {
                    cells: vec![c],
                    chip: Some((2.0, 2.0, chip_bg)),
                },
            });
        }
        let center = order_slots(center);
        let center: Vec<Group> = center.into_iter().map(|slot| slot.group).collect();

        let mut widths: Vec<f32> = Vec::new();
        for g in &center {
            widths.push(self.group_width(g));
        }
        let center_gap = 3.0 * k;
        let total: f32 =
            widths.iter().sum::<f32>() + center_gap * (center.len().saturating_sub(1)) as f32;
        let mut x = ((width - total) / 2.0).max(0.0);
        for (g, w) in center.iter().zip(widths.iter()) {
            self.draw_group(pixmap, g, x, chip_y, chip_h);
            x += w + center_gap;
        }

        let mut right: Vec<Slot> = Vec::new();
        if self.settings.recorder && sys.recorder_on {
            let c = Cell::new(format!("Rec {I_REC}"), p.text);
            right.push(Slot {
                order: self.settings.order_index("recorder"),
                group: Group {
                    cells: vec![c],
                    chip: None,
                },
            });
        }
        if self.settings.battery
            && let Some(pct) = sys.battery_pct
        {
            let (text, color, bg, with_icon) = if pixel {
                (
                    "BAT".to_string(),
                    if pct <= 15 { p.error } else { p.text },
                    chip_bg,
                    true,
                )
            } else if sys.charging {
                (
                    format!("{I_PLUG}{}{pct}%", self.icon_gap()),
                    p.text,
                    chip_bg,
                    false,
                )
            } else if pct <= 15 {
                let (color, bg) = low_battery_style(p, chip_bg);
                (format!("{pct}%"), color, bg, true)
            } else if pct <= 30 {
                (format!("{pct}%"), p.text.with_a(0.75), chip_bg, true)
            } else {
                (format!("{pct}%"), p.text, chip_bg, true)
            };
            let mut c = Cell::new(text, color);
            c.pad_l = if pixel { 2.0 } else { 0.0 };
            c.pad_r = if pixel { 2.0 } else { 0.0 };
            if with_icon {
                c.bat = Some(pct);
            }
            let mut cells = vec![c];
            if let Some(t) = &sys.bat_time {
                let mut dot = Cell::new("·".to_string(), p.text.with_a(0.3));
                dot.pad_l = 3.0;
                dot.pad_r = 3.0;
                cells.push(dot);
                let mut tc = Cell::new(t.clone(), p.text.with_a(0.9));
                tc.pad_l = 0.0;
                tc.pad_r = 0.0;
                cells.push(tc);
            }
            right.push(Slot {
                order: self.settings.order_index("battery"),
                group: Group {
                    cells,
                    chip: Some((8.0, 8.0, bg)),
                },
            });
        }

        if self.settings.system {
            let cells = if pixel {
                let mut cpu = Cell::new("CPU".to_string(), p.text);
                cpu.pad_l = 2.0;
                cpu.pad_r = 2.0;
                cpu.bar = Some(sys.cpu_pct);
                let mut ram = Cell::new("RAM".to_string(), p.text);
                ram.pad_l = 2.0;
                ram.pad_r = 2.0;
                ram.bar = Some(sys.mem_pct);
                let mut cells = vec![cpu];
                if let Some(t) = sys.temp_c {
                    let mut temp = Cell::new("TEMP".to_string(), p.text);
                    temp.pad_l = 2.0;
                    temp.pad_r = 2.0;
                    temp.bar = Some(t.clamp(0, 100) as u8);
                    cells.push(temp);
                }
                cells.push(ram);
                cells
            } else {
                let mut cells: Vec<Cell> = vec![Cell::new(
                    format!("{I_CPU}{}{}%", self.icon_gap(), sys.cpu_pct),
                    p.text,
                )];
                if let Some(t) = sys.temp_c {
                    cells.push(Cell::new(
                        format!("{I_TEMP}{}{t}\u{b0}C", self.icon_gap()),
                        p.text,
                    ));
                }
                let mut ram = Cell::new(format!("{:.2}GiB", sys.mem_used_gib), p.text);
                ram.pad_l = 2.0;
                ram.pad_r = 0.0;
                ram.ram = true;
                cells.push(ram);
                cells
            };
            right.push(Slot {
                order: self.settings.order_index("system"),
                group: Group {
                    cells,
                    chip: Some((5.0, 5.0, chip_bg)),
                },
            });
        }

        if self.settings.audio {
            let vol = if sys.vol_muted {
                Cell::new(I_VOL_MUTED.to_string(), p.text.with_a(0.4))
            } else {
                Cell::new(
                    format!("{I_VOL}{}{}{}%", self.icon_gap(), theme.vol_pad, sys.vol),
                    p.text,
                )
            };
            let mut vol = vol;
            vol.pad_l = 4.0;
            vol.pad_r = 4.0;
            vol.hit = Some(Hit::VolChip);
            let mic = if sys.mic_muted {
                let mut c = Cell::new(I_MIC_MUTED.to_string(), p.text.with_a(0.4));
                c.pad_l = 4.0;
                c.pad_r = 4.0;
                c.hit = Some(Hit::MicChip);
                c
            } else {
                let mut c = Cell::new(format!("{I_MIC}{}{}%", self.icon_gap(), sys.mic), p.text);
                c.pad_l = 4.0;
                c.pad_r = 4.0;
                c.hit = Some(Hit::MicChip);
                c
            };
            right.push(Slot {
                order: self.settings.order_index("audio"),
                group: Group {
                    cells: vec![vol, mic],
                    chip: Some((5.0, 5.0, chip_bg)),
                },
            });
        }

        if self.settings.network {
            let net_slot = 16.0;
            let mut wifi = Cell::new(
                I_WIFI.to_string(),
                if sys.wifi_enabled {
                    p.text
                } else {
                    p.text.with_a(0.4)
                },
            );
            wifi.hit = Some(Hit::WifiChip);
            let wifi_pad = ((net_slot - self.text_width(&wifi.text, wifi.size) / k) / 2.0).max(2.0);
            wifi.pad_l = wifi_pad;
            wifi.pad_r = wifi_pad;
            wifi.gap_r = 3.0;
            let mut bt = Cell::new(
                I_BT.to_string(),
                if sys.bt_on {
                    p.text
                } else {
                    p.text.with_a(0.4)
                },
            );
            bt.size = SIZE_BT;
            bt.hit = Some(Hit::BtChip);
            let bt_pad = ((net_slot - self.text_width(&bt.text, bt.size) / k) / 2.0).max(2.0);
            bt.pad_l = bt_pad;
            bt.pad_r = bt_pad;
            right.push(Slot {
                order: self.settings.order_index("network"),
                group: Group {
                    cells: vec![wifi, bt],
                    chip: Some((5.0, 5.0, chip_bg)),
                },
            });
        }

        if self.settings.dnd && sys.dnd {
            let mut c = Cell::new("DND".to_string(), p.base);
            c.pad_l = 0.0;
            c.pad_r = 0.0;
            right.push(Slot {
                order: self.settings.order_index("dnd"),
                group: Group {
                    cells: vec![c],
                    chip: Some((3.0, 3.0, p.error)),
                },
            });
        }

        // Шестерёнка всегда последняя и появляется, только если её не выключили.
        if let Some(slot) = control_slot(&self.settings, p.primary, chip_bg) {
            right.push(slot);
        }

        let right = order_slots(right);
        let right: Vec<Group> = right.into_iter().map(|slot| slot.group).collect();

        let mut rwidths: Vec<f32> = Vec::new();
        for g in &right {
            rwidths.push(self.group_width(g));
        }
        let rtotal: f32 =
            rwidths.iter().sum::<f32>() + gap * (right.len().saturating_sub(1)) as f32;
        let mut x = width - 2.0 * k - rtotal;
        for (g, w) in right.iter().zip(rwidths.iter()) {
            self.draw_group(pixmap, g, x, chip_y, chip_h);
            x += w + gap;
        }
    }

    // ---------- panel ----------
}

#[cfg(test)]
mod tests {
    use super::{Cell, Group, Slot, control_slot, low_battery_style, order_slots};
    use crate::hud::palette::Palette;
    use crate::hud::settings::{self, Settings};

    fn slot(key: &str, settings: &Settings, label: &str) -> Slot {
        Slot {
            order: settings.order_index(key),
            group: Group {
                cells: vec![Cell::new(label.to_string(), settings_default_color())],
                chip: None,
            },
        }
    }

    fn settings_default_color() -> crate::hud::palette::Rgba {
        Palette::default().text
    }

    #[test]
    fn low_battery_keeps_the_normal_chip_background() {
        let palette = Palette::default();
        let chip_bg = palette.secondary.with_a(0.1);

        let (text, background) = low_battery_style(palette, chip_bg);

        assert_eq!(text, palette.error);
        assert_eq!(background, chip_bg);
        assert_ne!(background, palette.error);
    }

    #[test]
    fn right_zone_follows_module_order() {
        let settings = Settings {
            // Порядок, обратный заводскому: код собирает группы сверху вниз,
            // а панель обязана показать их по настройкам.
            module_order: settings::normalize_module_order(&[
                "dnd".to_string(),
                "network".to_string(),
                "audio".to_string(),
                "system".to_string(),
                "battery".to_string(),
                "recorder".to_string(),
                "clock".to_string(),
                "webcam".to_string(),
                "weather".to_string(),
                "tray".to_string(),
            ]),
            ..Default::default()
        };

        let slots = order_slots(vec![
            slot("recorder", &settings, "Rec"),
            slot("battery", &settings, "Bat"),
            slot("dnd", &settings, "DND"),
        ]);
        let labels: Vec<&str> = slots
            .iter()
            .map(|slot| slot.group.cells[0].text.as_str())
            .collect();

        assert_eq!(labels, vec!["DND", "Bat", "Rec"]);
    }

    #[test]
    fn module_missing_from_order_goes_last_without_being_dropped() {
        let settings = Settings {
            module_order: vec!["clock".to_string(), "weather".to_string()],
            ..Default::default()
        };

        let known = settings.order_index("clock");
        let missing = settings.order_index("network");

        assert!(missing > known, "неизвестный ключ должен уйти в конец");
    }

    #[test]
    fn duplicate_keys_in_order_do_not_duplicate_a_module() {
        let settings = Settings::default();

        let first = settings.order_index("audio");
        let second = settings.order_index("audio");

        assert_eq!(
            first, second,
            "у ключа один индекс, а не по одному на вхождение"
        );
        assert_eq!(
            settings
                .module_order
                .iter()
                .filter(|key| *key == "audio")
                .count(),
            1
        );
    }

    #[test]
    fn control_button_is_not_a_module_and_stays_last() {
        // Порядок, при котором dnd(0) идёт раньше recorder(4). Если бы сортировки
        // не было, порядок вставки дал бы другой результат.
        let settings = Settings {
            module_order: settings::normalize_module_order(&[
                "dnd".to_string(),
                "tray".to_string(),
                "weather".to_string(),
                "webcam".to_string(),
                "clock".to_string(),
                "battery".to_string(),
                "system".to_string(),
                "audio".to_string(),
                "network".to_string(),
                "recorder".to_string(),
            ]),
            ..Default::default()
        };

        let color = settings_default_color();
        let mut slots = vec![
            slot("recorder", &settings, "Rec"),
            slot("dnd", &settings, "DND"),
        ];
        slots.extend(control_slot(&settings, color, color));
        let slots = order_slots(slots);
        let labels: Vec<&str> = slots
            .iter()
            .map(|slot| slot.group.cells[0].text.as_str())
            .collect();

        assert_eq!(labels, vec!["DND", "Rec", "⚙"]);
    }

    /// Выключенная кнопка Control Center не должна попадать в слоты панели:
    /// ни рисоваться, ни участвовать в hit-test.
    #[test]
    fn control_button_is_absent_when_switched_off() {
        let settings = Settings {
            control_button: false,
            ..Default::default()
        };

        let color = settings_default_color();
        let mut slots = Vec::new();
        slots.extend(control_slot(&settings, color, color));
        let slots = order_slots(slots);

        assert!(
            slots.is_empty(),
            "при control_button=false кнопки не должно быть среди слотов: {:?}",
            slots.len()
        );
        assert!(
            !slots
                .iter()
                .any(|slot| slot.group.cells.iter().any(|cell| cell.text == "⚙")),
            "шестерёнка не должна оставаться в слотах"
        );
    }

    #[test]
    fn control_button_defaults_to_visible() {
        assert!(
            Settings::default().control_button,
            "кнопка Control Center включена по умолчанию"
        );
        let without_key = crate::hud::settings::parse("{}");
        assert!(
            without_key.control_button,
            "отсутствующий ключ не должен выключать кнопку"
        );
    }

    /// Сортировка обязана быть устойчивой к порядку вставки: тест специально
    /// переставляет группы дважды и ждёт один и тот же результат.
    #[test]
    fn sorting_is_independent_of_insertion_order() {
        let settings = Settings {
            module_order: settings::normalize_module_order(&[
                "dnd".to_string(),
                "audio".to_string(),
                "battery".to_string(),
                "network".to_string(),
                "system".to_string(),
                "recorder".to_string(),
                "clock".to_string(),
                "webcam".to_string(),
                "weather".to_string(),
                "tray".to_string(),
            ]),
            ..Default::default()
        };

        let forwards = order_slots(vec![
            slot("recorder", &settings, "Rec"),
            slot("battery", &settings, "Bat"),
            slot("dnd", &settings, "DND"),
        ]);
        let backwards = order_slots(vec![
            slot("dnd", &settings, "DND"),
            slot("battery", &settings, "Bat"),
            slot("recorder", &settings, "Rec"),
        ]);

        fn labels(slots: &[Slot]) -> Vec<String> {
            slots
                .iter()
                .map(|slot| slot.group.cells[0].text.clone())
                .collect()
        }

        assert_eq!(labels(&forwards), vec!["DND", "Bat", "Rec"]);
        assert_eq!(labels(&forwards), labels(&backwards));
    }
}
