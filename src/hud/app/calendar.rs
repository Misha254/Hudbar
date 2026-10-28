use std::sync::atomic::Ordering;

use chrono::Datelike;

use super::render::App;
use super::strings::*;
use super::text::TextRenderer;
use super::ui::*;

use crate::hud::i18n;

pub trait CalendarView {
    fn render_clock(&mut self, pixmap: &mut tiny_skia::Pixmap, panel: &Panel, dy: f32);
    fn on_clock_click(&mut self, x: f32, y: f32);
    fn clock_nav_rects(&mut self) -> (Rect, Rect, Rect);
    fn cal_month(&self) -> (i32, u32);
}

impl CalendarView for App {
    fn render_clock(&mut self, pixmap: &mut tiny_skia::Pixmap, _panel: &Panel, dy: f32) {
        let p = self.palette;
        let k = SCALE;
        let w = pixmap.width() as f32;
        let (year, month) = self.cal_month();

        let now = chrono::Local::now();
        let lang = self.settings.language;
        let date = i18n::date_long(lang, now.date_naive());
        let dw = self.text_width(&date, 11.0);
        self.draw_text(
            pixmap,
            &date,
            11.0,
            p.text.with_a(0.55),
            (w - dw) / 2.0,
            dy + CLOCK_TITLE_H * k,
            CLOCK_DATE_H,
        );

        let (prev, title, next) = self.clock_nav_rects();
        let aw = self.text_width("‹", 15.0);
        self.draw_text(
            pixmap,
            "‹",
            15.0,
            p.primary,
            prev.x * k + (prev.w * k - aw) / 2.0,
            dy,
            CLOCK_TITLE_H,
        );
        let title_text = i18n::month_title(lang, year, month);
        self.draw_text(
            pixmap,
            &title_text,
            SIZE + 1.0,
            p.primary,
            title.x * k,
            dy,
            CLOCK_TITLE_H,
        );
        let aw = self.text_width("›", 15.0);
        self.draw_text(
            pixmap,
            "›",
            15.0,
            p.primary,
            next.x * k + (next.w * k - aw) / 2.0,
            dy,
            CLOCK_TITLE_H,
        );
        if self.popup_target() > 0.5 {
            self.regions.push((Hit::CalPrev, prev));
            self.regions.push((Hit::CalToday, title));
            self.regions.push((Hit::CalNext, next));
        }

        let grid_x = ((w / k - (CAL_WEEK_W + 7.0 * CAL_COL_W)) / 2.0).max(PANEL_PAD);
        let wd_y = dy + (CLOCK_TITLE_H + CLOCK_DATE_H) * k;
        let wn_w = self.text_width(week_number(lang), 10.0);
        self.draw_text(
            pixmap,
            week_number(lang),
            10.0,
            p.text.with_a(0.45),
            grid_x * k + (CAL_WEEK_W * k - wn_w) / 2.0,
            wd_y,
            CAL_WD_H,
        );
        for i in 0..7 {
            let name = i18n::weekday_short(lang, i);
            let tw = self.text_width(name, 10.0);
            let cell_x = (grid_x + CAL_WEEK_W + i as f32 * CAL_COL_W) * k;
            self.draw_text(
                pixmap,
                name,
                10.0,
                p.text.with_a(0.6),
                cell_x + (CAL_COL_W * k - tw) / 2.0,
                wd_y,
                CAL_WD_H,
            );
        }

        let today = now.date_naive();
        let y0 = dy + (CLOCK_TITLE_H + CLOCK_DATE_H + CAL_WD_H) * k;
        for (r, (week, days)) in cal_weeks(year, month).iter().enumerate() {
            let row_y = y0 + r as f32 * CAL_ROW_H * k;
            let week_s = week.to_string();
            let wn = self.text_width(&week_s, 9.0);
            self.draw_text(
                pixmap,
                &week_s,
                9.0,
                p.text.with_a(0.45),
                grid_x * k + (CAL_WEEK_W * k - wn) / 2.0,
                row_y,
                CAL_ROW_H,
            );
            for (c, day) in days.iter().enumerate() {
                let Some(day) = day else { continue };
                let cell_x = (grid_x + CAL_WEEK_W + c as f32 * CAL_COL_W) * k;
                let cw = CAL_COL_W * k;
                let is_today =
                    today.year() == year && today.month() == month && today.day() == *day;
                let label = day.to_string();
                let tw = self.text_width(&label, SIZE);
                if is_today {
                    fill_round_rect(
                        pixmap,
                        cell_x + 1.0 * k,
                        row_y + 0.5 * k,
                        cw - 2.0 * k,
                        (CAL_ROW_H - 1.0) * k,
                        4.0 * k,
                        p.primary,
                    );
                    self.draw_text(
                        pixmap,
                        &label,
                        SIZE,
                        p.on_primary,
                        cell_x + (cw - tw) / 2.0,
                        row_y,
                        CAL_ROW_H,
                    );
                } else {
                    self.draw_text(
                        pixmap,
                        &label,
                        SIZE,
                        p.text,
                        cell_x + (cw - tw) / 2.0,
                        row_y,
                        CAL_ROW_H,
                    );
                }
            }
        }
    }

    fn on_clock_click(&mut self, x: f32, y: f32) {
        let Some(PanelView::Calendar { year, month }) = self.panel.as_ref().map(|p| p.view.clone())
        else {
            return;
        };
        let (prev, title, next) = self.clock_nav_rects();
        let (y2, m2) = if prev.contains(x, y) {
            shift_month(year, month, -1)
        } else if next.contains(x, y) {
            shift_month(year, month, 1)
        } else if title.contains(x, y) {
            let now = chrono::Local::now();
            (now.year(), now.month())
        } else {
            return;
        };
        if let Some(p) = self.panel.as_mut() {
            p.view = PanelView::Calendar {
                year: y2,
                month: m2,
            };
        }
        self.shared.dirty.store(true, Ordering::Relaxed);
    }

    fn clock_nav_rects(&mut self) -> (Rect, Rect, Rect) {
        let pin_w = 20.0;
        let gap = 8.0;
        let (year, month) = self.cal_month();
        let lang = self.settings.language;
        let title = i18n::month_title(lang, year, month);
        let title_w = self.text_width(&title, SIZE + 1.0) / SCALE;
        let panel_w = self
            .popup
            .as_ref()
            .map(|p| p.size.0 as f32)
            .unwrap_or(PANEL_W_CLOCK);
        let total = pin_w + gap + title_w + gap + pin_w;
        let x0 = ((panel_w - total) / 2.0).max(PANEL_PAD);
        let y = 0.0;
        let prev = Rect {
            x: x0,
            y,
            w: pin_w,
            h: CLOCK_TITLE_H,
        };
        let title_rect = Rect {
            x: x0 + pin_w + gap,
            y,
            w: title_w,
            h: CLOCK_TITLE_H,
        };
        let next = Rect {
            x: title_rect.x + title_w + gap,
            y,
            w: pin_w,
            h: CLOCK_TITLE_H,
        };
        (prev, title_rect, next)
    }

    fn cal_month(&self) -> (i32, u32) {
        match self.panel.as_ref().map(|p| &p.view) {
            Some(PanelView::Calendar { year, month }) => (*year, *month),
            _ => {
                let n = chrono::Local::now();
                (n.year(), n.month())
            }
        }
    }
}
