use serde::{Deserialize, Serialize};

use crate::config::{DEFAULT_LONG_BREAK_NAMES, DEFAULT_SHORT_BREAK_NAMES};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageMode {
    #[default]
    #[serde(alias = "auto")]
    System,
    En,
    Vi,
}

impl LanguageMode {
    #[must_use]
    pub const fn resolve(self, system_locale: Locale) -> Locale {
        match self {
            Self::System => system_locale,
            Self::En => Locale::En,
            Self::Vi => Locale::Vi,
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::System => Self::En,
            Self::En => Self::Vi,
            Self::Vi => Self::System,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    #[default]
    En,
    Vi,
}

impl Locale {
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            return None;
        }
        let without_encoding = trimmed.split(['.', '@']).next().unwrap_or(trimmed);
        let primary = without_encoding
            .split(['-', '_'])
            .next()
            .unwrap_or(without_encoding);
        if primary.eq_ignore_ascii_case("vi") {
            Some(Self::Vi)
        } else if primary.eq_ignore_ascii_case("en") {
            Some(Self::En)
        } else {
            None
        }
    }

    #[must_use]
    pub fn from_tags<'a>(tags: impl IntoIterator<Item = &'a str>) -> Self {
        for tag in tags {
            if let Some(locale) = Self::from_tag(tag) {
                return locale;
            }
        }
        Self::En
    }
}

const SHORT_BREAK_VI_TRANSLATIONS: [(&str, &str); 13] = [
    (DEFAULT_SHORT_BREAK_NAMES[0], "Nhẹ nhàng nhắm mắt lại"),
    (DEFAULT_SHORT_BREAK_NAMES[1], "Hít thở sâu và chậm năm lần"),
    (
        DEFAULT_SHORT_BREAK_NAMES[2],
        "Xoay mắt theo chiều kim đồng hồ",
    ),
    (
        DEFAULT_SHORT_BREAK_NAMES[3],
        "Thả lỏng vai và hạ vai xuống khỏi tai",
    ),
    (DEFAULT_SHORT_BREAK_NAMES[4], "Chớp mắt đều đặn"),
    (
        DEFAULT_SHORT_BREAK_NAMES[5],
        "Nghiêng cổ nhẹ nhàng sang hai bên",
    ),
    (DEFAULT_SHORT_BREAK_NAMES[6], "Đảo mắt vài lần sang hai bên"),
    (
        DEFAULT_SHORT_BREAK_NAMES[7],
        "Chỉnh lại tư thế và ngồi tựa thoải mái",
    ),
    (
        DEFAULT_SHORT_BREAK_NAMES[8],
        "Xoay mắt ngược chiều kim đồng hồ",
    ),
    (
        DEFAULT_SHORT_BREAK_NAMES[9],
        "Thả lỏng hàm và thư giãn cơ mặt",
    ),
    (
        DEFAULT_SHORT_BREAK_NAMES[10],
        "Nhìn tập trung vào một điểm ở xa",
    ),
    (DEFAULT_SHORT_BREAK_NAMES[11], "Duỗi cổ tay và các ngón tay"),
    (DEFAULT_SHORT_BREAK_NAMES[12], "Uống một ngụm nước"),
];

const LONG_BREAK_VI_TRANSLATIONS: [(&str, &str); 6] = [
    (
        DEFAULT_LONG_BREAK_NAMES[0],
        "Đứng dậy vươn vai, duỗi lưng và chân",
    ),
    (DEFAULT_LONG_BREAK_NAMES[1], "Ngả lưng ra ghế và thư giãn"),
    (
        DEFAULT_LONG_BREAK_NAMES[2],
        "Đi lại một chút và rót thêm nước",
    ),
    (DEFAULT_LONG_BREAK_NAMES[3], "Đi bộ một lát"),
    (
        DEFAULT_LONG_BREAK_NAMES[4],
        "Rời khỏi màn hình và giãn cơ nhẹ nhàng",
    ),
    (
        DEFAULT_LONG_BREAK_NAMES[5],
        "Cho mắt nghỉ ngơi và nhìn các vật ở xa",
    ),
];

#[must_use]
pub fn localize_exercise_name(name: &str, locale: Locale) -> &str {
    match locale {
        Locale::En => name,
        Locale::Vi => {
            let trimmed = name.trim();
            for &(en, vi) in &SHORT_BREAK_VI_TRANSLATIONS {
                if trimmed == en {
                    return vi;
                }
            }
            for &(en, vi) in &LONG_BREAK_VI_TRANSLATIONS {
                if trimmed == en {
                    return vi;
                }
            }
            name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_mode_resolves_and_cycles() {
        assert_eq!(LanguageMode::System.resolve(Locale::En), Locale::En);
        assert_eq!(LanguageMode::System.resolve(Locale::Vi), Locale::Vi);
        assert_eq!(LanguageMode::En.resolve(Locale::Vi), Locale::En);
        assert_eq!(LanguageMode::Vi.resolve(Locale::En), Locale::Vi);

        assert_eq!(LanguageMode::System.next(), LanguageMode::En);
        assert_eq!(LanguageMode::En.next(), LanguageMode::Vi);
        assert_eq!(LanguageMode::Vi.next(), LanguageMode::System);
    }

    #[test]
    fn locale_parses_bcp47_and_posix_tags() {
        assert_eq!(Locale::from_tag("vi"), Some(Locale::Vi));
        assert_eq!(Locale::from_tag("vi-VN"), Some(Locale::Vi));
        assert_eq!(Locale::from_tag("vi_VN.UTF-8"), Some(Locale::Vi));
        assert_eq!(Locale::from_tag("VI_VN@euro"), Some(Locale::Vi));
        assert_eq!(Locale::from_tag("en"), Some(Locale::En));
        assert_eq!(Locale::from_tag("en-US"), Some(Locale::En));
        assert_eq!(Locale::from_tag("en_GB.UTF-8"), Some(Locale::En));
        assert_eq!(Locale::from_tag("fr-FR"), None);
        assert_eq!(Locale::from_tag("C"), None);
        assert_eq!(Locale::from_tag("POSIX"), None);
        assert_eq!(Locale::from_tag("   "), None);

        assert_eq!(Locale::from_tags(["fr-FR", "vi-VN", "en-US"]), Locale::Vi);
        assert_eq!(Locale::from_tags(["ja-JP", "de-DE"]), Locale::En);
        assert_eq!(Locale::from_tags(std::iter::empty()), Locale::En);
    }

    #[test]
    fn all_default_exercises_have_vietnamese_translations_and_custom_names_pass_through() {
        for name in DEFAULT_SHORT_BREAK_NAMES
            .iter()
            .chain(DEFAULT_LONG_BREAK_NAMES.iter())
        {
            assert_eq!(localize_exercise_name(name, Locale::En), *name);
            let vi = localize_exercise_name(name, Locale::Vi);
            assert!(!vi.trim().is_empty());
            assert_ne!(vi, *name, "missing Vietnamese translation for `{name}`");
        }

        assert_eq!(
            localize_exercise_name("Stretch your wrists slowly", Locale::Vi),
            "Stretch your wrists slowly"
        );
        assert_eq!(
            localize_exercise_name("Uống trà xanh", Locale::En),
            "Uống trà xanh"
        );
        assert_eq!(
            localize_exercise_name("Uống trà xanh", Locale::Vi),
            "Uống trà xanh"
        );
    }
}
