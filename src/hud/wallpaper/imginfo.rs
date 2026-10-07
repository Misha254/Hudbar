//! Размеры изображения из заголовка, без полной декодировки.

use std::path::Path;

/// Читает PNG/JPEG/GIF/WEBP заголовок и возвращает `(width, height)`.
// Только префикс файла: размеры лежат в первых килобайтах, а целиком
// двухмегабайтные фото ради подписи читать незачем — особенно в живом окне,
// где подписи нужны десяткам файлов сразу.
pub fn dimensions(path: &Path) -> Option<(u32, u32)> {
    dimensions_of(&read_prefix(path, 131_072)?)
}

/// Размеры из уже прочитанных байт: то же, что `dimensions`, но без диска.
// Нужна предзагрузке подписей, которая читает префиксы сама пачкой.
pub fn dimensions_of(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        parse_png(bytes)
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        parse_jpeg(bytes)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        parse_gif(bytes)
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        parse_webp(bytes)
    } else {
        None
    }
}

/// Первые `limit` байт файла: заголовкам хватает, остальное не трогаем.
fn read_prefix(path: &Path, limit: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::with_capacity(limit.min(65536));
    file.take(limit as u64).read_to_end(&mut bytes).ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

fn parse_png(bytes: &[u8]) -> Option<(u32, u32)> {
    // IHDR идёт первым чанком: 8 байт сигнатуры + 4 байта длины + «IHDR».
    if bytes.len() < 24 {
        return None;
    }
    if &bytes[12..16] != b"IHDR" {
        return None;
    }
    Some((read_be_u32(bytes, 16)?, read_be_u32(bytes, 20)?))
}

fn parse_jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut index = 2;
    while index + 9 <= bytes.len() {
        if bytes[index] != 0xFF {
            break;
        }
        let marker = bytes[index + 1];
        // Заполняющий байт перед маркером: не маркер, длины за ним нет.
        if marker == 0xFF {
            index += 1;
            continue;
        }
        // Маркеры без поля длины: у них за 0xFF сразу идёт следующий байт.
        // Читать для них «длину» нельзя — это уже данные другого сегмента,
        // из-за чего разбор уходил в сторону или обрывался.
        if marker == 0x01 || marker == 0xD8 || (0xD0..=0xD7).contains(&marker) {
            index += 2;
            continue;
        }
        if marker == 0xC0 || marker == 0xC1 || marker == 0xC2 {
            // SOFn: fp 1 байт, height 2 байта, width 2 байта.
            let height = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]);
            let width = u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]);
            return Some((width as u32, height as u32));
        }
        let len = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]) as usize;
        if len < 2 {
            return None;
        }
        index += 2 + len;
    }
    None
}

fn parse_gif(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 10 {
        return None;
    }
    Some((
        u16::from_le_bytes([bytes[6], bytes[7]]) as u32,
        u16::from_le_bytes([bytes[8], bytes[9]]) as u32,
    ))
}

fn parse_webp(bytes: &[u8]) -> Option<(u32, u32)> {
    // Упрощённый разбор чанков RIFF/WEBP.
    let chunk = bytes.get(12..16)?;
    if chunk == b"VP8X" {
        // Канvas: 3 байта width-1 и height-1 после 4-байтного флага.
        let width = 1
            + (*bytes.get(24)? as u32)
            + ((*bytes.get(25)? as u32) << 8)
            + ((*bytes.get(26)? as u32) << 16);
        let height = 1
            + (*bytes.get(27)? as u32)
            + ((*bytes.get(28)? as u32) << 8)
            + ((*bytes.get(29)? as u32) << 16);
        return Some((width, height));
    }
    if chunk == b"VP8L" && bytes.get(20) == Some(&0x2F) {
        // 5 байт после подписи.
        let b1 = *bytes.get(21)?;
        let b2 = *bytes.get(22)?;
        let b3 = *bytes.get(23)?;
        let b4 = *bytes.get(24)?;
        let width = (((b2 as u32) & 0x3F) << 8) | b1 as u32;
        let height = ((b4 as u32) << 6) | ((b3 as u32) >> 2);
        return Some((width + 1, height + 1));
    }
    None
}

fn read_be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
        *bytes.get(offset + 2)?,
        *bytes.get(offset + 3)?,
    ]))
}

/// Сокращённая метка соотношения сторон для подписи под миниатюрой.
pub fn ratio_label(width: u32, height: u32) -> String {
    if width == 0 || height == 0 {
        return format!("{}×{}", width, height);
    }
    let ratio = width as f64 / height as f64;
    let candidates: [(f64, &str); 7] = [
        (16.0 / 9.0, "16:9"),
        (21.0 / 9.0, "21:9"),
        (16.0 / 10.0, "16:10"),
        (4.0 / 3.0, "4:3"),
        (3.0 / 2.0, "3:2"),
        (1.0, "1:1"),
        (9.0 / 16.0, "9:16"),
    ];
    let mut best = (f64::MAX, "W×H");
    for (candidate, label) in candidates {
        let error = (ratio - candidate).abs() / candidate;
        if error < best.0 {
            best = (error, label);
        }
    }
    if best.0 <= 0.08 {
        best.1.to_string()
    } else {
        format!("{}×{}", width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Синтетический заголовок PNG с известными IHDR-полями.
    #[test]
    fn png_dimensions_come_from_ihdr() {
        let mut bytes = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
        bytes.extend_from_slice(&4u32.to_be_bytes()); // длина чанка
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&1920u32.to_be_bytes());
        bytes.extend_from_slice(&1080u32.to_be_bytes());
        let path =
            std::env::temp_dir().join(format!("hudbar-wp-imginfo-{}.png", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(dimensions(&path), Some((1920, 1080)));
        let _ = std::fs::remove_file(&path);
    }

    /// Синтетический JPEG: FF D9, затем декларация маркера SOF0.
    #[test]
    fn jpeg_dimensions_come_from_sofn() {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
        // Длина сегмента включает и саму себя: 2 байта + 15 байт полезной части.
        bytes.extend_from_slice(&17u16.to_be_bytes());
        bytes.extend_from_slice(b"JFIF\0\0\x01\x01\x00\x00\x01\x00\x01\x00\x00");
        bytes.extend_from_slice(&[0xFF, 0xC0]);
        bytes.extend_from_slice(&17u16.to_be_bytes());
        bytes.push(8); // precision
        bytes.extend_from_slice(&1080u16.to_be_bytes());
        bytes.extend_from_slice(&1920u16.to_be_bytes());
        let path =
            std::env::temp_dir().join(format!("hudbar-wp-imginfo-{}.jpg", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(dimensions(&path), Some((1920, 1080)));
        let _ = std::fs::remove_file(&path);
    }

    /// Регрессия: маркеры без поля длины (RSTn, TEM, SOI) и заполняющие
    /// байты 0xFF раньше читались как сегменты с длиной, из-за чего разбор
    /// уходил в данные следующего сегмента и возвращал None вместо SOFn.
    #[test]
    fn jpeg_survives_markers_without_a_length_field() {
        let mut bytes = vec![0xFF, 0xD8]; // SOI — длины нет
        bytes.extend_from_slice(&[0xFF, 0xFF]); // заполняющий байт
        bytes.extend_from_slice(&[0xFF, 0xD0]); // RST0 — длины нет
        bytes.extend_from_slice(&[0xFF, 0x01]); // TEM — длины нет
        bytes.extend_from_slice(&[0xFF, 0xD7]); // RST7 — длины нет
        bytes.extend_from_slice(&[0xFF, 0xE0]); // APP0, у него длина есть
        bytes.extend_from_slice(&17u16.to_be_bytes());
        bytes.extend_from_slice(b"JFIF\0\0\x01\x01\x00\x00\x01\x00\x01\x00\x00");
        bytes.extend_from_slice(&[0xFF, 0xC0]);
        bytes.extend_from_slice(&17u16.to_be_bytes());
        bytes.push(8);
        bytes.extend_from_slice(&1080u16.to_be_bytes());
        bytes.extend_from_slice(&1920u16.to_be_bytes());

        assert_eq!(parse_jpeg(&bytes), Some((1920, 1080)));
    }

    /// Тот же случай, но маркеров без длины в файле нет — обычный путь не
    /// должен пострадать.
    #[test]
    fn jpeg_with_only_length_bearing_markers_is_unaffected() {
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&[0xFF, 0xE1]);
        // Длина 2 — это только два байта самого поля длины, полезной части нет.
        bytes.extend_from_slice(&2u16.to_be_bytes());
        bytes.extend_from_slice(&[0xFF, 0xC0]);
        bytes.extend_from_slice(&17u16.to_be_bytes());
        bytes.push(8);
        bytes.extend_from_slice(&720u16.to_be_bytes());
        bytes.extend_from_slice(&1280u16.to_be_bytes());

        assert_eq!(parse_jpeg(&bytes), Some((1280, 720)));
    }

    #[test]
    fn gif_dimensions_come_from_logical_screen_descriptor() {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend_from_slice(&640u16.to_le_bytes());
        bytes.extend_from_slice(&480u16.to_le_bytes());
        let path =
            std::env::temp_dir().join(format!("hudbar-wp-imginfo-{}.gif", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(dimensions(&path), Some((640, 480)));
        let _ = std::fs::remove_file(&path);
    }

    /// Синтетический WEBP с VP8X-чанком: 3-байтные поля минус один.
    #[test]
    fn webp_dimensions_come_from_vp8x() {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"VP8X");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0]); // 4 байта флага
        let w = 1280u32;
        let h = 720u32;
        bytes.extend_from_slice(&[
            ((w - 1) & 0xFF) as u8,
            (((w - 1) >> 8) & 0xFF) as u8,
            (((w - 1) >> 16) & 0xFF) as u8,
        ]);
        bytes.extend_from_slice(&[
            ((h - 1) & 0xFF) as u8,
            (((h - 1) >> 8) & 0xFF) as u8,
            (((h - 1) >> 16) & 0xFF) as u8,
        ]);
        let path =
            std::env::temp_dir().join(format!("hudbar-wp-imginfo-{}.webp", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(dimensions(&path), Some((1280, 720)));
        let _ = std::fs::remove_file(&path);
    }

    /// Знакомые отношения сторон получают метки, прочие — W×H.
    #[test]
    fn ratio_labels_match_the_common_shapes() {
        assert_eq!(ratio_label(1920, 1080), "16:9");
        assert_eq!(ratio_label(2560, 1080), "21:9");
        assert_eq!(ratio_label(1920, 1200), "16:10");
        assert_eq!(ratio_label(1024, 768), "4:3");
        assert_eq!(ratio_label(3, 2), "3:2");
        assert_eq!(ratio_label(1000, 1000), "1:1");
        assert_eq!(ratio_label(1080, 1920), "9:16");
        assert_eq!(ratio_label(2000, 1000), "2000×1000");
    }
}
