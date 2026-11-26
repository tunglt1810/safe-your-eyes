use nsh_core::{
    element::{AdaptiveColor, Rgba, rgb, rgba},
    geometry::{Points, px},
    platform::Appearance,
};
use sye_core::config::ThemeMode;

pub const SUPPORT_URL: &str = "https://buymeacoffee.com/bbeeezzzzz";
pub const DOWNLOAD_PAGE_URL: &str = sye_core::PUBLIC_DOWNLOAD_URL;

pub(crate) const fn appearance_for_theme_mode(mode: ThemeMode) -> Appearance {
    match mode {
        ThemeMode::System => Appearance::System,
        ThemeMode::Light => Appearance::Light,
        ThemeMode::Dark => Appearance::Dark,
    }
}

pub(crate) const fn theme_mode_for_appearance(appearance: Appearance) -> ThemeMode {
    match appearance {
        Appearance::System => ThemeMode::System,
        Appearance::Light => ThemeMode::Light,
        Appearance::Dark => ThemeMode::Dark,
    }
}

pub(crate) const fn next_theme_mode(mode: ThemeMode) -> ThemeMode {
    match mode {
        ThemeMode::System => ThemeMode::Light,
        ThemeMode::Light => ThemeMode::Dark,
        ThemeMode::Dark => ThemeMode::System,
    }
}

pub(crate) const fn theme_mode_label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::System => "Theme: System",
        ThemeMode::Light => "Theme: Light",
        ThemeMode::Dark => "Theme: Dark",
    }
}

const fn tone(light: Rgba, dark: Rgba) -> AdaptiveColor {
    AdaptiveColor {
        light: light.to_hsla(),
        dark: dark.to_hsla(),
    }
}

pub(crate) mod colors {
    use super::{AdaptiveColor, rgb, rgba, tone};

    pub(crate) const BACKGROUND_BODY: AdaptiveColor = tone(rgb(0xFAEBDA), rgb(0x18140E));
    pub(crate) const BACKGROUND_SURFACE: AdaptiveColor = tone(rgb(0xFBF1E1), rgb(0x231D16));
    pub(crate) const BACKGROUND_MUTED: AdaptiveColor = tone(rgb(0xF3DFC8), rgb(0x30261D));
    pub(crate) const TRACK: AdaptiveColor = tone(rgb(0xE6D2BC), rgb(0x30271E));

    pub(crate) const SIDEBAR_BG: AdaptiveColor = tone(rgb(0xC3CBB9), rgb(0x29332A));
    pub(crate) const SIDEBAR_ACTIVE: AdaptiveColor = tone(rgb(0xF8E8D3), rgb(0x404A41));
    pub(crate) const SIDEBAR_TEXT: AdaptiveColor = tone(rgb(0x403B26), rgb(0xFFFEF7));
    pub(crate) const SIDEBAR_TEXT_MUTED: AdaptiveColor = tone(rgb(0x55503A), rgb(0xB8C4B6));
    pub(crate) const OVERLAY_HOVER: AdaptiveColor = tone(rgba(0x2611010D), rgba(0xFFFEF70F));

    pub(crate) const ACCENT: AdaptiveColor = tone(rgb(0xEF8F63), rgb(0xEE8F64));
    pub(crate) const ON_ACCENT: AdaptiveColor = tone(rgb(0x261101), rgb(0x18140E));
    pub(crate) const FOCUS_RING: AdaptiveColor = tone(rgb(0xA84A24), rgb(0xF2A57E));

    pub(crate) const TEXT_PRIMARY: AdaptiveColor = tone(rgb(0x261101), rgb(0xFFFEF7));
    pub(crate) const TEXT_SECONDARY: AdaptiveColor = tone(rgb(0x5B4C3B), rgb(0xC2B7B1));
    pub(crate) const TEXT_DISABLED: AdaptiveColor = tone(rgb(0x968574), rgb(0x70655C));

    pub(crate) const WARNING: AdaptiveColor = tone(rgb(0xB87A12), rgb(0xF5C156));
    pub(crate) const ERROR: AdaptiveColor = tone(rgb(0xC8433B), rgb(0xF27B70));
    pub(crate) const WARNING_SOFT: AdaptiveColor = tone(rgb(0xFAE1B5), rgb(0x443A26));
    pub(crate) const ON_WARNING_MUTED: AdaptiveColor = tone(rgb(0x6B4408), rgb(0xFCE09B));
    pub(crate) const ERROR_SOFT: AdaptiveColor = tone(rgb(0xF9CFC8), rgb(0x472826));
    pub(crate) const ON_ERROR_MUTED: AdaptiveColor = tone(rgb(0x7E211C), rgb(0xF9BEB8));
    pub(crate) const ACCENT_SOFT: AdaptiveColor = tone(rgba(0xEF8F6333), rgba(0xEE8F6438));
    pub(crate) const HEADING_HIGHLIGHT: AdaptiveColor = tone(rgb(0xFAD3B9), rgb(0x7D4737));

    pub(crate) const BORDER: AdaptiveColor = tone(rgba(0x5B4C3B22), rgb(0x3E3831));
    pub(crate) const BORDER_EMPHASIZED: AdaptiveColor = tone(rgb(0xDFCBB5), rgb(0x52493F));

    pub(crate) const SWITCH_KNOB: AdaptiveColor = tone(rgb(0xFFFEF7), rgb(0xFFFEF7));

    pub(crate) const OVERLAY_CANVAS: AdaptiveColor = tone(rgb(0x18140E), rgb(0x18140E));
    pub(crate) const OVERLAY_CARD: AdaptiveColor = tone(rgb(0x231D16), rgb(0x231D16));
    pub(crate) const OVERLAY_TEXT: AdaptiveColor = tone(rgb(0xFFFEF7), rgb(0xFFFEF7));
    pub(crate) const OVERLAY_MUTED: AdaptiveColor = tone(rgb(0xC2B7B1), rgb(0xC2B7B1));
    pub(crate) const OVERLAY_TRACK: AdaptiveColor = tone(rgb(0x30271E), rgb(0x30271E));
    pub(crate) const OVERLAY_BORDER: AdaptiveColor = tone(rgb(0x3E3831), rgb(0x3E3831));
    pub(crate) const OVERLAY_BORDER_EMPHASIZED: AdaptiveColor = tone(rgb(0x52493F), rgb(0x52493F));
}

pub(crate) mod kpi {
    use super::{AdaptiveColor, rgb, tone};

    pub(crate) const SAGE_BG: AdaptiveColor = tone(rgb(0xE2DEC4), rgb(0x31392C));
    pub(crate) const SAGE_ACCENT: AdaptiveColor = tone(rgb(0x7EAB9A), rgb(0x5E8C7A));

    pub(crate) const PEACH_BG: AdaptiveColor = tone(rgb(0xFBCBB4), rgb(0x432F28));
    pub(crate) const PEACH_ACCENT: AdaptiveColor = tone(rgb(0xF48A65), rgb(0xE5866B));

    pub(crate) const BUTTER_BG: AdaptiveColor = tone(rgb(0xFAE1B5), rgb(0x443A26));
    pub(crate) const BUTTER_ACCENT: AdaptiveColor = tone(rgb(0xFAC76A), rgb(0xD9A74E));

    pub(crate) const MIST_BG: AdaptiveColor = tone(rgb(0xD2D7DA), rgb(0x2B333D));
    pub(crate) const MIST_ACCENT: AdaptiveColor = tone(rgb(0xA2B3C1), rgb(0x6D8296));
}

pub(crate) mod radii {
    use super::{Points, px};

    pub(crate) const ELEMENT: Points = px(8.0);
    pub(crate) const NAV_ITEM: Points = px(8.0);
    pub(crate) const CONTAINER: Points = px(10.0);
    pub(crate) const KPI: Points = px(10.0);
    pub(crate) const SIDEBAR: Points = px(10.0);
    pub(crate) const PAGE: Points = px(28.0);
    pub(crate) const PILL: Points = px(9999.0);
}

pub(crate) mod motion {
    use nsh_core::animation::Easing;
    use nsh_core::element::Spring;

    pub(crate) const COZY_SPRING: Spring = Spring {
        stiffness: 420.0,
        damping: 32.0,
        mass: 0.9,
        velocity: None,
        frame_rate: None,
    };

    pub(crate) const COZY_EASE: Easing = Easing::Bezier(0.22, 1.0, 0.36, 1.0);
    pub(crate) const COZY_STANDARD_BEZIER: (f32, f32, f32, f32) = (0.4, 0.0, 0.2, 1.0);

    pub(crate) const HIGHLIGHT_SWEEP_MS: f32 = 380.0;
    pub(crate) const THEME_COLOR_MS: f32 = 200.0;
    pub(crate) const ROW_ENTER_MS: f32 = 260.0;
    pub(crate) const ROW_ENTER_Y: f32 = 10.0;
    pub(crate) const ROW_STAGGER_STEP_MS: f32 = 28.0;
    pub(crate) const ROW_STAGGER_MAX_INDEX: usize = 12;
    pub(crate) const CONTROL_MS: f32 = 160.0;
    pub(crate) const PRESS_SCALE: f32 = 0.97;

    pub(crate) fn row_stagger_delay_ms(index: usize) -> f32 {
        (index.min(ROW_STAGGER_MAX_INDEX) as f32) * ROW_STAGGER_STEP_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_mode_cycle_and_appearance_mapping_round_trip() {
        assert_eq!(next_theme_mode(ThemeMode::System), ThemeMode::Light);
        assert_eq!(next_theme_mode(ThemeMode::Light), ThemeMode::Dark);
        assert_eq!(next_theme_mode(ThemeMode::Dark), ThemeMode::System);

        for mode in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
            let appearance = appearance_for_theme_mode(mode);
            assert_eq!(theme_mode_for_appearance(appearance), mode);
        }

        assert_eq!(theme_mode_label(ThemeMode::System), "Theme: System");
        assert_eq!(theme_mode_label(ThemeMode::Light), "Theme: Light");
        assert_eq!(theme_mode_label(ThemeMode::Dark), "Theme: Dark");
    }

    #[test]
    fn concentric_radii_match_cozy_scale_and_hierarchy() {
        const {
            assert!(radii::ELEMENT.0 < radii::CONTAINER.0);
            assert!(radii::NAV_ITEM.0 < radii::SIDEBAR.0);
            assert!(radii::SIDEBAR.0 < 16.0);
            assert!(radii::CONTAINER.0 < 16.0);
            assert!(radii::SIDEBAR.0 < radii::PAGE.0);
        }
        assert_eq!(radii::ELEMENT, px(8.0));
        assert_eq!(radii::NAV_ITEM, px(8.0));
        assert_eq!(radii::CONTAINER, px(10.0));
        assert_eq!(radii::KPI, px(10.0));
        assert_eq!(radii::SIDEBAR, px(10.0));
        assert_eq!(radii::PAGE, px(28.0));
        assert_eq!(motion::COZY_STANDARD_BEZIER, (0.4, 0.0, 0.2, 1.0));
    }

    #[test]
    fn row_stagger_delay_caps_at_max_index() {
        assert_eq!(motion::row_stagger_delay_ms(0), 0.0);
        assert_eq!(motion::row_stagger_delay_ms(1), 28.0);
        assert_eq!(motion::row_stagger_delay_ms(5), 140.0);
        assert_eq!(motion::row_stagger_delay_ms(12), 336.0);
        assert_eq!(motion::row_stagger_delay_ms(20), 336.0);
    }
}
