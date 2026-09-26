use chrono::Datelike;
pub const FONT: &str = "Minecraft Rus";
pub const SIZE: f32 = 12.0;
pub const SIZE_BT: f32 = 13.0;
pub const SCALE: f32 = 2.0;

pub const PIXEL_BASE: crate::hud::palette::Rgba = crate::hud::palette::Rgba(0x0b, 0x10, 0x20, 255);
pub const PIXEL_PANEL: crate::hud::palette::Rgba = crate::hud::palette::Rgba(0x11, 0x1b, 0x31, 255);
pub const PIXEL_PANEL_ALT: crate::hud::palette::Rgba =
    crate::hud::palette::Rgba(0x0e, 0x17, 0x2b, 255);
pub const PIXEL_BORDER: crate::hud::palette::Rgba =
    crate::hud::palette::Rgba(0x31, 0x5b, 0x9b, 255);
pub const PIXEL_HIGHLIGHT: crate::hud::palette::Rgba =
    crate::hud::palette::Rgba(0x79, 0xa7, 0xff, 255);
pub const PIXEL_TEXT: crate::hud::palette::Rgba = crate::hud::palette::Rgba(0xe8, 0xea, 0xff, 255);
pub const PIXEL_MUTED: crate::hud::palette::Rgba = crate::hud::palette::Rgba(0x8d, 0x9a, 0xbd, 255);
pub const PIXEL_ACTIVE: crate::hud::palette::Rgba =
    crate::hud::palette::Rgba(0x20, 0xe2, 0x7a, 255);
pub const PIXEL_ACTIVE_TEXT: crate::hud::palette::Rgba =
    crate::hud::palette::Rgba(0x07, 0x15, 0x0e, 255);
pub const PIXEL_WARNING: crate::hud::palette::Rgba =
    crate::hud::palette::Rgba(0xff, 0xc8, 0x57, 255);
pub struct Theme {
    pub chip_inset: f32,
    pub chip_gap: f32,
    pub tray_icon: f32,
    pub tray_space: f32,
    pub tray_pad: f32,
    pub vol_pad: &'static str,
    pub icon_gap: &'static str,
    pub full_hover: bool,
    pub wifi_extra: f32,
}

impl Theme {
    pub fn pixel() -> Self {
        Self {
            chip_inset: 2.0,
            chip_gap: 2.0,
            tray_icon: 14.0,
            tray_space: 6.0,
            tray_pad: 3.0,
            vol_pad: "  ",
            icon_gap: "   ",
            full_hover: true,
            wifi_extra: 5.0,
        }
    }

    pub fn normal() -> Self {
        Self {
            chip_inset: 3.0,
            chip_gap: 5.0,
            tray_icon: 17.0,
            tray_space: 10.0,
            tray_pad: 5.0,
            vol_pad: "",
            icon_gap: " ",
            full_hover: false,
            wifi_extra: 0.0,
        }
    }
}

pub const PIXEL_SEGMENT_COUNT: usize = 5;
pub const PIXEL_SEGMENT_W: f32 = 3.0;
pub const PIXEL_SEGMENT_GAP: f32 = 1.0;
pub const PIXEL_SEGMENT_H: f32 = 2.0;

pub fn configured_font() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    std::fs::read_to_string(format!("{home}/.config/hudbar/font"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| FONT.to_string())
}

pub const I_CPU: &str = "\u{f4bc}";
pub const I_TEMP: &str = "\u{f2c9}";
pub const I_VOL: &str = "\u{f028}";
pub const I_VOL_MUTED: &str = "\u{f026}";
pub const I_MIC: &str = "\u{f130}";
pub const I_MIC_MUTED: &str = "\u{f131}";
pub const I_WIFI: &str = "\u{f1eb}";
pub const I_BT: &str = "\u{f294}";
pub const I_PLUG: &str = "\u{f1e6}";
pub const I_REC: &str = "\u{f044a}";
pub const I_WEBCAM: &str = "\u{f09de}";
pub const I_LOCK: &str = "\u{f023}";
pub const I_CHECK: &str = "\u{f00c}";

pub const PANEL_W: f32 = 340.0;
pub const PANEL_W_WIDE: f32 = 400.0;
pub const PANEL_W_CLOCK: f32 = 236.0;
pub const PANEL_W_AV: f32 = 260.0;
pub const PANEL_HEADER: f32 = 30.0;
pub const PANEL_ROW: f32 = 28.0;
pub const PANEL_PAD_BOTTOM: f32 = 6.0;
pub const PANEL_PAD: f32 = 10.0;
pub const PANEL_TOGGLE_W: f32 = 36.0;
pub const PANEL_TOGGLE_H: f32 = 18.0;

pub const CLOCK_TITLE_H: f32 = 26.0;
pub const CLOCK_DATE_H: f32 = 18.0;
pub const AV_PCT_H: f32 = 30.0;
pub const AV_SLIDER_H: f32 = 26.0;
pub const AV_SECTION_H: f32 = 18.0;
pub const BAT_ICON_W: f32 = 25.0;
pub const CAL_WD_H: f32 = 18.0;
pub const CAL_ROW_H: f32 = 22.0;
pub const CAL_COL_W: f32 = 26.0;
pub const CAL_WEEK_W: f32 = 22.0;
pub const WX_H: u32 = 244;

pub const MONTHS_NOM: [&str; 12] = [
    "Январь",
    "Февраль",
    "Март",
    "Апрель",
    "Май",
    "Июнь",
    "Июль",
    "Август",
    "Сентябрь",
    "Октябрь",
    "Ноябрь",
    "Декабрь",
];
pub const MONTHS_GEN: [&str; 12] = [
    "января",
    "февраля",
    "марта",
    "апреля",
    "мая",
    "июня",
    "июля",
    "августа",
    "сентября",
    "октября",
    "ноября",
    "декабря",
];
pub const WEEKDAYS_FULL: [&str; 7] = [
    "Понедельник",
    "Вторник",
    "Среда",
    "Четверг",
    "Пятница",
    "Суббота",
    "Воскресенье",
];
pub const WEEKDAYS_SHORT: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];

pub fn is_hint(label: &str) -> bool {
    label.starts_with("Поиск")
        || label.starts_with("Нет")
        || label.starts_with("Выключено")
        || label.starts_with("Ошибка")
}

pub fn cal_next_month(year: i32, month: u32) -> Option<chrono::NaiveDate> {
    if month == 12 {
        chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
}

pub fn cal_rows(year: i32, month: u32) -> u32 {
    let Some(first) = chrono::NaiveDate::from_ymd_opt(year, month, 1) else {
        return 6;
    };
    let offset = first.weekday().num_days_from_monday();
    let days = cal_next_month(year, month)
        .map(|n| (n - first).num_days())
        .unwrap_or(30) as u32;
    (offset + days).div_ceil(7)
}

pub fn cal_weeks(year: i32, month: u32) -> Vec<(u32, Vec<Option<u32>>)> {
    let Some(first) = chrono::NaiveDate::from_ymd_opt(year, month, 1) else {
        return Vec::new();
    };
    let offset = first.weekday().num_days_from_monday() as i64;
    let Some(next) = cal_next_month(year, month) else {
        return Vec::new();
    };
    let days = (next - first).num_days();
    let start = first - chrono::Days::new(offset as u64);
    let rows = (offset + days + 6) / 7;
    let mut out = Vec::new();
    for r in 0..rows {
        let monday = start + chrono::Days::new((r * 7) as u64);
        let mut cells = Vec::new();
        for c in 0..7 {
            let d = monday + chrono::Days::new(c);
            cells.push((d.month() == month).then_some(d.day()));
        }
        out.push((monday.iso_week().week(), cells));
    }
    out
}

pub fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let mut y = year;
    let mut m = month as i32 + delta;
    while m < 1 {
        m += 12;
        y -= 1;
    }
    while m > 12 {
        m -= 12;
        y += 1;
    }
    (y, m as u32)
}

#[cfg(test)]
mod tests {
    use super::{cal_rows, cal_weeks, shift_month};

    #[test]
    fn handles_year_boundaries_when_shifting_months() {
        assert_eq!(shift_month(2026, 1, -1), (2025, 12));
        assert_eq!(shift_month(2026, 12, 1), (2027, 1));
    }

    #[test]
    fn calculates_leap_year_calendar_shape() {
        assert_eq!(cal_rows(2024, 2), 5);
        assert_eq!(cal_weeks(2024, 2).len(), 5);
    }

    #[test]
    fn invalid_month_returns_safe_calendar_defaults() {
        assert_eq!(cal_rows(2026, 13), 6);
        assert!(cal_weeks(2026, 13).is_empty());
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    WifiChip,
    BtChip,
    ClockChip,
    WeatherChip,
    VolChip,
    MicChip,
    Workspace(usize),
    PanelRow(usize),
    PanelSlider,
    TrayItem(usize),
    CalPrev,
    CalNext,
    CalToday,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PanelKind {
    Wifi,
    Bt,
    Clock,
    Weather,
    Volume,
    Mic,
    TrayMenu { idx: usize },
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

#[derive(Clone, Debug)]
pub enum PanelView {
    List,
    Password { ssid: String, input: String },
    Calendar { year: i32, month: u32 },
}

#[derive(Clone, Debug)]
pub struct Panel {
    pub kind: PanelKind,
    pub hover: Option<usize>,
    pub hover_toggle: bool,
    pub view: PanelView,
}

pub struct Popup {
    pub layer: smithay_client_toolkit::shell::wlr_layer::LayerSurface,
    pub configured: bool,
    pub size: (u32, u32),
}

pub struct Grab {
    pub layer: smithay_client_toolkit::shell::wlr_layer::LayerSurface,
    pub configured: bool,
    pub size: (u32, u32),
    pub region: Option<smithay_client_toolkit::compositor::Region>,
}

pub struct Tooltip {
    pub layer: smithay_client_toolkit::shell::wlr_layer::LayerSurface,
    pub configured: bool,
    pub size: (u32, u32),
    pub text: String,
}

pub const TOOLTIP_H: f32 = 22.0;
pub const TOOLTIP_PAD_X: f32 = 8.0;

pub struct Cell {
    pub text: String,
    pub size: f32,
    pub color: crate::hud::palette::Rgba,
    pub pad_l: f32,
    pub pad_r: f32,
    pub gap_r: f32,
    pub bg: Option<crate::hud::palette::Rgba>,
    pub hit: Option<Hit>,
    pub bat: Option<u8>,
    pub bar: Option<u8>,
}

impl Cell {
    pub fn new(text: String, color: crate::hud::palette::Rgba) -> Self {
        Cell {
            text,
            size: SIZE,
            color,
            pad_l: 3.0,
            pad_r: 3.0,
            gap_r: 0.0,
            bg: None,
            hit: None,
            bat: None,
            bar: None,
        }
    }
}

pub struct Group {
    pub cells: Vec<Cell>,
    pub chip: Option<(f32, f32, crate::hud::palette::Rgba)>,
}

pub fn fill_rect(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: crate::hud::palette::Rgba,
) {
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Shader::SolidColor(color.to_tiny()),
        anti_alias: false,
        ..Default::default()
    };
    if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
        pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
    }
}

pub fn fill_pixel_chamfer(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: crate::hud::palette::Rgba,
    cut: f32,
) {
    let cut = cut.min(w / 2.0).min(h / 2.0).max(0.0);
    fill_rect(pixmap, x + cut, y, w - cut * 2.0, h, color);
    fill_rect(pixmap, x, y + cut, cut, h - cut * 2.0, color);
    fill_rect(pixmap, x + w - cut, y + cut, cut, h - cut * 2.0, color);
    fill_rect(pixmap, x + cut, y + h - cut, w - cut * 2.0, cut, color);
}

#[allow(clippy::too_many_arguments)]
pub fn draw_pixel_frame(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fill: crate::hud::palette::Rgba,
    border: crate::hud::palette::Rgba,
    highlight: crate::hud::palette::Rgba,
) {
    let k = SCALE;
    let cut = (2.0 * k).min(w / 2.0).min(h / 2.0);
    let inset = k;
    fill_pixel_chamfer(pixmap, x, y, w, h, border, cut);
    fill_pixel_chamfer(
        pixmap,
        x + inset,
        y + inset,
        w - inset * 2.0,
        h - inset * 2.0,
        fill,
        (cut - inset).max(0.0),
    );
    let inner_x = x + cut + inset;
    let inner_y = y + inset;
    let inner_w = (w - (cut + inset) * 2.0).max(0.0);
    let inner_h = (h - (cut + inset) * 2.0).max(0.0);
    fill_rect(pixmap, inner_x, inner_y, inner_w, inset, highlight);
    fill_rect(
        pixmap,
        inner_x,
        inner_y,
        inset,
        inner_h,
        highlight.with_a(0.42),
    );
}

pub fn draw_pixel_outline(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: crate::hud::palette::Rgba,
) {
    let k = SCALE;
    let cut = k.min(w / 2.0).min(h / 2.0);
    let b = k;
    fill_rect(pixmap, x + cut, y, w - cut * 2.0, b, color);
    fill_rect(pixmap, x + cut, y + h - b, w - cut * 2.0, b, color);
    fill_rect(pixmap, x, y + cut, b, h - cut * 2.0, color);
    fill_rect(pixmap, x + w - b, y + cut, b, h - cut * 2.0, color);
    let steps = (cut / b).round().max(1.0) as i32;
    for i in 0..steps {
        let s = i as f32 * b;
        fill_rect(pixmap, x + cut - b - s, y + s, b, b, color);
        fill_rect(pixmap, x + w - cut + s, y + s, b, b, color);
        fill_rect(pixmap, x + cut - b - s, y + h - b - s, b, b, color);
        fill_rect(pixmap, x + w - cut + s, y + h - b - s, b, b, color);
    }
}

pub fn fill_round_rect(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    color: crate::hud::palette::Rgba,
) {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    if let Some(path) = pb.finish() {
        let paint = tiny_skia::Paint {
            shader: tiny_skia::Shader::SolidColor(color.to_tiny()),
            anti_alias: true,
            ..Default::default()
        };
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }
}

pub fn blit_argb_nearest(
    pixmap: &mut tiny_skia::Pixmap,
    icon: &crate::hud::tray::TrayIcon,
    x: f32,
    y: f32,
    size: f32,
) {
    let (sw, sh) = (icon.w as usize, icon.h as usize);
    if sw == 0 || sh == 0 || icon.argb.len() < sw * sh * 4 {
        return;
    }
    let dst_s = (size * SCALE) as usize;
    if dst_s == 0 {
        return;
    }
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let x0 = x as i32;
    let y0 = y as i32;
    let pixels = pixmap.pixels_mut();
    for dy in 0..dst_s as i32 {
        let sy = ((dy as usize * sh) / dst_s).min(sh - 1);
        for dx in 0..dst_s as i32 {
            let sx = ((dx as usize * sw) / dst_s).min(sw - 1);
            let o = (sy * sw + sx) * 4;
            let alpha = icon.argb[o + 3];
            if alpha < 128 {
                continue;
            }
            let xx = x0 + dx;
            let yy = y0 + dy;
            if xx < 0 || yy < 0 || xx >= pw || yy >= ph {
                continue;
            }
            let inv = alpha as f32 / 255.0;
            let dst = &mut pixels[(yy * pw + xx) as usize];
            let nr = (icon.argb[o + 2] as f32 * inv + dst.red() as f32 * (1.0 - inv)) as u8;
            let ng = (icon.argb[o + 1] as f32 * inv + dst.green() as f32 * (1.0 - inv)) as u8;
            let nb = (icon.argb[o] as f32 * inv + dst.blue() as f32 * (1.0 - inv)) as u8;
            let na = (alpha as f32 + dst.alpha() as f32 * (1.0 - inv)) as u8;
            if let Some(px) = tiny_skia::PremultipliedColorU8::from_rgba(nr, ng, nb, na) {
                *dst = px;
            }
        }
    }
}

pub fn blit_argb(
    pixmap: &mut tiny_skia::Pixmap,
    icon: &crate::hud::tray::TrayIcon,
    x: f32,
    y: f32,
    size: f32,
) {
    let (sw, sh) = (icon.w as usize, icon.h as usize);
    if sw == 0 || sh == 0 || icon.argb.len() < sw * sh * 4 {
        return;
    }
    let dst_s = (size * SCALE) as usize;
    if dst_s == 0 {
        return;
    }
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let pixels = pixmap.pixels_mut();
    let mut max_a = 0u8;
    for px in icon.argb.as_chunks::<4>().0 {
        max_a = max_a.max(px[3]);
    }
    let boost = if max_a > 0 { 255.0 / max_a as f32 } else { 1.0 };
    let x0 = x as i32;
    let y0 = y as i32;
    for dy in 0..dst_s as i32 {
        let fy = (dy as f32 + 0.5) * sh as f32 / dst_s as f32 - 0.5;
        let y1 = fy.floor() as i32;
        let ty = (fy - y1 as f32).clamp(0.0, 1.0);
        for dx in 0..dst_s as i32 {
            let fx = (dx as f32 + 0.5) * sw as f32 / dst_s as f32 - 0.5;
            let x1 = fx.floor() as i32;
            let tx = (fx - x1 as f32).clamp(0.0, 1.0);
            let mut acc = [0f32; 4];
            for oy in 0..2 {
                for ox in 0..2 {
                    let sx = (x1 + ox).clamp(0, sw as i32 - 1) as usize;
                    let sy = (y1 + oy).clamp(0, sh as i32 - 1) as usize;
                    let o = (sy * sw + sx) * 4;
                    let wgt = (if ox == 0 { 1.0 - tx } else { tx })
                        * (if oy == 0 { 1.0 - ty } else { ty });
                    acc[0] += icon.argb[o + 2] as f32 * wgt;
                    acc[1] += icon.argb[o + 1] as f32 * wgt;
                    acc[2] += icon.argb[o] as f32 * wgt;
                    acc[3] += icon.argb[o + 3] as f32 * wgt;
                }
            }
            let sa = ((acc[3] * boost) / 255.0).clamp(0.0, 1.0);
            if sa <= 0.004 {
                continue;
            }
            let xx = x0 + dx;
            let yy = y0 + dy;
            if xx < 0 || yy < 0 || xx >= pw || yy >= ph {
                continue;
            }
            let dst = &mut pixels[(yy * pw + xx) as usize];
            let inv = 1.0 - sa;
            // source is straight alpha → premultiply here
            let nr = (acc[0] * sa + dst.red() as f32 * inv).min(255.0) as u8;
            let ng = (acc[1] * sa + dst.green() as f32 * inv).min(255.0) as u8;
            let nb = (acc[2] * sa + dst.blue() as f32 * inv).min(255.0) as u8;
            let na = (acc[3] + dst.alpha() as f32 * inv).min(255.0) as u8;
            if let Some(px) = tiny_skia::PremultipliedColorU8::from_rgba(nr, ng, nb, na) {
                *dst = px;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn blend(
    pixmap: &mut tiny_skia::Pixmap,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: crate::hud::palette::Rgba,
    mask: &[u8],
    hard: bool,
) {
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let pixels = pixmap.pixels_mut();
    let (cr, cg, cb, ca) = (
        color.0 as u32,
        color.1 as u32,
        color.2 as u32,
        color.3 as u32,
    );

    for row in 0..h as i32 {
        let yy = y + row;
        if yy < 0 || yy >= ph {
            continue;
        }
        for col in 0..w as i32 {
            let xx = x + col;
            if xx < 0 || xx >= pw {
                continue;
            }
            let raw_m = mask[(row * w as i32 + col) as usize] as u32;
            let m = if hard && raw_m < 128 {
                0
            } else if hard {
                255
            } else {
                raw_m
            };
            if m == 0 {
                continue;
            }
            let sa = ca * m / 255;
            if sa == 0 {
                continue;
            }
            let sr = cr * sa / 255;
            let sg = cg * sa / 255;
            let sb = cb * sa / 255;
            let dst = &mut pixels[(yy * pw + xx) as usize];
            let inv = 255 - sa;
            let nr = (sr + dst.red() as u32 * inv / 255).min(255) as u8;
            let ng = (sg + dst.green() as u32 * inv / 255).min(255) as u8;
            let nb = (sb + dst.blue() as u32 * inv / 255).min(255) as u8;
            let na = (sa + dst.alpha() as u32 * inv / 255).min(255) as u8;
            if let Some(px) = tiny_skia::PremultipliedColorU8::from_rgba(nr, ng, nb, na) {
                *dst = px;
            }
        }
    }
}
