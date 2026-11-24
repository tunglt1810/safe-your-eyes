//! Reusable Cozy styled UI primitives built on `nsh_base`, `theme`, and Astryx layout rules.
//!
//! Design system reference: <https://github.com/tunglt1810/cozy>.

use std::sync::{Arc, OnceLock};

use nsh_base::{button, input, link, switch};
use nsh_core::canvas::{LineCap, LineJoin};
use nsh_core::element::desktop::title_bar;
use nsh_core::element::{ImageSrc, Painter};
use nsh_core::prelude::*;
use sye_core::config::ThemeMode;

use crate::theme::{SUPPORT_URL, colors, motion, radii, theme_mode_label};

include!(concat!(env!("OUT_DIR"), "/app_icon.rs"));

/// Shared geometry constants for spring-animated sliding indicators.
const SIDEBAR_ITEM_HEIGHT: f32 = 38.0;
const SIDEBAR_ITEM_GAP: f32 = 4.0;

/// 24x24 Lucide-style stroke SVG paths rendered via `Painter::stroke_svg` at 16x16.
const SUN_ICON_SVG: &str = "\
M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8Z \
M12 2v2 M12 20v2 \
M4.93 4.93l1.41 1.41 M17.66 17.66l1.41 1.41 \
M2 12h2 M20 12h2 \
M6.34 17.66l-1.41 1.41 M19.07 4.93l-1.41 1.41";
const MOON_ICON_SVG: &str = "M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z";
const MONITOR_ICON_SVG: &str = "\
M4 4h16a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z \
M8 21h8 M12 17v4";

const APP_ICON_RGBA_LEN: usize = (APP_ICON_SIZE as usize) * (APP_ICON_SIZE as usize) * 4;

fn app_icon_rgba() -> Arc<[u8]> {
    static PIXELS: OnceLock<Arc<[u8]>> = OnceLock::new();
    Arc::clone(PIXELS.get_or_init(|| {
        let mut out = Vec::with_capacity(APP_ICON_RGBA_LEN);
        for &[count, r, g, b, a] in APP_ICON_RLE.as_chunks::<5>().0 {
            let rgba = [r, g, b, a];
            for _ in 0..usize::from(count) {
                out.extend_from_slice(&rgba);
            }
        }
        out.resize(APP_ICON_RGBA_LEN, 0);
        Arc::from(out)
    }))
}

/// Renders the embedded Safe Your Eyes squircle app icon at `size_px`.
pub(crate) fn app_icon(size_px: f32) -> impl IntoElement {
    let src = ImageSrc::rgba(APP_ICON_SIZE, APP_ICON_SIZE, app_icon_rgba())
        .size(Size::new(px(size_px), px(size_px)));
    image(src).w(px(size_px)).h(px(size_px)).flex_shrink_0()
}

/// Pastel KPI card color variant matching `tunglt1810/cozy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KpiTone {
    Sage,
    Peach,
    Butter,
    Mist,
}

impl KpiTone {
    const fn bg(self) -> AdaptiveColor {
        match self {
            Self::Sage => crate::theme::kpi::SAGE_BG,
            Self::Peach => crate::theme::kpi::PEACH_BG,
            Self::Butter => crate::theme::kpi::BUTTER_BG,
            Self::Mist => crate::theme::kpi::MIST_BG,
        }
    }

    const fn accent(self) -> AdaptiveColor {
        match self {
            Self::Sage => crate::theme::kpi::SAGE_ACCENT,
            Self::Peach => crate::theme::kpi::PEACH_ACCENT,
            Self::Butter => crate::theme::kpi::BUTTER_ACCENT,
            Self::Mist => crate::theme::kpi::MIST_ACCENT,
        }
    }
}

/// Page H1 heading paired with a Cozy pastel highlighter bar sweeping from left (`tunglt1810/cozy`).
pub(crate) fn page_heading(
    title: impl IntoReactive<SharedString>,
    highlight_scale: &AnimatedValue,
) -> impl IntoElement {
    view()
        .gap_1()
        .child(
            text(title)
                .text_2xl()
                .font_weight(FontWeight::BOLD)
                .text_color(colors::TEXT_PRIMARY),
        )
        .child(
            view()
                .w(px(48.0))
                .h(px(4.0))
                .rounded(radii::PILL)
                .bg(colors::HEADING_HIGHLIGHT)
                .origin_left()
                .bind_transform(nsh_core::TransformKind::ScaleX, highlight_scale),
        )
}

/// Top-right Cozy theme toggle pill with crisp vector SVG icon and center-origin tracking.
pub(crate) fn theme_toggle_button(
    mode: impl Fn() -> ThemeMode + Copy + 'static,
    measured_center: Signal<Option<[f32; 2]>>,
    on_press: impl Fn(Option<[f32; 2]>) + 'static,
) -> impl IntoElement {
    let btn_ref = element_ref();
    let (bx1, by1, bx2, by2) = motion::COZY_STANDARD_BEZIER;

    effect(move || {
        // Re-measure when native titlebar insets or window dimensions shift parent layout.
        let _ = title_bar();
        let _ = window();
        spawn(async move {
            if let Some(m) = btn_ref.measure().await {
                let cx = m.page.x.0 + m.frame.size.width.0 * 0.5;
                let cy = m.page.y.0 + m.frame.size.height.0 * 0.5;
                measured_center.set(Some([cx, cy]));
            }
        });
    });

    let icon = canvas(move || {
        let svg = match mode() {
            ThemeMode::Light => SUN_ICON_SVG,
            ThemeMode::Dark => MOON_ICON_SVG,
            ThemeMode::System => MONITOR_ICON_SVG,
        };
        let stroke = color(colors::TEXT_PRIMARY);
        Painter::new()
            .scale(16.0_f32 / 24.0_f32, 16.0_f32 / 24.0_f32)
            .stroke_color(stroke)
            .line_width(2.0_f32)
            .line_cap(LineCap::Round)
            .line_join(LineJoin::Round)
            .stroke_svg(svg)
    })
    .w(px(16.0))
    .h(px(16.0))
    .flex_shrink_0();

    view()
        .element_ref(&btn_ref)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .window_drag(false)
        .on_layout(move |_: &Bounds<Points>| {
            spawn(async move {
                if let Some(m) = btn_ref.measure().await {
                    let cx = m.page.x.0 + m.frame.size.width.0 * 0.5;
                    let cy = m.page.y.0 + m.frame.size.height.0 * 0.5;
                    measured_center.set(Some([cx, cy]));
                }
            });
        })
        .on_press(move |ev: &PressEvent| {
            let p = ev.page_position;
            let live_press = (p.x.0 > 0.0 || p.y.0 > 0.0).then_some([p.x.0, p.y.0]);
            let origin = live_press.or_else(|| measured_center.get());
            on_press(origin);
        })
        .h(px(36.0))
        .px_3()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_2()
        .rounded(radii::PILL)
        .bg(colors::BACKGROUND_SURFACE)
        .border_1()
        .border_color(colors::BORDER_EMPHASIZED)
        .transition()
        .duration(motion::CONTROL_MS)
        .ease_bezier(bx1, by1, bx2, by2)
        .hover(|s| s.bg(colors::OVERLAY_HOVER))
        .active(|s| s.scale(motion::PRESS_SCALE).opacity(0.88))
        .child(icon)
        .child(
            text(move || theme_mode_label(mode()))
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT_PRIMARY),
        )
}

/// Section heading with optional supporting caption inside the unified Form Card.
pub(crate) fn section_header(title: &'static str, caption: &'static str) -> impl IntoElement {
    view()
        .gap_1()
        .child(
            text(title)
                .text_base()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT_PRIMARY),
        )
        .child(text(caption).text_xs().text_color(colors::TEXT_SECONDARY))
}

/// Hairline horizontal divider (`1px`) separating sections inside a Form Card (Astryx Divider).
pub(crate) fn section_divider() -> impl IntoElement {
    view().h(px(1.0)).bg(colors::BORDER)
}

/// Cozy primary pill button (warm coral fill + WCAG AAA dark espresso text).
pub(crate) fn primary_button(
    label: impl IntoReactive<SharedString>,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .on_press(on_press)
        .h(px(36.0))
        .px_4()
        .items_center()
        .justify_center()
        .rounded(radii::PILL)
        .bg(colors::ACCENT)
        .transition()
        .duration(motion::CONTROL_MS)
        .ease_out()
        .hover(|s| s.opacity(0.92))
        .active(|s| s.scale(motion::PRESS_SCALE).opacity(0.88))
        .disabled_style(|s| s.opacity(0.45))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::ON_ACCENT),
        )
}

/// Cozy secondary pill button (`bg-surface` fill + `border-emphasized`, weight 600 per Cozy v0.2.0).
pub(crate) fn secondary_button(
    label: impl IntoReactive<SharedString>,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .on_press(on_press)
        .h(px(36.0))
        .px_4()
        .items_center()
        .justify_center()
        .rounded(radii::PILL)
        .bg(colors::BACKGROUND_SURFACE)
        .border_1()
        .border_color(colors::BORDER_EMPHASIZED)
        .transition()
        .duration(motion::CONTROL_MS)
        .ease_out()
        .hover(|s| s.bg(colors::OVERLAY_HOVER))
        .active(|s| s.scale(motion::PRESS_SCALE).opacity(0.88))
        .disabled_style(|s| s.opacity(0.45))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT_PRIMARY),
        )
}

/// Cozy ghost pill button for tertiary/cancel actions (weight 600 per Cozy v0.2.0).
pub(crate) fn ghost_button(
    label: impl IntoReactive<SharedString>,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .on_press(on_press)
        .h(px(36.0))
        .px_3()
        .items_center()
        .justify_center()
        .rounded(radii::PILL)
        .transition()
        .duration(motion::CONTROL_MS)
        .ease_out()
        .hover(|s| s.bg(colors::OVERLAY_HOVER))
        .active(|s| s.scale(motion::PRESS_SCALE).opacity(0.84))
        .disabled_style(|s| s.opacity(0.45))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT_SECONDARY),
        )
}

/// Cozy destructive pill button using measured `error-muted` / `on-error-muted` pair from `tunglt1810/cozy`.
pub(crate) fn destructive_button(
    label: impl IntoReactive<SharedString>,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .on_press(on_press)
        .h(px(30.0))
        .px_3()
        .items_center()
        .justify_center()
        .rounded(radii::PILL)
        .bg(colors::ERROR_SOFT)
        .transition()
        .duration(motion::CONTROL_MS)
        .ease_out()
        .hover(|s| s.opacity(0.9))
        .active(|s| s.scale(motion::PRESS_SCALE).opacity(0.82))
        .child(
            text(label)
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::ON_ERROR_MUTED),
        )
}

/// Pastel KPI summary card with concentric corner radius, accent dot, label, and value.
pub(crate) fn pastel_kpi_card(
    tone: KpiTone,
    label: &'static str,
    value: impl IntoReactive<SharedString>,
) -> impl IntoElement {
    view()
        .flex_1()
        .rounded(radii::KPI)
        .bg(tone.bg())
        .border_1()
        .border_color(colors::BORDER)
        .px_4()
        .py_3()
        .gap_1()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    view()
                        .w(px(8.0))
                        .h(px(8.0))
                        .rounded(radii::PILL)
                        .bg(tone.accent()),
                )
                .child(
                    text(label)
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors::TEXT_SECONDARY),
                ),
        )
        .child(
            text(value)
                .text_lg()
                .font_weight(FontWeight::BOLD)
                .text_color(colors::TEXT_PRIMARY),
        )
}

/// Cozy toggle switch row aligned to the shared 36px control rhythm (Astryx control-rhythm DR1).
pub(crate) fn cozy_switch(
    label: &'static str,
    checked: impl Fn() -> bool + 'static,
    on_change: impl Fn(bool) + 'static,
) -> impl IntoElement {
    view()
        .flex_row()
        .h(px(36.0))
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors::TEXT_PRIMARY),
        )
        .child(
            switch()
                .checked(checked)
                .on_change(move |val: &bool| on_change(*val))
                .accessibility_label(label)
                .track_style(|s| {
                    s.w(px(42.0))
                        .h(px(24.0))
                        .flex_row()
                        .items_center()
                        .px(px(2.0))
                        .rounded(radii::PILL)
                        .bg(colors::TRACK)
                        .border_1()
                        .border_color(colors::BORDER_EMPHASIZED)
                        .transition_colors()
                        .duration(motion::CONTROL_MS)
                        .ease_out()
                        .checked(|c| c.bg(colors::ACCENT).border_color(colors::ACCENT))
                })
                .thumb_style(|s| {
                    s.w(px(18.0))
                        .h(px(18.0))
                        .rounded(radii::PILL)
                        .bg(colors::SWITCH_KNOB)
                        .translate_x(px(0.0))
                        .transition_transform()
                        .ease_spring(motion::COZY_SPRING)
                        .checked(|c| c.translate_x(px(18.0)))
                }),
        )
}

/// Cozy styled single-line text input (`8px` radius, `36px` height, `bg-surface` per Cozy v0.2.0).
pub(crate) fn cozy_input(
    placeholder: &'static str,
    value: impl Fn() -> String + 'static,
    on_change: impl Fn(&str) + 'static,
) -> impl IntoElement {
    input()
        .value(value)
        .on_change(on_change)
        .placeholder(placeholder)
        .placeholder_color(|| color(colors::TEXT_DISABLED))
        .caret_color(|| color(colors::ACCENT))
        .h(px(36.0))
        .px_3()
        .text_sm()
        .text_color(colors::TEXT_PRIMARY)
        .rounded(radii::ELEMENT)
        .bg(colors::BACKGROUND_SURFACE)
        .border_1()
        .border_color(colors::BORDER_EMPHASIZED)
        .transition_colors()
        .duration(motion::CONTROL_MS)
        .ease_out()
        .hover(|s| s.border_color(colors::TEXT_DISABLED))
        .focus(|s| s.border_color(colors::FOCUS_RING))
}

/// Spring-animated sliding background indicator for the Floating Sage Sidebar (`tunglt1810/cozy`).
pub(crate) fn sidebar_nav_indicator(active_index: impl Fn() -> u8 + 'static) -> impl IntoElement {
    let step = SIDEBAR_ITEM_HEIGHT + SIDEBAR_ITEM_GAP;
    view()
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .right(px(0.0))
        .z_index(0)
        .h(px(SIDEBAR_ITEM_HEIGHT))
        .rounded(radii::NAV_ITEM)
        .bg(colors::SIDEBAR_ACTIVE)
        .translate_y(move || px(f32::from(active_index()) * step))
        .transition_transform()
        .ease_spring(motion::COZY_SPRING)
}

/// Sidebar navigation button layered above `sidebar_nav_indicator`.
pub(crate) fn sidebar_nav_item(
    label: &'static str,
    selected: impl Fn() -> bool + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .on_press(on_press)
        .selected(selected)
        .z_index(1)
        .h(px(SIDEBAR_ITEM_HEIGHT))
        .px_3()
        .items_start()
        .justify_center()
        .rounded(radii::NAV_ITEM)
        .transition_colors()
        .duration(motion::CONTROL_MS)
        .ease_out()
        .hover(|s| s.bg(colors::OVERLAY_HOVER))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::SIDEBAR_TEXT),
        )
}

/// Warm status banner using Cozy v0.2.0 contrast-checked fill, text, and border tokens.
pub(crate) fn status_banner(
    is_error: bool,
    message: impl IntoReactive<SharedString>,
) -> impl IntoElement {
    let (bg, fg, border) = if is_error {
        (colors::ERROR_SOFT, colors::ON_ERROR_MUTED, colors::ERROR)
    } else {
        (
            colors::WARNING_SOFT,
            colors::ON_WARNING_MUTED,
            colors::WARNING,
        )
    };
    view()
        .rounded(radii::ELEMENT)
        .bg(bg)
        .border_1()
        .border_color(border)
        .px_3()
        .py_2()
        .child(
            text(message)
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg),
        )
}

/// Direct Buy Me a Coffee link styled as a Cozy coral pill CTA.
pub(crate) fn support_link(label: &'static str) -> impl IntoElement {
    link()
        .href(SUPPORT_URL)
        .h(px(36.0))
        .px_4()
        .items_center()
        .justify_center()
        .rounded(radii::PILL)
        .bg(colors::ACCENT)
        .transition()
        .duration(motion::CONTROL_MS)
        .ease_out()
        .hover(|s| s.opacity(0.92))
        .active(|s| s.scale(motion::PRESS_SCALE).opacity(0.88))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::ON_ACCENT),
        )
}

/// Encodes `SUPPORT_URL` into a cached square boolean QR matrix (`true` = dark module).
pub(crate) fn build_support_qr_matrix() -> Option<&'static (usize, Box<[bool]>)> {
    static MATRIX: OnceLock<Option<(usize, Box<[bool]>)>> = OnceLock::new();
    MATRIX
        .get_or_init(|| {
            let code = qrcode::QrCode::new(SUPPORT_URL.as_bytes()).ok()?;
            let width = code.width();
            let modules: Box<[bool]> = code
                .into_colors()
                .into_iter()
                .map(|c| c == qrcode::Color::Dark)
                .collect();
            Some((width, modules))
        })
        .as_ref()
}

/// Scannable Buy Me a Coffee QR code component with Cozy warm cream quiet zone and espresso modules.
pub(crate) fn support_qr_card() -> impl IntoElement {
    let qr_bg = rgb(0xFFFEF7);
    let qr_fg = rgb(0x261101);
    let module_px = 3.5_f32;

    let mut grid = view().gap(px(0.0));
    if let Some((width, modules)) = build_support_qr_matrix() {
        for row_modules in modules.chunks_exact(*width) {
            let mut row = view().flex_row().gap(px(0.0));
            for &is_dark in row_modules {
                row = row.child(view().w(px(module_px)).h(px(module_px)).bg(if is_dark {
                    qr_fg
                } else {
                    qr_bg
                }));
            }
            grid = grid.child(row);
        }
    }

    view()
        .items_center()
        .gap_2()
        .flex_shrink_0()
        .child(
            view()
                .p_3()
                .rounded(radii::CONTAINER)
                .bg(qr_bg)
                .border_1()
                .border_color(colors::BORDER_EMPHASIZED)
                .child(grid),
        )
        .child(
            text("Scan to Buy Me a Coffee")
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors::TEXT_SECONDARY),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_qr_matrix_encodes_buy_me_a_coffee_url() {
        let matrix = build_support_qr_matrix();
        assert!(matrix.is_some());
        if let Some((width, modules)) = matrix {
            assert!(
                *width >= 21,
                "QR code width must be at least Version 1 (21)"
            );
            assert_eq!(modules.len(), *width * *width);
            // Top-left finder pattern corner (0, 0) is always dark in standard QR codes.
            assert_eq!(modules.first().copied(), Some(true));
        }
    }

    #[test]
    fn embedded_app_icon_decodes_to_exact_rgba_buffer() {
        let pixels = app_icon_rgba();
        assert_eq!(pixels.len(), APP_ICON_RGBA_LEN);
        assert!(pixels.as_chunks::<4>().0.iter().any(|&[_, _, _, a]| a > 0));
    }

    #[test]
    fn cozy_switch_moves_thumb_translate_x_when_toggled() -> Result<(), String> {
        use std::sync::Mutex;

        use nsh_core::MountItem;
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        #[component]
        fn SwitchHost() -> impl IntoElement {
            let checked = signal(false);
            view().p_4().child(cozy_switch(
                "Smart Pause on idle",
                move || checked.get(),
                move |next| checked.set(next),
            ))
        }

        let observed_x: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&observed_x);

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(400.0), Points(120.0)),
            ..EngineConfig::new(Root::of::<SwitchHost>())
        })
        .map_err(|e| e.to_string())?;
        app.observe(move |_step, batch| {
            for item in &batch.items {
                if let MountItem::ViewStyle { style, .. } = item {
                    for op in style.transform.ops() {
                        if op.kind == nsh_core::TransformKind::TranslateX
                            && let Ok(mut guard) = sink.lock()
                        {
                            guard.push(op.value);
                        }
                    }
                }
            }
        });

        app.wait_for("Smart Pause on idle")?;
        let nodes = app.screen().layout_nodes();
        let thumb = nodes
            .iter()
            .find(|n| {
                (n.bounds.size.width.0 - 18.0).abs() < 0.01
                    && (n.bounds.size.height.0 - 18.0).abs() < 0.01
            })
            .ok_or_else(|| format!("expected 18x18 switch thumb node, got {nodes:#?}"))?;
        let track = nodes
            .iter()
            .find(|n| n.id == thumb.parent)
            .ok_or_else(|| format!("expected parent track node for thumb, got {nodes:#?}"))?;
        assert!(
            (track.bounds.size.width.0 - 42.0).abs() < 0.01
                && (track.bounds.size.height.0 - 24.0).abs() < 0.01,
            "Expected 42x24 switch track parent, got {:?}",
            track.bounds
        );
        let left_inset = thumb.bounds.origin.x.0 - track.bounds.origin.x.0;
        let top_inset = thumb.bounds.origin.y.0 - track.bounds.origin.y.0;
        let bottom_inset = (track.bounds.origin.y.0 + track.bounds.size.height.0)
            - (thumb.bounds.origin.y.0 + thumb.bounds.size.height.0);
        assert!(
            (left_inset - 3.0).abs() < 0.01
                && (top_inset - 3.0).abs() < 0.01
                && (bottom_inset - 3.0).abs() < 0.01,
            "Expected symmetric 3pt thumb inset inside 42x24 track, got left={left_inset}, top={top_inset}, bottom={bottom_inset}"
        );

        app.press("Smart Pause on idle")?;
        app.settle();
        let on_values = observed_x.lock().map_err(|e| e.to_string())?.clone();
        let on_dx = on_values
            .last()
            .copied()
            .ok_or_else(|| "expected TranslateX on toggle ON".to_owned())?;
        let right_inset_on = (track.bounds.origin.x.0 + track.bounds.size.width.0)
            - (thumb.bounds.origin.x.0 + on_dx + thumb.bounds.size.width.0);
        assert!(
            (on_dx - 18.0).abs() < 0.01 && (right_inset_on - 3.0).abs() < 0.01,
            "Expected switch thumb TranslateX=18.0 and right_inset=3.0 when ON, got dx={on_dx}, right_inset={right_inset_on}"
        );

        app.press("Smart Pause on idle")?;
        app.settle();
        let off_values = observed_x.lock().map_err(|e| e.to_string())?.clone();
        assert_eq!(
            off_values.last().copied(),
            Some(0.0),
            "Expected switch thumb TranslateX to return to 0.0 when toggled OFF, got {off_values:?}"
        );
        Ok(())
    }

    #[test]
    fn theme_toggle_button_reports_fresh_origin_after_title_bar_shift() -> Result<(), String> {
        use std::sync::Mutex;

        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        static LAST_ORIGIN: Mutex<Option<[f32; 2]>> = Mutex::new(None);

        #[component]
        fn ToggleHost() -> impl IntoElement {
            let mode = signal(ThemeMode::System);
            let measured = signal(None::<[f32; 2]>);
            view()
                .p_3()
                .child(view().h(move || title_bar().top))
                .child(theme_toggle_button(
                    move || mode.get(),
                    measured,
                    move |origin| {
                        if let Ok(mut guard) = LAST_ORIGIN.lock() {
                            *guard = origin;
                        }
                    },
                ))
        }

        if let Ok(mut guard) = LAST_ORIGIN.lock() {
            *guard = None;
        }
        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(400.0), Points(200.0)),
            ..EngineConfig::new(Root::of::<ToggleHost>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Theme: System")?;

        app.set_environment("win-titlebar", |env| {
            env.title_bar.top = Points(36.0);
            env.title_bar.right = Points(138.0);
        })?;
        app.settle();
        app.press("Theme: System")?;

        let origin = LAST_ORIGIN
            .lock()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "expected non-None origin on press".to_owned())?;
        // 12pt root padding + 36pt titlebar spacer + ~18pt half button height = ~66pt (not stale ~30pt).
        assert!(
            origin[1] > 55.0,
            "Expected fresh origin y > 55.0 after 36pt titlebar shift, got {origin:?}"
        );
        Ok(())
    }
}
