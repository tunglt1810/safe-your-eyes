use sye_core::Locale;

#[must_use]
pub fn detect_system_locale() -> Locale {
    #[cfg(target_os = "macos")]
    {
        if let Some(locale) = macos_impl::detect_locale() {
            return locale;
        }
        detect_locale_from_process_unix_env()
    }

    #[cfg(windows)]
    {
        windows_impl::detect_locale()
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        detect_locale_from_process_unix_env()
    }
}

#[cfg(unix)]
fn detect_locale_from_process_unix_env() -> Locale {
    let lc_all = std::env::var("LC_ALL").ok();
    let language = std::env::var("LANGUAGE").ok();
    let lc_messages = std::env::var("LC_MESSAGES").ok();
    let lang = std::env::var("LANG").ok();
    detect_locale_from_unix_env(
        lc_all.as_deref(),
        language.as_deref(),
        lc_messages.as_deref(),
        lang.as_deref(),
    )
}

#[cfg(any(unix, test))]
#[must_use]
pub(crate) fn detect_locale_from_unix_env(
    lc_all: Option<&str>,
    language: Option<&str>,
    lc_messages: Option<&str>,
    lang: Option<&str>,
) -> Locale {
    // Same precedence as gettext: LANGUAGE wins unless the effective message locale is C/POSIX.
    let effective = [lc_all, lc_messages, lang]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty());
    if let Some(val) = effective {
        let base = val.split(['.', '@']).next().unwrap_or(val);
        if base == "C" || base == "POSIX" {
            return Locale::En;
        }
    }

    if let Some(list) = language.map(str::trim).filter(|s| !s.is_empty()) {
        for tag in list.split(':').map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(locale) = Locale::from_tag(tag) {
                return locale;
            }
        }
    }

    effective.and_then(Locale::from_tag).unwrap_or(Locale::En)
}

#[cfg(any(windows, test))]
#[must_use]
pub(crate) fn parse_windows_multi_sz_tags(buf: &[u16]) -> Vec<String> {
    let mut tags = Vec::new();
    for segment in buf.split(|&unit| unit == 0) {
        if segment.is_empty() {
            break;
        }
        tags.push(String::from_utf16_lossy(segment));
    }
    tags
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{NSUserDefaults, ns_string};
    use sye_core::Locale;

    pub(super) fn detect_locale() -> Option<Locale> {
        autoreleasepool(|_| {
            let defaults = NSUserDefaults::standardUserDefaults();
            let langs = defaults.stringArrayForKey(ns_string!("AppleLanguages"))?;
            for item in langs {
                if let Some(locale) = Locale::from_tag(&item.to_string()) {
                    return Some(locale);
                }
            }
            None
        })
    }
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Win32 Globalization API for user preferred UI language and default locale"
)]
mod windows_impl {
    use sye_core::Locale;
    use windows_sys::Win32::Globalization::{
        GetUserDefaultLocaleName, GetUserPreferredUILanguages, MUI_LANGUAGE_NAME,
    };

    // windows-sys only exposes this as `u32` under `Win32_System_SystemServices`, but
    // `GetUserDefaultLocaleName` takes an `i32` length.
    const LOCALE_NAME_MAX_LENGTH: i32 = 85;

    use super::parse_windows_multi_sz_tags;

    pub(super) fn detect_locale() -> Locale {
        if let Some(locale) = detect_from_preferred_ui_languages() {
            return locale;
        }
        detect_from_default_locale_name().unwrap_or(Locale::En)
    }

    fn detect_from_preferred_ui_languages() -> Option<Locale> {
        let mut num_languages: u32 = 0;
        let mut buf_len: u32 = 0;
        // SAFETY: Passing null buffer with `buf_len = 0` queries the required UTF-16 buffer length.
        let ok = unsafe {
            GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                &mut num_languages,
                std::ptr::null_mut(),
                &mut buf_len,
            )
        };
        if ok == 0 || buf_len == 0 {
            return None;
        }

        let mut buf = vec![0u16; usize::try_from(buf_len).ok()?];
        // SAFETY: `buf` has length `buf_len` UTF-16 code units as reported by the previous call.
        let ok = unsafe {
            GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                &mut num_languages,
                buf.as_mut_ptr(),
                &mut buf_len,
            )
        };
        if ok == 0 {
            return None;
        }

        let tags = parse_windows_multi_sz_tags(&buf);
        for tag in &tags {
            if let Some(locale) = Locale::from_tag(tag) {
                return Some(locale);
            }
        }
        None
    }

    fn detect_from_default_locale_name() -> Option<Locale> {
        let cap = usize::try_from(LOCALE_NAME_MAX_LENGTH).ok()?;
        let mut buf = vec![0u16; cap];
        // SAFETY: `buf` has capacity `LOCALE_NAME_MAX_LENGTH` UTF-16 code units.
        let written = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), LOCALE_NAME_MAX_LENGTH) };
        let len = usize::try_from(written).ok()?.checked_sub(1)?;
        let tag = String::from_utf16_lossy(&buf[..len.min(buf.len())]);
        Locale::from_tag(&tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_env_precedence_matches_gettext() {
        assert_eq!(
            detect_locale_from_unix_env(Some("vi_VN.UTF-8"), None, None, Some("en_US")),
            Locale::Vi
        );
        assert_eq!(
            detect_locale_from_unix_env(Some("en_US.UTF-8"), Some("vi:en"), None, None),
            Locale::Vi
        );
        assert_eq!(
            detect_locale_from_unix_env(None, Some("fr_FR"), None, Some("vi_VN.UTF-8")),
            Locale::Vi
        );
        assert_eq!(
            detect_locale_from_unix_env(None, Some("vi"), None, Some("C.UTF-8")),
            Locale::En
        );
        assert_eq!(
            detect_locale_from_unix_env(Some("C"), Some("vi_VN"), Some("vi_VN"), Some("vi_VN")),
            Locale::En
        );
        assert_eq!(
            detect_locale_from_unix_env(None, Some("fr_FR:vi_VN:en_US"), None, Some("en_US")),
            Locale::Vi
        );
        assert_eq!(
            detect_locale_from_unix_env(None, Some(""), Some("vi_VN.UTF-8"), Some("en_US.UTF-8")),
            Locale::Vi
        );
        assert_eq!(
            detect_locale_from_unix_env(None, None, None, Some("vi_VN.UTF-8")),
            Locale::Vi
        );
        assert_eq!(
            detect_locale_from_unix_env(None, None, None, Some("ja_JP.UTF-8")),
            Locale::En
        );
        assert_eq!(
            detect_locale_from_unix_env(None, None, None, None),
            Locale::En
        );
    }

    #[test]
    fn windows_multi_sz_parser_extracts_ordered_language_tags() {
        let mut raw: Vec<u16> = "vi-VN".encode_utf16().collect();
        raw.push(0);
        raw.extend("en-US".encode_utf16());
        raw.push(0);
        raw.push(0);

        let tags = parse_windows_multi_sz_tags(&raw);
        assert_eq!(tags, vec!["vi-VN", "en-US"]);
        assert_eq!(
            Locale::from_tags(tags.iter().map(String::as_str)),
            Locale::Vi
        );
    }
}
