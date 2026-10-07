//! Кэш превью в стиле `wall.sh`: ключи, stale-проверка и командные строки,
//! которыми отсутствующие изображения восполняются.

use std::path::{Path, PathBuf};

/// Спецификация программной команды без фактического запуска. Отделена от
/// `std::process::Command`: тесты сборки команд не должны поднимать vips/magick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: &'static str,
    pub args: Vec<String>,
}

/// Размер превью как в wall.sh.
pub const PREVIEW_W: u32 = 400;
pub const PREVIEW_H: u32 = 225;

/// `~/.cache/wallpaper-previews`.
pub fn cache_dir() -> PathBuf {
    home().join(".cache").join("wallpaper-previews")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Имя наличного превью в каталоге `cache_dir()`.
pub fn preview_for(cache: &Path, file: &Path) -> PathBuf {
    cache.join(format!(
        "image-w3-{}.png",
        sha256_hex(&file.to_string_lossy())
    ))
}

/// Файл «моложе» превью? Если превью единственное или его нет вовсе —
/// генерация нужна. То же, что `[ "$file" -nt "$preview" ]` в wall.sh.
pub fn is_stale(file: &Path, preview: &Path) -> bool {
    let Ok(meta_file) = std::fs::metadata(file) else {
        return true;
    };
    let Ok(meta_preview) = std::fs::metadata(preview) else {
        return true;
    };
    let Ok(mt_file) = meta_file.modified() else {
        return true;
    };
    let Ok(mt_preview) = meta_preview.modified() else {
        return true;
    };
    mt_file > mt_preview
}

/// Команда `vipsthumbnail` как в wall.sh.
pub fn vipsthumbnail(file: &Path, preview: &Path) -> CommandSpec {
    CommandSpec {
        program: "vipsthumbnail",
        args: vec![
            file.to_string_lossy().to_string(),
            "--size".into(),
            format!("{}x{}", PREVIEW_W, PREVIEW_H),
            "--smartcrop".into(),
            "centre".into(),
            "--vips-concurrency=1".into(),
            "-o".into(),
            preview.to_string_lossy().to_string(),
        ],
    }
}

/// Команда `magick` как fallback через `[0]` как в wall.sh.
pub fn magick(file: &Path, preview: &Path) -> CommandSpec {
    CommandSpec {
        program: "magick",
        args: vec![
            format!("{}[0]", file.to_string_lossy()),
            "-thumbnail".into(),
            format!("{}x{}^", PREVIEW_W, PREVIEW_H),
            "-gravity".into(),
            "center".into(),
            "-extent".into(),
            format!("{}x{}", PREVIEW_W, PREVIEW_H),
            preview.to_string_lossy().to_string(),
        ],
    }
}

/// Временное имя превью перед rename (та же идея, что `.tmp.XXXXXX.png` в
/// wall.sh, но стабильная для снапшотов тестов: когда в кэшируемом файле
/// плотнеет tmp, она совместима с `$` там).
pub fn temp_preview(preview: &Path) -> PathBuf {
    let name = preview
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tmp");
    let parent = preview.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(".{}.tmp.png", name))
}

/// Битое имя команды в корректный вид не приводим: это тестовый формат.
pub fn sha256_hex(text: &str) -> String {
    sha256(text.as_bytes())
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>()
}

/// Полный SHA-256 пакета без внешних зависимостей: имена ключей используются
/// только как публичные идентификаторы, поэтому внешний crate не нужен.
fn sha256(words: &[u8]) -> [u8; 32] {
    let mut message = words.to_vec();
    let bit_len = (words.len() * 8) as u64;
    message.push(0x80);
    while (message.len() % 64) != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    for number in 0..message.len() / 64 {
        let block = &message[number * 64..number * 64 + 64];
        let mut w = [0u32; 64];
        for (i, chunk) in w.iter_mut().enumerate().take(16) {
            *chunk = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = (h[i / 4] >> (24 - (i % 4) * 8)) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Известный SHA-256 векторного значения, поэтому ключ не может «пустой
    /// предъявить» хеш.
    #[test]
    fn sha256_matches_the_known_test_vector() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// Ключ стабилен относительно пути: одно и то же изображение получает одно
    /// и то же имя, другой путь — другое имя.
    #[test]
    fn preview_key_is_stable_for_the_same_path() {
        let cache = PathBuf::from("/tmp");
        let file = PathBuf::from("/home/mihail/wallpapers/anime/x.png");
        let first = preview_for(&cache, &file);
        let second = preview_for(&cache, &file);
        assert_eq!(first, second);
        assert!(first.ends_with(format!(
            "image-w3-{}.png",
            sha256_hex("/home/mihail/wallpapers/anime/x.png")
        )));
        let different = preview_for(
            &cache,
            &PathBuf::from("/home/mihail/wallpapers/anime/y.png"),
        );
        assert_ne!(first, different);
    }

    /// stale: превью нет/устарело, если файл «моложе».
    #[test]
    fn is_stale_flags_missing_and_older_previews() {
        let file = std::env::temp_dir().join("hudbar-wp-stale-test.png");
        let preview = std::env::temp_dir().join("hudbar-wp-stale-test.preview.png");
        std::fs::write(&file, b"x").unwrap();
        let _ = std::fs::remove_file(&preview);
        assert!(is_stale(&file, &preview), "нет превью — надо генерировать");
        std::fs::write(&preview, b"x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&file, b"x").unwrap();
        assert!(is_stale(&file, &preview), "файл обновлён после превью");
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(&preview);
    }

    /// Сборка команд: vipsthumbnail с --smartcrop centre, magick с
    /// thumbnail^/gravity center/extent — тот же рецепт, что у wall.sh.
    #[test]
    fn commands_keep_the_wall_sh_recipe() {
        let file = PathBuf::from("/tmp/a.png");
        let preview = PathBuf::from("/tmp/out.png");
        let vips = vipsthumbnail(&file, &preview);
        assert_eq!(vips.program, "vipsthumbnail");
        assert!(
            vips.args
                .windows(2)
                .any(|pair| pair == ["--smartcrop", "centre"])
        );
        assert!(vips.args.iter().any(|arg| arg == "400x225"));
        let magick = magick(&file, &preview);
        assert_eq!(magick.program, "magick");
        assert_eq!(magick.args[0], "/tmp/a.png[0]");
        assert!(magick.args.iter().any(|arg| arg == "400x225^"));
        assert!(magick.args.iter().any(|arg| arg == "-extent"));
    }
}
