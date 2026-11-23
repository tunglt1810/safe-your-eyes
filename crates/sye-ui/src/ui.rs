//! Reusable Cozy styled UI primitives built on `nsh_base`, `theme`, and Astryx layout rules.
//!
//! Design system reference: <https://github.com/tunglt1810/cozy>.

use std::sync::{Arc, OnceLock};

use nsh_base::{button, input, link, switch};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;

use crate::theme::{SUPPORT_URL, colors, motion, radii};

include!(concat!(env!("OUT_DIR"), "/app_icon.rs"));

/// Shared geometry constants for spring-animated sliding indicators.
const SIDEBAR_ITEM_HEIGHT: f32 = 38.0;
const SIDEBAR_ITEM_GAP: f32 = 4.0;
const SEGMENT_OPTION_WIDTH: f32 = 80.0;
const SEGMENT_OPTION_HEIGHT: f32 = 32.0;

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
    subtitle: impl IntoReactive<SharedString>,
    highlight_scale: &AnimatedValue,
) -> impl IntoElement {
    let (bx1, by1, bx2, by2) = motion::COZY_BEZIER;
    view()
        .gap_1()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .child(
                    text(title)
                        .text_2xl()
                        .font_weight(FontWeight::BOLD)
                        .text_color(colors::TEXT_PRIMARY),
                )
                .child(
                    view()
                        .px_2()
                        .py_1()
                        .rounded(radii::PILL)
                        .bg(colors::BACKGROUND_MUTED)
                        .border_1()
                        .border_color(colors::BORDER)
                        .transition_colors()
                        .duration(motion::THEME_COLOR_MS)
                        .ease_bezier(bx1, by1, bx2, by2)
                        .child(
                            text(subtitle)
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(colors::TEXT_SECONDARY),
                        ),
                ),
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

/// Astryx-style `SegmentedControl` pill track with spring-animated sliding thumb (`tunglt1810/cozy`).
pub(crate) fn segmented_track(active_index: impl Fn() -> u8 + 'static) -> View {
    let (bx1, by1, bx2, by2) = motion::COZY_BEZIER;
    let sliding_pill = view()
        .absolute()
        .top(px(4.0))
        .left(px(4.0))
        .z_index(0)
        .w(px(SEGMENT_OPTION_WIDTH))
        .h(px(SEGMENT_OPTION_HEIGHT))
        .rounded(radii::PILL)
        .bg(colors::SEGMENT_ACTIVE)
        .border_1()
        .border_color(colors::BORDER_EMPHASIZED)
        .translate_x(move || px(f32::from(active_index()) * SEGMENT_OPTION_WIDTH))
        .transition_transform()
        .ease_spring(motion::COZY_SPRING);

    view()
        .relative()
        .flex_row()
        .items_center()
        .p_1()
        .rounded(radii::PILL)
        .bg(colors::BACKGROUND_MUTED)
        .border_1()
        .border_color(colors::BORDER)
        .transition_colors()
        .duration(motion::THEME_COLOR_MS)
        .ease_bezier(bx1, by1, bx2, by2)
        .child(sliding_pill)
}

/// Fixed-width segment button overlaid above `segmented_track`'s sliding spring pill.
pub(crate) fn segmented_option(
    label: &'static str,
    selected: impl Fn() -> bool + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .on_press(on_press)
        .selected(selected)
        .z_index(1)
        .w(px(SEGMENT_OPTION_WIDTH))
        .h(px(SEGMENT_OPTION_HEIGHT))
        .items_center()
        .justify_center()
        .rounded(radii::PILL)
        .transition_colors()
        .duration(motion::CONTROL_MS)
        .ease_out()
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT_PRIMARY),
        )
}

/// Pastel KPI summary card (`20px` radius per `tunglt1810/cozy`) with accent dot, label, and value.
pub(crate) fn pastel_kpi_card(
    tone: KpiTone,
    label: &'static str,
    value: impl IntoReactive<SharedString>,
) -> impl IntoElement {
    let (bx1, by1, bx2, by2) = motion::COZY_BEZIER;
    view()
        .flex_1()
        .rounded(radii::KPI)
        .bg(tone.bg())
        .border_1()
        .border_color(colors::BORDER)
        .px_4()
        .py_3()
        .gap_1()
        .transition_colors()
        .duration(motion::THEME_COLOR_MS)
        .ease_bezier(bx1, by1, bx2, by2)
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
                        .transition_transform()
                        .ease_spring(motion::COZY_SPRING)
                }),
        )
}

/// Cozy styled single-line text input (`12px` radius, `36px` height, `bg-surface` per Cozy v0.2.0).
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
}
