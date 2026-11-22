//! Cozy Design System tokens for `safe-your-eyes`, aligned with `DESIGN.md` and Astryx.
//!
//! Uses warm oatmeal/espresso light tones and dark-roast/warm-cream dark tones,
//! eliminating pure `#FFF`/`#000`, enforcing WCAG AAA contrast on coral CTAs, and
//! using dedicated WCAG AA `--color-text-{warning,error}` tokens for semantic text.

use nsh_core::{
    element::{AdaptiveColor, Rgba, rgb, rgba},
    geometry::{Points, px},
};

/// Author support URL shown in Settings (About & Support) and documentation.
pub const SUPPORT_URL: &str = "https://buymeacoffee.com/bbeeezzzzz";

/// Converts `#rrggbb` + opacity into an `Rgba` value.
pub(crate) const fn rgb_a(hex: u32, alpha: f32) -> Rgba {
    let a = (alpha * 255.0 + 0.5) as u32;
    rgba((hex << 8) | (a & 0xFF))
}

/// Pairs light and dark `Rgba` tones into an `AdaptiveColor`.
const fn tone(light: Rgba, dark: Rgba) -> AdaptiveColor {
    AdaptiveColor {
        light: light.to_hsla(),
        dark: dark.to_hsla(),
    }
}

/// Semantic Cozy palette tokens (Light / Dark) matching `DESIGN.md` §4.1.
pub(crate) mod colors {
    use super::{AdaptiveColor, rgb, rgb_a, rgba, tone};

    /// Main canvas background (`--color-background-body`: `#FAEBDA` / `#18140E`).
    pub(crate) const BACKGROUND_BODY: AdaptiveColor = tone(rgb(0xFAEBDA), rgb(0x18140E));
    /// Card, panel, and sticky toolbar surface (`--color-background-surface`: `#FBF1E1` / `#231D16`).
    pub(crate) const BACKGROUND_SURFACE: AdaptiveColor = tone(rgb(0xFBF1E1), rgb(0x231D16));
    /// Segmented track, input fill, and subtle highlight (`--color-background-muted`: `#F3DFC8` / `#30261D`).
    pub(crate) const BACKGROUND_MUTED: AdaptiveColor = tone(rgb(0xF3DFC8), rgb(0x30261D));
    /// Elevated active pill inside `SegmentedControl` (brighter than track in both Light and Dark).
    pub(crate) const SEGMENT_ACTIVE: AdaptiveColor = tone(rgb(0xFBF1E1), rgb(0x46392E));

    /// Floating Sage Sidebar background (`--cozy-sidebar-bg`: `#C3CBB9` / `#29332A`).
    pub(crate) const SIDEBAR_BG: AdaptiveColor = tone(rgb(0xC3CBB9), rgb(0x29332A));
    /// Active navigation pill inside the Floating Sage Sidebar (`--cozy-sidebar-active`: `#F8E8D3` / `#404A41`).
    pub(crate) const SIDEBAR_ACTIVE: AdaptiveColor = tone(rgb(0xF8E8D3), rgb(0x404A41));
    /// Subtle hover tint on inactive sidebar items without reducing label contrast.
    pub(crate) const SIDEBAR_HOVER: AdaptiveColor =
        tone(rgb_a(0xF8E8D3, 0.45), rgb_a(0x404A41, 0.45));

    /// Primary coral CTA accent (`--color-accent`: `#EF8F63` / `#EE8F64`).
    pub(crate) const ACCENT: AdaptiveColor = tone(rgb(0xEF8F63), rgb(0xEE8F64));
    /// WCAG AAA dark espresso text on `ACCENT` (`--color-on-accent`: `#261101` / `#18140E`).
    pub(crate) const ON_ACCENT: AdaptiveColor = tone(rgb(0x261101), rgb(0x18140E));

    /// Warm primary text (`--color-text-primary`: `#261101` / `#FFFEF7`).
    pub(crate) const TEXT_PRIMARY: AdaptiveColor = tone(rgb(0x261101), rgb(0xFFFEF7));
    /// Warm secondary text (`--color-text-secondary`: `#5B4C3B` / `#C2B7B1`).
    pub(crate) const TEXT_SECONDARY: AdaptiveColor = tone(rgb(0x5B4C3B), rgb(0xC2B7B1));
    /// Warm muted/disabled text (`--color-text-disabled`: `#968574` / `#70655C`).
    pub(crate) const TEXT_DISABLED: AdaptiveColor = tone(rgb(0x968574), rgb(0x70655C));

    /// Semantic status text tokens achieving WCAG AA on Light & Dark surfaces (`DESIGN.md` §4.1).
    pub(crate) const TEXT_WARNING: AdaptiveColor = tone(rgb(0x8A5406), rgb(0xF5C96B));
    pub(crate) const TEXT_ERROR: AdaptiveColor = tone(rgb(0xB0322B), rgb(0xF5938A));

    /// Soft tinted backgrounds for status banners, highlighters, and destructive pills.
    pub(crate) const WARNING_SOFT: AdaptiveColor =
        tone(rgb_a(0xD99220, 0.14), rgb_a(0xF5C156, 0.16));
    pub(crate) const ERROR_SOFT: AdaptiveColor = tone(rgb_a(0xC8433B, 0.12), rgb_a(0xF27B70, 0.16));
    pub(crate) const ACCENT_SOFT: AdaptiveColor =
        tone(rgb_a(0xEF8F63, 0.18), rgb_a(0xEE8F64, 0.18));
    pub(crate) const HEADING_HIGHLIGHT: AdaptiveColor =
        tone(rgb_a(0xEF8F63, 0.42), rgb_a(0xEE8F64, 0.36));

    /// Hairline and emphasized borders (`--color-border` / `--color-border-emphasized`).
    pub(crate) const BORDER: AdaptiveColor = tone(rgba(0x5B4C3B22), rgb(0x3E3831));
    pub(crate) const BORDER_EMPHASIZED: AdaptiveColor = tone(rgb(0xDFCBB5), rgb(0x52493F));

    /// Switch knob color (warm cream).
    pub(crate) const SWITCH_KNOB: AdaptiveColor = tone(rgb(0xFFFEF7), rgb(0xFFFEF7));

    /// Overlay always uses restful dark-roast tones so eyes relax during breaks.
    pub(crate) const OVERLAY_CANVAS: AdaptiveColor = tone(rgb(0x18140E), rgb(0x18140E));
    pub(crate) const OVERLAY_CARD: AdaptiveColor = tone(rgb(0x231D16), rgb(0x231D16));
    pub(crate) const OVERLAY_TEXT: AdaptiveColor = tone(rgb(0xFFFEF7), rgb(0xFFFEF7));
    pub(crate) const OVERLAY_MUTED: AdaptiveColor = tone(rgb(0xC2B7B1), rgb(0xC2B7B1));
    pub(crate) const OVERLAY_TRACK: AdaptiveColor = tone(rgb(0x30261D), rgb(0x30261D));
    pub(crate) const OVERLAY_BORDER: AdaptiveColor = tone(rgb(0x3E3831), rgb(0x3E3831));
    pub(crate) const OVERLAY_BORDER_EMPHASIZED: AdaptiveColor = tone(rgb(0x52493F), rgb(0x52493F));
}

/// Pastel KPI card color pairs (Sage, Peach, Butter, Mist) matching `DESIGN.md` §4.1.
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

/// Cozy border-radius scale matching `DESIGN.md` §4.4 and Astryx shape relationships.
pub(crate) mod radii {
    use super::{Points, px};

    pub(crate) const ELEMENT: Points = px(12.0);
    pub(crate) const CONTAINER: Points = px(18.0);
    pub(crate) const KPI: Points = px(20.0);
    pub(crate) const SIDEBAR: Points = px(22.0);
    pub(crate) const PAGE: Points = px(28.0);
    pub(crate) const PILL: Points = px(9999.0);
}

/// Cozy motion presets matching `DESIGN.md` §5 and §7.4.
pub(crate) mod motion {
    use nsh_core::animation::Easing;
    use nsh_core::element::Spring;

    /// Smooth spring physics for sliding sidebar and segmented control indicators.
    pub(crate) const COZY_SPRING: Spring = Spring {
        stiffness: 380.0,
        damping: 32.0,
        mass: 1.0,
        velocity: None,
        frame_rate: None,
    };

    /// Decelerating Cozy ease curve (`cubic-bezier(0.22, 1, 0.36, 1)`).
    pub(crate) const COZY_EASE: Easing = Easing::Bezier(0.22, 1.0, 0.36, 1.0);
    pub(crate) const COZY_BEZIER: (f32, f32, f32, f32) = (0.22, 1.0, 0.36, 1.0);

    /// Tab panel entrance transition duration (`DESIGN.md` §7.4: 160ms).
    pub(crate) const PANEL_ENTER_MS: f32 = 160.0;
    /// Heading highlighter horizontal sweep duration (`DESIGN.md` §5.2: 380ms).
    pub(crate) const HIGHLIGHT_SWEEP_MS: f32 = 380.0;
    /// Surface color transition duration when toggling Light / Dark mode.
    pub(crate) const THEME_COLOR_MS: f32 = 220.0;
    /// Standard hover/press/focus control transition duration.
    pub(crate) const CONTROL_MS: f32 = 150.0;
}
