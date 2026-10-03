//! Уведомления dunst: точечная замена трёх значений в двух файлах.
//!
//! `matugen` владеет всеми цветами и генерирует `dunst/dunstrc` из
//! `matugen/templates/dunst.conf`. Значит править нужно шаблон, а `dunstrc` —
//! производный файл, который мы обновляем точечно, не трогая цвета: полный
//! прогон matugen пересчитал бы палитру и сломал бы её.
//!
//! Правило замены: строка целиком не переписывается. Если в значении есть
//! плейсхолдер `{{...}}`, меняется только значение после плейсхолдера.

use std::path::Path;
use std::process::Command;

use super::config::Config;
use super::config_io::{WriteError, atomic_write, home};

/// Три значения, которыми управляет окно настроек.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DunstValues {
    /// Например `JetBrainsMono Nerd Font 13`.
    pub font: String,
    pub line_height: u32,
    /// Dunst ждёт `top-right`, `bottom-left` и подобное.
    pub origin: String,
}

pub fn values_from_config(config: &Config) -> DunstValues {
    let theme = config.theme.unwrap_or(super::config::Theme::Normal);
    DunstValues {
        font: format!("{} {}", theme.font(), config.font_size),
        line_height: config.line_height,
        origin: config.position.key().to_string(),
    }
}

impl DunstValues {
    fn font_value(&self) -> String {
        self.font.trim().to_string()
    }
}

/// Новое значение для строки с ключом `key`, если оно отличается от текущего.
///
/// `_ => return None` — единственная защита от посторонних строк; сравнение с
/// `current` ниже её дублирует, но служит второй линией на случай, если
/// значение чужого ключа случайно совпало бы с нашим.
fn replacement(key: &str, current: &str, values: &DunstValues) -> Option<String> {
    let new_value = match key {
        "font" => values.font_value(),
        "line_height" => values.line_height.to_string(),
        "origin" => values.origin.trim().to_string(),
        _ => return None,
    };
    if new_value.is_empty() || new_value == current {
        return None;
    }
    Some(new_value)
}

/// Меняет значение в строке, не трогая плейсхолдер, если он есть.
///
/// Возвращает `None`, если строка не наша или менять нечего.
fn patch_line(line: &str, values: &DunstValues) -> Option<String> {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let (key, rest) = trimmed.split_once('=')?;
    let key = key.trim();
    let current = rest.trim();
    // Ключ проверяется в `replacement`: это единственное место, где решается,
    // наша ли строка. Там же отсекаются комментарии: у `# font = 10` ключ
    // получается `# font`, а не `font`.
    let new_value = replacement(key, current, values)?;

    // Плейсхолдер matugen не трогаем: если новое значение начинается с него же,
    // префикс уже приходит вместе со значением, иначе он бы продублировался.
    // Кавычки вокруг значения сохраняются: переписываем только содержимое.
    let quoted = current.starts_with('"') && current.ends_with('"') && current.len() >= 2;
    let bare = if quoted {
        &current[1..current.len() - 1]
    } else {
        current
    };

    let placeholder_end = bare
        .find("{{")
        .and_then(|start| bare[start..].find("}}").map(|end| start + end + 2));
    let keep_prefix = placeholder_end.is_some() && !new_value.starts_with("{{");
    let rebuilt = match (keep_prefix, placeholder_end) {
        (true, Some(end)) => {
            let prefix = &bare[..end];
            // Плейсхолдер и шрифт разделяем пробелом, но лишний не нужен.
            if prefix.ends_with(char::is_whitespace) || new_value.starts_with(char::is_whitespace) {
                format!("{prefix}{new_value}")
            } else {
                format!("{prefix} {new_value}")
            }
        }
        _ => new_value,
    };
    let value = if quoted {
        format!("\"{rebuilt}\"")
    } else {
        rebuilt
    };
    Some(format!("{}{key} = {value}", &line[..indent_len]))
}

/// Точечно обновляет файл. Остальные строки возвращаются байт в байт.
pub fn patch_file(path: &Path, values: &DunstValues) -> Result<bool, WriteError> {
    let text = std::fs::read_to_string(path).map_err(|why| WriteError::Read(why.to_string()))?;
    let (patched, changed) = patch_text(&text, values);
    if !changed {
        return Ok(false);
    }
    atomic_write(path, &patched)?;
    Ok(true)
}

fn patch_text(text: &str, values: &DunstValues) -> (String, bool) {
    let had_trailing_newline = text.ends_with('\n');
    let mut changed = false;
    let mut out: Vec<String> = Vec::with_capacity(text.len());
    for line in text.lines() {
        match patch_line(line, values) {
            Some(next) => {
                changed = true;
                out.push(next);
            }
            None => out.push(line.to_string()),
        }
    }
    let mut patched = out.join("\n");
    if had_trailing_newline {
        patched.push('\n');
    }
    (patched, changed)
}

pub fn template_path() -> std::path::PathBuf {
    home().join(".config/matugen/templates/dunst.conf")
}

pub fn dunstrc_path() -> std::path::PathBuf {
    home().join(".config/dunst/dunstrc")
}

/// Обновляет оба файла. Возвращает `Ok(false)`, если значения уже совпадают и
/// перезаписывать ничего не нужно.
pub fn apply(values: &DunstValues) -> Result<bool, WriteError> {
    let changed = apply_files(&template_path(), &dunstrc_path(), values)?;
    if changed {
        reload();
    }
    Ok(changed)
}

/// Чистая часть `apply`: пишет оба файла и возвращает, было ли что менять.
/// Вынесена отдельно, чтобы тесты проверяли её без запуска `dunstctl`.
pub fn apply_files(
    template: &Path,
    generated: &Path,
    values: &DunstValues,
) -> Result<bool, WriteError> {
    let mut changed = false;
    // Шаблон — источник правды, поэтому он идёт первым.
    if patch_file(template, values)? {
        changed = true;
    }
    if patch_file(generated, values)? {
        changed = true;
    }
    Ok(changed)
}

/// Просит dunst перечитать конфиг. Ошибка не роняет применение: настройки
/// уже записаны, dunst подхватит их при следующем чтении.
pub fn reload() {
    let _ = Command::new("dunstctl").arg("reload").status();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> DunstValues {
        DunstValues {
            font: "JetBrainsMono Nerd Font 14".to_string(),
            line_height: 18,
            origin: "bottom-left".to_string(),
        }
    }

    fn changed_font() -> DunstValues {
        DunstValues {
            font: "Minecraft Rus 14".to_string(),
            ..values()
        }
    }

    #[test]
    fn replaces_the_three_values() {
        let (patched, changed) = patch_text(
            "font = JetBrainsMono Nerd Font 13\nline_height = 15\norigin = top-right\n",
            &values(),
        );

        assert!(changed);
        assert_eq!(
            patched,
            "font = JetBrainsMono Nerd Font 14\nline_height = 18\norigin = bottom-left\n"
        );
    }

    #[test]
    fn leaves_every_other_line_byte_identical() {
        let original = concat!(
            "[global]\n",
            "separator_height = 0\n",
            "frame_color = \"{{colors.primary.default.hex}}59\"\n",
            "highlight = \"{{colors.primary.default.hex}}\"\n",
            "font = JetBrainsMono Nerd Font 13\n",
            "gap_size = 8\n",
            "    background = \"{{colors.surface.default.hex}}FA\"\n",
            "    foreground = \"{{colors.on_surface.default.hex}}\"\n",
            "line_height = 15\n",
            "format = \"<span foreground='{{colors.primary.default.hex}}'>%s</b></span>\\n%b\"\n",
            "origin = top-right\n",
            "width = (320, 550)\n",
        );
        let expected_placeholders = original.matches("{{").count();

        let (patched, changed) = patch_text(original, &values());

        assert!(changed);
        assert_eq!(
            patched.matches("{{").count(),
            expected_placeholders,
            "плейсхолдеры {{colors.*}} должны сохраниться"
        );
        // Каждая неизменённая строка обязана совпасть побайтово.
        let before: Vec<&str> = original.lines().collect();
        let after: Vec<&str> = patched.lines().collect();
        assert_eq!(before.len(), after.len(), "число строк изменилось");
        let targets = ["font = ", "line_height = ", "origin = "];
        for (old, new) in before.iter().zip(after.iter()) {
            if targets.iter().any(|target| old.starts_with(target)) {
                continue;
            }
            assert_eq!(old, new, "строка изменена: {old:?} -> {new:?}");
        }
    }

    #[test]
    fn keeps_placeholder_prefix_when_the_value_has_one() {
        let (patched, changed) = patch_text(
            "font = \"{{colors.text.default.hex}} 13\"\n",
            &DunstValues {
                font: "{{colors.text.default.hex}} 16".to_string(),
                ..values()
            },
        );

        assert!(changed);
        assert_eq!(patched, "font = \"{{colors.text.default.hex}} 16\"\n");
        assert_eq!(patched.matches("{{").count(), 1, "плейсхолдер не потерян");
    }

    #[test]
    fn placeholder_survives_when_only_the_number_changes() {
        let (patched, _) = patch_text(
            "font = \"{{colors.text.default.hex}} 13\"\n",
            &DunstValues {
                font: "{{colors.text.default.hex}} 18".to_string(),
                ..values()
            },
        );

        assert_eq!(patched, "font = \"{{colors.text.default.hex}} 18\"\n");
    }

    #[test]
    fn keeps_indentation_and_quotes() {
        let (patched, _) = patch_text("    line_height = 15\n", &values());

        assert_eq!(patched, "    line_height = 18\n");
    }

    #[test]
    fn ignores_commented_out_lines() {
        let original = "# font = Old 10\n#line_height = 5\n# origin = middle\n";
        let (patched, changed) = patch_text(original, &values());

        assert!(!changed, "закомментированные строки не трогаются");
        assert_eq!(patched, original);
    }

    #[test]
    fn ignores_unrelated_keys() {
        let original = "gap_size = 8\ncorner_radius = 0\nmin_icon_size = 64\n";
        let (patched, changed) = patch_text(original, &values());

        assert!(!changed);
        assert_eq!(patched, original);
    }

    #[test]
    fn reports_no_change_when_values_already_match() {
        let original =
            "font = JetBrainsMono Nerd Font 14\nline_height = 18\norigin = bottom-left\n";
        let (patched, changed) = patch_text(original, &values());

        assert!(!changed, "совпадающие значения не требуют записи");
        assert_eq!(patched, original);
    }

    #[test]
    fn preserves_missing_trailing_newline() {
        let (patched, _) = patch_text("font = JetBrainsMono Nerd Font 13", &values());

        assert_eq!(patched, "font = JetBrainsMono Nerd Font 14");
        assert!(!patched.ends_with('\n'));
    }

    #[test]
    fn preserves_trailing_newline_when_present() {
        let (patched, _) = patch_text("origin = top-right\n", &values());

        assert!(patched.ends_with('\n'));
    }

    #[test]
    fn placeholder_prefix_survives_when_the_new_value_has_none() {
        // Старое значение — с плейсхолдером, новое — без. Плейсхолдер matugen
        // обязан остаться, иначе шаблон потеряет привязку к палитре.
        let (patched, changed) = patch_text(
            "font = {{colors.text.default.hex}} 13\n",
            &DunstValues {
                font: "Minecraft Rus 14".to_string(),
                ..values()
            },
        );

        assert!(changed);
        assert_eq!(
            patched,
            "font = {{colors.text.default.hex}} Minecraft Rus 14\n"
        );
        assert_eq!(patched.matches("{{").count(), 1);
    }

    #[test]
    fn pixel_theme_font_survives_the_patch() {
        let original = "font = JetBrainsMono Nerd Font 13\n";

        let (patched, changed) = patch_text(original, &changed_font());

        assert!(changed);
        assert_eq!(patched, "font = Minecraft Rus 14\n");
    }

    #[test]
    fn both_files_end_up_with_identical_values() {
        // Реальный случай: шаблон и dunstrc отличаются только цветами.
        let template = concat!(
            "[global]\n",
            "frame_color = \"{{colors.primary.default.hex}}59\"\n",
            "font = JetBrainsMono Nerd Font 13\n",
            "line_height = 15\n",
            "origin = top-right\n",
            "    background = \"{{colors.surface.default.hex}}FA\"\n",
        );
        let dunstrc = concat!(
            "[global]\n",
            "frame_color = \"#a7c8ff59\"\n",
            "font = JetBrainsMono Nerd Font 13\n",
            "line_height = 15\n",
            "origin = top-right\n",
            "    background = \"#0f141cFA\"\n",
        );

        let placeholders_before = template.matches("{{").count();
        let (from_template, _) = patch_text(template, &values());
        let (from_dunstrc, _) = patch_text(dunstrc, &values());

        for key in ["font", "line_height", "origin"] {
            let in_template = line_with_key(&from_template, key);
            let in_dunstrc = line_with_key(&from_dunstrc, key);
            assert_eq!(in_template, in_dunstrc, "{key} разошёлся между файлами");
        }
        assert_eq!(
            from_template.matches("{{").count(),
            placeholders_before,
            "плейсхолдеры шаблона должны сохраниться"
        );
        assert_eq!(
            from_dunstrc.matches("{{").count(),
            0,
            "в dunstrc их не было"
        );
    }

    fn line_with_key(text: &str, key: &str) -> String {
        text.lines()
            .find(|line| line.trim_start().starts_with(key))
            .unwrap_or_default()
            .to_string()
    }

    // ---- уровень patch_line: каждая строка проверяется отдельно ----

    #[test]
    fn patch_line_refuses_commented_lines() {
        // Комментарий отсекается белым списком ключей: у `# font = Old 10`
        // ключ получается `# font`, а не `font`.
        assert_eq!(patch_line("# font = Old 10", &values()), None);
        assert_eq!(patch_line("  ; font = Old 10", &values()), None);
        assert_eq!(patch_line("// origin = middle", &values()), None);
        assert_eq!(patch_line("#line_height = 5", &values()), None);
    }

    #[test]
    fn patch_line_refuses_unrelated_keys() {
        for line in ["gap_size = 8", "corner_radius = 0", "width = (320, 550)"] {
            assert_eq!(patch_line(line, &values()), None, "{line}");
        }
    }

    #[test]
    fn patch_line_refuses_lines_without_assignment() {
        assert_eq!(patch_line("[global]", &values()), None);
        assert_eq!(patch_line("", &values()), None);
        assert_eq!(patch_line("font", &values()), None);
    }

    #[test]
    fn patch_line_patches_each_target_key() {
        assert_eq!(
            patch_line("font = JetBrainsMono Nerd Font 13", &values()).as_deref(),
            Some("font = JetBrainsMono Nerd Font 14")
        );
        assert_eq!(
            patch_line("line_height = 15", &values()).as_deref(),
            Some("line_height = 18")
        );
        assert_eq!(
            patch_line("origin = top-right", &values()).as_deref(),
            Some("origin = bottom-left")
        );
    }

    #[test]
    fn font_value_is_trimmed() {
        // В settings.json значение может прийти с пробелами, а в dunst-конфиг
        // пробелы вокруг шрифта попадать не должны.
        let padded = DunstValues {
            font: "  JetBrainsMono Nerd Font 14  ".to_string(),
            ..values()
        };

        let (patched, changed) = patch_text("font = Old 10\n", &padded);

        assert!(changed);
        assert_eq!(patched, "font = JetBrainsMono Nerd Font 14\n");
    }

    #[test]
    fn origin_value_is_trimmed_too() {
        let padded = DunstValues {
            origin: "  bottom-left ".to_string(),
            ..values()
        };

        let (patched, _) = patch_text("origin = top-right\n", &padded);

        assert_eq!(patched, "origin = bottom-left\n");
    }

    #[test]
    fn apply_reloads_only_when_something_changed() {
        // Решение о reload принимает `apply_files`, а сам `reload` — побочный
        // эффект. Проверяем решение, а не запуск dunstctl.
        let dir = std::env::temp_dir().join("hudbar-dunst-reload");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let template = dir.join("dunst.conf");
        let generated = dir.join("dunstrc");
        let stale = "font = Old 10\nline_height = 15\norigin = top-right\n";
        let fresh = "font = JetBrainsMono Nerd Font 14\nline_height = 18\norigin = bottom-left\n";
        std::fs::write(&template, stale).unwrap();
        std::fs::write(&generated, stale).unwrap();

        assert!(apply_files(&template, &generated, &values()).unwrap());
        // Второй вызов уже ничего не меняет — reload был бы лишним.
        assert!(!apply_files(&template, &generated, &values()).unwrap());
        assert_eq!(std::fs::read_to_string(&template).unwrap(), fresh);
    }

    // ---- уровень patch_file: запись на диск целиком ----

    #[test]
    fn patch_file_writes_atomically_and_keeps_placeholders() {
        let dir = std::env::temp_dir().join("hudbar-dunst-patch-file");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dunst.conf");
        let original = concat!(
            "[global]\n",
            "frame_color = \"{{colors.primary.default.hex}}59\"\n",
            "font = JetBrainsMono Nerd Font 13\n",
            "line_height = 15\n",
            "origin = top-right\n",
        );
        std::fs::write(&path, original).unwrap();

        let changed = patch_file(&path, &values()).unwrap();

        assert!(changed);
        let patched = std::fs::read_to_string(&path).unwrap();
        assert!(patched.contains("frame_color = \"{{colors.primary.default.hex}}59\""));
        assert!(patched.contains("font = JetBrainsMono Nerd Font 14"));
        assert!(patched.contains("line_height = 18"));
        assert!(patched.contains("origin = bottom-left"));
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(
            leftovers.is_empty(),
            "остались временные файлы: {leftovers:?}"
        );
    }

    #[test]
    fn patch_file_reports_no_change_and_keeps_mtime_intact() {
        let dir = std::env::temp_dir().join("hudbar-dunst-noop");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dunst.conf");
        let original =
            "font = JetBrainsMono Nerd Font 14\nline_height = 18\norigin = bottom-left\n";
        std::fs::write(&path, original).unwrap();
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();

        let changed = patch_file(&path, &values()).unwrap();

        assert!(!changed, "совпадающие значения не переписывают файл");
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn patch_file_fails_loudly_on_a_missing_file() {
        let path = std::env::temp_dir().join("hudbar-dunst-missing/nope.conf");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("hudbar-dunst-missing"));

        let error = patch_file(&path, &values()).unwrap_err();

        assert!(
            matches!(error, WriteError::Read(_)),
            "отсутствие файла — это ошибка чтения"
        );
    }

    // ---- уровень apply: оба файла и reload ----

    #[test]
    fn apply_files_writes_both_and_reports_a_change() {
        let dir = std::env::temp_dir().join("hudbar-dunst-apply");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let template = dir.join("dunst.conf");
        let generated = dir.join("dunstrc");
        std::fs::write(
            &template,
            "font = JetBrainsMono Nerd Font 13\nline_height = 15\norigin = top-right\n",
        )
        .unwrap();
        std::fs::write(
            &generated,
            "font = JetBrainsMono Nerd Font 13\nline_height = 15\norigin = top-right\n",
        )
        .unwrap();

        let changed = apply_files(&template, &generated, &values()).unwrap();

        assert!(changed);
        for path in [&template, &generated] {
            let text = std::fs::read_to_string(path).unwrap();
            assert!(
                text.contains("font = JetBrainsMono Nerd Font 14"),
                "{path:?}"
            );
            assert!(text.contains("line_height = 18"), "{path:?}");
            assert!(text.contains("origin = bottom-left"), "{path:?}");
        }
    }

    #[test]
    fn apply_files_is_a_no_op_when_both_already_match() {
        let dir = std::env::temp_dir().join("hudbar-dunst-apply-noop");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let template = dir.join("dunst.conf");
        let generated = dir.join("dunstrc");
        let text = "font = JetBrainsMono Nerd Font 14\nline_height = 18\norigin = bottom-left\n";
        std::fs::write(&template, text).unwrap();
        std::fs::write(&generated, text).unwrap();

        let changed = apply_files(&template, &generated, &values()).unwrap();

        assert!(!changed, "без изменений перезаписывать нечего");
        assert_eq!(std::fs::read_to_string(&template).unwrap(), text);
    }

    #[test]
    fn apply_files_stops_at_a_missing_file_without_writing_the_second() {
        let dir = std::env::temp_dir().join("hudbar-dunst-apply-missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let template = dir.join("dunst.conf");
        let generated = dir.join("dunstrc");
        std::fs::write(&template, "font = JetBrainsMono Nerd Font 13\n").unwrap();

        let error = apply_files(&template, &generated, &values());

        assert!(matches!(error, Err(WriteError::Read(_))));
        assert!(
            !generated.exists(),
            "второй файл не должен появляться, если первый не прочитан"
        );
    }

    #[test]
    fn apply_touches_the_template_and_the_generated_file() {
        // Оба пути — под путями matugen/dunst, и ни один не про цвета.
        let template = template_path().to_string_lossy().into_owned();
        let generated = dunstrc_path().to_string_lossy().into_owned();

        assert!(template.contains("matugen/templates/dunst.conf"));
        assert!(generated.ends_with("dunst/dunstrc"));
        assert!(!template.ends_with(".css"));
        assert!(!generated.ends_with(".css"));
        assert!(!template.contains("colors.css"));
        assert!(!generated.contains("colors.css"));
    }
}
