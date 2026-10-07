//! Swatches схем matugen: схема → четыре цвета для точек в полосе «Схема».

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Формат:
/// ```text
/// {
///   "scheme-tonal-spot": ["#b5c4ff", "#f0b8ff", ...],
///   ...
/// }
/// ```
pub type Swatches = BTreeMap<String, Vec<String>>;

/// `~/.config/hudbar/scheme-swatches.json`.
pub fn swatches_path() -> PathBuf {
    home().join(".config/hudbar/scheme-swatches.json")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Загружает файл; отсутствующий или битый — `None`.
pub fn load() -> Option<Swatches> {
    let text = std::fs::read_to_string(swatches_path()).ok()?;
    parse(&text)
}

/// Чистая функция разбора документа без внешних зависимостей:
/// объект ключей → массив из 4 строк; ничего другого не принимаем.
pub fn parse(text: &str) -> Option<Swatches> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return None;
    };
    let object = value.as_object()?;
    let mut map = BTreeMap::new();
    for (key, entry) in object {
        let colors = entry.as_array()?;
        if colors.len() != 4 {
            return None;
        }
        let mut out = Vec::with_capacity(4);
        for color in colors {
            let string = color.as_str()?;
            if !string.starts_with('#') || (string.len() != 7 && string.len() != 9) {
                return None;
            }
            out.push(string.to_string());
        }
        map.insert(key.clone(), out);
    }
    Some(map)
}

/// Сериализация обратно, та же форма что в load.
pub fn serialize(map: &Swatches) -> String {
    let mut out = String::from("{\n");
    let mut first = true;
    for (key, colors) in map {
        if !first {
            out.push_str(",\n");
        }
        first = false;
        out.push_str(&format!("  \"{}\": [", key));
        let mut first_color = true;
        for color in colors {
            if !first_color {
                out.push_str(", ");
            }
            first_color = false;
            out.push_str(&format!("\"{}\"", color));
        }
        out.push(']');
    }
    out.push_str("\n}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Валидный документ: объект → массивы из 4 и только цветов.
    #[test]
    fn parse_reads_a_well_formed_document() {
        let text = serde_json::json!({
            "scheme-tonal-spot": ["#b5c4ff", "#c1c5dd", "#121318", "#ffb4ab"],
            "scheme-smart": ["#aaaaaa", "#bbbbbb", "#cccccc", "#dddddd"],
        })
        .to_string();
        let map = parse(&text).expect("документ распознан");
        assert_eq!(map.len(), 2);
        let tonal = map.get("scheme-tonal-spot").unwrap();
        assert_eq!(tonal.len(), 4);
        assert_eq!(tonal[0], "#b5c4ff");
    }

    /// Ошибочные формы возвращают None, а не молчаливый пропуск.
    #[test]
    fn parse_rejects_invalid_shapes() {
        assert!(parse("[]").is_none());
        assert!(parse("{\"a\": [\"#aabbcc\"]}").is_none()); // слишком короткий
        assert!(parse("{\"a\": [\"aabbcc\", \"x\", \"y\", \"z\"]}").is_none()); // нет '#'
        assert!(parse("broken").is_none());
    }

    /// Круговая дорожка: serialize(parse(x)) даёт тот же логический документ.
    #[test]
    fn serialize_round_trips_parse() {
        let mut map = BTreeMap::new();
        map.insert(
            "scheme-tonal-spot".to_string(),
            vec![
                "#b5c4ff".into(),
                "#c1c5dd".into(),
                "#121318".into(),
                "#ffb4ab".into(),
            ],
        );
        let text = serialize(&map);
        let parsed = parse(&text).expect("после сериализации документ чинён");
        assert_eq!(parsed, map);
    }
}
