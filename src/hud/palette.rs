use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub fn with_a(self, a: f32) -> Self {
        Rgba(
            self.0,
            self.1,
            self.2,
            (self.3 as f32 * a).clamp(0.0, 255.0) as u8,
        )
    }

    pub fn to_tiny(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.0, self.1, self.2, self.3)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub base: Rgba,
    pub text: Rgba,
    pub primary: Rgba,
    pub on_primary: Rgba,
    pub secondary: Rgba,
    pub error: Rgba,
}

impl Default for Palette {
    fn default() -> Self {
        Palette {
            base: Rgba(0x12, 0x13, 0x18, 255),
            text: Rgba(0xe3, 0xe2, 0xe9, 255),
            primary: Rgba(0xb5, 0xc4, 0xff, 255),
            on_primary: Rgba(0x1a, 0x2c, 0x5f, 255),
            secondary: Rgba(0xc1, 0xc5, 0xdd, 255),
            error: Rgba(0xff, 0xb4, 0xab, 255),
        }
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

pub fn load() -> Palette {
    let Ok(content) = std::fs::read_to_string(home().join(".config/hudbar/colors.css")) else {
        return Palette::default();
    };

    parse(&content)
}

fn parse(content: &str) -> Palette {
    let mut p = Palette::default();
    for line in content.lines() {
        let Some(rest) = line.trim().strip_prefix("@define-color") else {
            continue;
        };
        let mut it = rest.split_whitespace();
        let (Some(name), Some(hex)) = (it.next(), it.next()) else {
            continue;
        };
        let hex = hex.trim_end_matches(';').trim_start_matches('#');
        // Длина считается в байтах, а срез ниже — тоже по байтам. Строка из
        // трёх двухбайтовых букв («ёёё») даёт 6 байт и прошла бы проверку
        // длины, после чего `hex[0..2]` разрезал бы UTF-8 между символами.
        // Поэтому ASCII проверяется явно, до среза.
        if hex.len() != 6 || !hex.is_ascii() {
            continue;
        }
        let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&hex[0..2], 16),
            u8::from_str_radix(&hex[2..4], 16),
            u8::from_str_radix(&hex[4..6], 16),
        ) else {
            continue;
        };
        let c = Rgba(r, g, b, 255);
        match name {
            "base" => p.base = c,
            "text" => p.text = c,
            "primary" => p.primary = c,
            "on_primary" => p.on_primary = c,
            "secondary" => p.secondary = c,
            "error" => p.error = c,
            _ => {}
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::{Palette, Rgba, parse};

    #[test]
    fn parses_known_colors_and_keeps_defaults_for_missing_colors() {
        let palette = parse("@define-color base #010203;\n@define-color primary #aabbcc;\n");

        assert_eq!(palette.base, Rgba(1, 2, 3, 255));
        assert_eq!(palette.primary, Rgba(0xaa, 0xbb, 0xcc, 255));
        assert_eq!(palette.text, Palette::default().text);
    }

    /// Регрессия: не-ASCII в значении цвета не должно ронять разбор. Раньше
    /// `hex.len()` считал байты, «ёёё» давало ровно 6 и проходило проверку,
    /// после чего срез `hex[0..2]` разрезал UTF-8 между символами.
    #[test]
    fn multibyte_color_does_not_panic_and_is_ignored() {
        let palette = parse("@define-color base ёёё;\n@define-color primary #aabbcc;\n");

        assert_eq!(
            palette.base,
            Palette::default().base,
            "не-ASCII значение должно быть проигнорировано, а не разобрано"
        );
        assert_eq!(
            palette.primary,
            Rgba(0xaa, 0xbb, 0xcc, 255),
            "следующая строка разбирается как обычно"
        );
    }

    /// Четыре двухбайтовые буквы — восемь байт, длину проходит только при
    /// обрезании; проверка ASCII всё равно обязана отбросить значение.
    #[test]
    fn multibyte_color_of_other_length_is_ignored() {
        let palette = parse("@define-color text ёёёё;\n");

        assert_eq!(palette.text, Palette::default().text);
    }

    /// Настоящий триггер паники: «€» занимает три байта, поэтому в строке
    /// «€abc» ровно 6 байт, длина проходит, а `hex[0..2]` разрезает UTF-8
    /// между байтами символа. Двухбайтовые буквы («ёёё») такой границы не
    /// дают и были безопасны всегда — этот тест остаётся как страховка.
    #[test]
    fn three_byte_char_does_not_panic_and_is_ignored() {
        let palette = parse("@define-color base €abc;\n@define-color primary #aabbcc;\n");

        assert_eq!(palette.base, Palette::default().base);
        assert_eq!(palette.primary, Rgba(0xaa, 0xbb, 0xcc, 255));
    }

    /// Смешанный случай: кириллица вперемешку с латиницей, длина случайно
    /// совпала с шестью байтами.
    #[test]
    fn mixed_multibyte_color_is_ignored() {
        let palette = parse("@define-color text aёbcd;\n@define-color base ёa€bc;\n");

        assert_eq!(palette.text, Palette::default().text);
        assert_eq!(palette.base, Palette::default().base);
    }

    #[test]
    fn ignores_invalid_color_lines() {
        let palette = parse("@define-color base #xyz;\n@define-color text #12345;\n");

        assert_eq!(palette, Palette::default());
    }
}
