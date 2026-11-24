//! Cozy Design System tokens for `safe-your-eyes`.
//!
//! Reference: <https://github.com/tunglt1810/cozy> (`v0.2.0` — `tokens/cozy.tokens.json`,
//! `css/effects.css`, `docs/motion.md`, and `docs/patterns.md`), combined with Astryx layout rules.

use std::sync::OnceLock;

use nsh_core::{
    element::{AdaptiveColor, Rgba, rgb, rgba},
    geometry::{Points, px},
    platform::Appearance,
    shader::Shader,
};
use sye_core::config::ThemeMode;

/// Author support URL shown in Settings (About & Support) and documentation.
pub const SUPPORT_URL: &str = "https://buymeacoffee.com/bbeeezzzzz";

/// Maps a persisted `ThemeMode` to an `nsh` `Appearance`.
pub(crate) const fn appearance_for_theme_mode(mode: ThemeMode) -> Appearance {
    match mode {
        ThemeMode::System => Appearance::System,
        ThemeMode::Light => Appearance::Light,
        ThemeMode::Dark => Appearance::Dark,
    }
}

/// Maps an `nsh` `Appearance` back to a persisted `ThemeMode`.
pub(crate) const fn theme_mode_for_appearance(appearance: Appearance) -> ThemeMode {
    match appearance {
        Appearance::System => ThemeMode::System,
        Appearance::Light => ThemeMode::Light,
        Appearance::Dark => ThemeMode::Dark,
    }
}

/// Cycles Cozy theme modes in canonical order: `System -> Light -> Dark -> System`.
pub(crate) const fn next_theme_mode(mode: ThemeMode) -> ThemeMode {
    match mode {
        ThemeMode::System => ThemeMode::Light,
        ThemeMode::Light => ThemeMode::Dark,
        ThemeMode::Dark => ThemeMode::System,
    }
}

/// Display label for the top-right Cozy theme toggle pill.
pub(crate) const fn theme_mode_label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::System => "Theme: System",
        ThemeMode::Light => "Theme: Light",
        ThemeMode::Dark => "Theme: Dark",
    }
}

/// Pairs light and dark `Rgba` tones into an `AdaptiveColor`.
const fn tone(light: Rgba, dark: Rgba) -> AdaptiveColor {
    AdaptiveColor {
        light: light.to_hsla(),
        dark: dark.to_hsla(),
    }
}

/// Semantic Cozy palette tokens (Light / Dark) from `tunglt1810/cozy` v0.2.0.
pub(crate) mod colors {
    use super::{AdaptiveColor, rgb, rgba, tone};

    /// Main canvas background (`--cozy-bg-body`: `#FAEBDA` / `#18140E`).
    pub(crate) const BACKGROUND_BODY: AdaptiveColor = tone(rgb(0xFAEBDA), rgb(0x18140E));
    /// Card, panel, input, secondary button, and sticky toolbar surface (`--cozy-bg-surface`: `#FBF1E1` / `#231D16`).
    pub(crate) const BACKGROUND_SURFACE: AdaptiveColor = tone(rgb(0xFBF1E1), rgb(0x231D16));
    /// Quiet emphasis surface (`--cozy-bg-muted`: `#F3DFC8` / `#30261D`).
    pub(crate) const BACKGROUND_MUTED: AdaptiveColor = tone(rgb(0xF3DFC8), rgb(0x30261D));
    /// Switch and progress track (`--cozy-track`: `#E6D2BC` / `#30271E`).
    pub(crate) const TRACK: AdaptiveColor = tone(rgb(0xE6D2BC), rgb(0x30271E));

    /// Floating Sage Sidebar background (`--cozy-nav-bg`: `#C3CBB9` / `#29332A`).
    pub(crate) const SIDEBAR_BG: AdaptiveColor = tone(rgb(0xC3CBB9), rgb(0x29332A));
    /// Active navigation pill (`--cozy-nav-active`: `#F8E8D3` / `#404A41`).
    pub(crate) const SIDEBAR_ACTIVE: AdaptiveColor = tone(rgb(0xF8E8D3), rgb(0x404A41));
    /// Navigation label (`--cozy-nav-text`: `#403B26` / `#FFFEF7`).
    pub(crate) const SIDEBAR_TEXT: AdaptiveColor = tone(rgb(0x403B26), rgb(0xFFFEF7));
    /// Navigation secondary caption & version (`--cozy-nav-text-muted`: `#55503A` / `#B8C4B6`).
    pub(crate) const SIDEBAR_TEXT_MUTED: AdaptiveColor = tone(rgb(0x55503A), rgb(0xB8C4B6));
    /// Subtle hover layer on any surface (`--cozy-overlay-hover`: `#2611010D` / `#FFFEF70F`).
    pub(crate) const OVERLAY_HOVER: AdaptiveColor = tone(rgba(0x2611010D), rgba(0xFFFEF70F));

    /// Primary coral CTA accent (`--cozy-accent`: `#EF8F63` / `#EE8F64`).
    pub(crate) const ACCENT: AdaptiveColor = tone(rgb(0xEF8F63), rgb(0xEE8F64));
    /// WCAG AAA dark espresso text on `ACCENT` (`--cozy-on-accent`: `#261101` / `#18140E`).
    pub(crate) const ON_ACCENT: AdaptiveColor = tone(rgb(0x261101), rgb(0x18140E));
    /// Keyboard focus outline (`--cozy-focus-ring`: `#A84A24` / `#F2A57E`).
    pub(crate) const FOCUS_RING: AdaptiveColor = tone(rgb(0xA84A24), rgb(0xF2A57E));

    /// Warm primary text (`--cozy-text-primary`: `#261101` / `#FFFEF7`).
    pub(crate) const TEXT_PRIMARY: AdaptiveColor = tone(rgb(0x261101), rgb(0xFFFEF7));
    /// Warm secondary text (`--cozy-text-secondary`: `#5B4C3B` / `#C2B7B1`).
    pub(crate) const TEXT_SECONDARY: AdaptiveColor = tone(rgb(0x5B4C3B), rgb(0xC2B7B1));
    /// Warm muted/disabled text (`--cozy-text-disabled`: `#968574` / `#70655C`).
    pub(crate) const TEXT_DISABLED: AdaptiveColor = tone(rgb(0x968574), rgb(0x70655C));

    /// Status borders and icons (v0.2.0: light warning `#B87A12` reaches >= 3:1 on body/surface).
    pub(crate) const WARNING: AdaptiveColor = tone(rgb(0xB87A12), rgb(0xF5C156));
    pub(crate) const ERROR: AdaptiveColor = tone(rgb(0xC8433B), rgb(0xF27B70));
    /// Status banner & destructive chip surfaces and measured high-contrast text pairs.
    pub(crate) const WARNING_SOFT: AdaptiveColor = tone(rgb(0xFAE1B5), rgb(0x443A26));
    pub(crate) const ON_WARNING_MUTED: AdaptiveColor = tone(rgb(0x6B4408), rgb(0xFCE09B));
    pub(crate) const ERROR_SOFT: AdaptiveColor = tone(rgb(0xF9CFC8), rgb(0x472826));
    pub(crate) const ON_ERROR_MUTED: AdaptiveColor = tone(rgb(0x7E211C), rgb(0xF9BEB8));
    pub(crate) const ACCENT_SOFT: AdaptiveColor = tone(rgba(0xEF8F6333), rgba(0xEE8F6438));
    /// Highlighter stroke under headings (`--cozy-mark-peach`: `#FAD3B9` / `#7D4737`).
    pub(crate) const HEADING_HIGHLIGHT: AdaptiveColor = tone(rgb(0xFAD3B9), rgb(0x7D4737));

    /// Hairline and emphasized borders (`--cozy-border` / `--cozy-border-emphasized`).
    pub(crate) const BORDER: AdaptiveColor = tone(rgba(0x5B4C3B22), rgb(0x3E3831));
    pub(crate) const BORDER_EMPHASIZED: AdaptiveColor = tone(rgb(0xDFCBB5), rgb(0x52493F));

    /// Switch knob color (warm cream).
    pub(crate) const SWITCH_KNOB: AdaptiveColor = tone(rgb(0xFFFEF7), rgb(0xFFFEF7));

    /// Overlay always uses restful dark-roast tones so eyes relax during breaks.
    pub(crate) const OVERLAY_CANVAS: AdaptiveColor = tone(rgb(0x18140E), rgb(0x18140E));
    pub(crate) const OVERLAY_CARD: AdaptiveColor = tone(rgb(0x231D16), rgb(0x231D16));
    pub(crate) const OVERLAY_TEXT: AdaptiveColor = tone(rgb(0xFFFEF7), rgb(0xFFFEF7));
    pub(crate) const OVERLAY_MUTED: AdaptiveColor = tone(rgb(0xC2B7B1), rgb(0xC2B7B1));
    pub(crate) const OVERLAY_TRACK: AdaptiveColor = tone(rgb(0x30271E), rgb(0x30271E));
    pub(crate) const OVERLAY_BORDER: AdaptiveColor = tone(rgb(0x3E3831), rgb(0x3E3831));
    pub(crate) const OVERLAY_BORDER_EMPHASIZED: AdaptiveColor = tone(rgb(0x52493F), rgb(0x52493F));
}

/// Pastel KPI card color pairs (Sage, Peach, Butter, Mist) from `tokens/cozy.tokens.json`.
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

/// Concentric desktop border-radius scale obeying Cozy `container -> element` nesting against
/// native OS window corners.
pub(crate) mod radii {
    use super::{Points, px};

    pub(crate) const ELEMENT: Points = px(8.0);
    pub(crate) const NAV_ITEM: Points = px(8.0);
    pub(crate) const CONTAINER: Points = px(12.0);
    pub(crate) const KPI: Points = px(12.0);
    pub(crate) const SIDEBAR: Points = px(12.0);
    pub(crate) const PAGE: Points = px(28.0);
    pub(crate) const PILL: Points = px(9999.0);
}

/// Cozy motion presets matching `tunglt1810/cozy` `tokens/cozy.tokens.json` & `docs/motion.md`.
pub(crate) mod motion {
    use nsh_core::animation::Easing;
    use nsh_core::element::Spring;

    /// `spring.default` (`stiffness: 420, damping: 32, mass: 0.9`) for sliding indicators and switch thumbs.
    pub(crate) const COZY_SPRING: Spring = Spring {
        stiffness: 420.0,
        damping: 32.0,
        mass: 0.9,
        velocity: None,
        frame_rate: None,
    };

    /// `motion.ease-out` (`cubic-bezier(0.22, 1, 0.36, 1)`).
    pub(crate) const COZY_EASE: Easing = Easing::Bezier(0.22, 1.0, 0.36, 1.0);
    /// `motion.ease-standard` (`cubic-bezier(0.4, 0, 0.2, 1)`) for theme transitions.
    pub(crate) const COZY_STANDARD_BEZIER: (f32, f32, f32, f32) = (0.4, 0.0, 0.2, 1.0);

    /// `motion.duration-fast` (160ms): tab panel entrance and control color transitions.
    pub(crate) const PANEL_ENTER_MS: f32 = 160.0;
    /// Heading highlighter horizontal sweep duration (`css/effects.css`: 380ms).
    pub(crate) const HIGHLIGHT_SWEEP_MS: f32 = 380.0;
    /// `motion.duration-page` (200ms): overlay card scale-in duration.
    pub(crate) const THEME_COLOR_MS: f32 = 200.0;
    /// `motion.duration-slow` (480ms): circular theme reveal duration.
    pub(crate) const THEME_REVEAL_MS: u64 = 480;
    /// `motion.duration-fast` (160ms): standard hover/press/focus control transitions.
    pub(crate) const CONTROL_MS: f32 = 160.0;
    /// `.cozy-press:active` subtle press downscale (`0.97`).
    pub(crate) const PRESS_SCALE: f32 = 0.97;
}

/// WGSL shader implementing Cozy's Circular Theme Reveal (`480ms`, `cubic-bezier(0.4, 0, 0.2, 1)`).
///
/// `_pad` enforces 16-byte uniform buffer alignment required by strict GLES drivers.
pub(crate) const THEME_REVEAL_WGSL: &str = r"
struct Uniforms {
    origin: vec2f,
    _pad: vec2f,
}

fn bezier_x(t: f32) -> f32 {
    let u = 1.0 - t;
    return 3.0 * u * u * t * 0.4 + 3.0 * u * t * t * 0.2 + t * t * t;
}

fn bezier_dx(t: f32) -> f32 {
    let u = 1.0 - t;
    return 3.0 * u * u * 0.4 + 6.0 * u * t * (0.2 - 0.4) + 3.0 * t * t * (1.0 - 0.2);
}

fn bezier_y(t: f32) -> f32 {
    let u = 1.0 - t;
    return 3.0 * u * t * t * 1.0 + t * t * t;
}

fn ease_standard(x: f32) -> f32 {
    var t = clamp(x, 0.0, 1.0);
    for (var i = 0; i < 6; i = i + 1) {
        let err = bezier_x(t) - x;
        let d = max(bezier_dx(t), 1e-4);
        t = clamp(t - err / d, 0.0, 1.0);
    }
    return bezier_y(t);
}

fn shade(uv: vec2f) -> vec4f {
    let p = uv * nsh.size;
    let o = uniforms.origin;
    let max_dx = max(o.x, nsh.size.x - o.x);
    let max_dy = max(o.y, nsh.size.y - o.y);
    let max_r = length(vec2f(max_dx, max_dy));
    let aa = 1.0 / max(nsh.scale, 1.0);
    let r = (max_r + aa) * ease_standard(nsh.progress);
    let dist = length(p - o);
    let mask = 1.0 - smoothstep(r - aa, r + aa, dist);
    return mix(sample_from(uv), sample_to(uv), mask);
}
";

/// Compiles and caches the Cozy Circular Theme Reveal shader once per process.
pub(crate) fn theme_reveal_shader() -> Option<Shader> {
    static SHADER: OnceLock<Option<Shader>> = OnceLock::new();
    SHADER
        .get_or_init(|| Shader::compile(THEME_REVEAL_WGSL).ok())
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_reveal_wgsl_compiles_cleanly() {
        let compiled = Shader::compile(THEME_REVEAL_WGSL);
        assert!(
            compiled.is_ok(),
            "THEME_REVEAL_WGSL failed to compile: {:?}",
            compiled.err()
        );
        assert!(theme_reveal_shader().is_some());
    }

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
    fn concentric_radii_respect_container_to_element_hierarchy() {
        const {
            assert!(radii::ELEMENT.0 < radii::CONTAINER.0);
            assert!(radii::NAV_ITEM.0 < radii::SIDEBAR.0);
        }
        assert_eq!(radii::SIDEBAR, radii::CONTAINER);
        assert_eq!(radii::KPI, radii::CONTAINER);
        assert_eq!(motion::THEME_REVEAL_MS, 480);
        assert_eq!(motion::COZY_STANDARD_BEZIER, (0.4, 0.0, 0.2, 1.0));
    }
}
