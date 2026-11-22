//! Reusable Cozy styled UI primitives built on `nsh_base`, `theme`, and Astryx layout rules.

use std::sync::{Arc, OnceLock};

use nsh_base::{button, input, link, switch};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;

use crate::theme::{SUPPORT_URL, colors, radii};

include!(concat!(env!("OUT_DIR"), "/app_icon.rs"));

fn app_icon_rgba() -> Arc<[u8]> {
    static PIXELS: OnceLock<Arc<[u8]>> = OnceLock::new();
    Arc::clone(PIXELS.get_or_init(|| {
        let expected = (APP_ICON_SIZE * APP_ICON_SIZE * 4) as usize;
        let mut out = Vec::with_capacity(expected);
        for &[count, r, g, b, a] in APP_ICON_RLE.as_chunks::<5>().0 {
            let rgba = [r, g, b, a];
            for _ in 0..usize::from(count) {
                out.extend_from_slice(&rgba);
            }
        }
        out.resize(expected, 0);
        Arc::from(out)
    }))
}

/// Renders the embedded Safe Your Eyes squircle app icon at `size_px`.
pub(crate) fn app_icon(size_px: f32) -> impl IntoElement {
    let src = ImageSrc::rgba(APP_ICON_SIZE, APP_ICON_SIZE, app_icon_rgba())
        .size(Size::new(px(size_px), px(size_px)));
    image(src).w(px(size_px)).h(px(size_px)).flex_shrink_0()
}

/// Pastel KPI card color variant matching `DESIGN.md` §4.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KpiTone {
    Sage,
    Peach,
    Butter,
    Mist,
}

impl KpiTone {
    fn bg(self) -> AdaptiveColor {
        match self {
            Self::Sage => crate::theme::kpi::SAGE_BG,
            Self::Peach => crate::theme::kpi::PEACH_BG,
            Self::Butter => crate::theme::kpi::BUTTER_BG,
            Self::Mist => crate::theme::kpi::MIST_BG,
        }
    }

    fn accent(self) -> AdaptiveColor {
        match self {
            Self::Sage => crate::theme::kpi::SAGE_ACCENT,
            Self::Peach => crate::theme::kpi::PEACH_ACCENT,
            Self::Butter => crate::theme::kpi::BUTTER_ACCENT,
            Self::Mist => crate::theme::kpi::MIST_ACCENT,
        }
    }
}

/// Page H1 heading paired with a subtle Cozy pastel highlighter bar (`DESIGN.md` §5.2).
pub(crate) fn page_heading(
    title: impl IntoReactive<SharedString>,
    subtitle: impl IntoReactive<SharedString>,
) -> impl IntoElement {
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
                .bg(colors::HEADING_HIGHLIGHT),
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
        .hover(|s| s.opacity(0.92))
        .active(|s| s.opacity(0.84))
        .disabled_style(|s| s.opacity(0.45))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::ON_ACCENT),
        )
}

/// Cozy secondary pill button (warm muted surface + emphasized border).
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
        .bg(colors::BACKGROUND_MUTED)
        .border_1()
        .border_color(colors::BORDER_EMPHASIZED)
        .hover(|s| s.opacity(0.9))
        .active(|s| s.opacity(0.8))
        .disabled_style(|s| s.opacity(0.45))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors::TEXT_PRIMARY),
        )
}

/// Cozy ghost pill button for tertiary/cancel actions.
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
        .hover(|s| s.bg(colors::BACKGROUND_MUTED))
        .active(|s| s.opacity(0.8))
        .disabled_style(|s| s.opacity(0.45))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors::TEXT_SECONDARY),
        )
}

/// Cozy destructive pill button using WCAG AA `--color-text-error` (`DESIGN.md` §4.1).
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
        .hover(|s| s.opacity(0.88))
        .active(|s| s.opacity(0.76))
        .child(
            text(label)
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT_ERROR),
        )
}

/// Astryx-style SegmentedControl pill track (`DESIGN.md` §7.1).
pub(crate) fn segmented_track() -> View {
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .p_1()
        .rounded(radii::PILL)
        .bg(colors::BACKGROUND_MUTED)
        .border_1()
        .border_color(colors::BORDER)
}

/// Segment option inside `segmented_track()`, highlighting active selection with surface elevation.
pub(crate) fn segmented_option(
    label: &'static str,
    selected: impl Fn() -> bool + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .on_press(on_press)
        .selected(selected)
        .h(px(32.0))
        .px_4()
        .items_center()
        .justify_center()
        .rounded(radii::PILL)
        .hover(|s| s.bg(colors::SEGMENT_ACTIVE).opacity(0.75))
        .selected_style(|s| {
            s.bg(colors::SEGMENT_ACTIVE)
                .border_1()
                .border_color(colors::BORDER_EMPHASIZED)
        })
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT_PRIMARY),
        )
}

/// Pastel KPI summary card (`20px` radius per `DESIGN.md` §1) with accent dot, label, and value.
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
                        .rounded(radii::PILL)
                        .bg(colors::BACKGROUND_MUTED)
                        .border_1()
                        .border_color(colors::BORDER_EMPHASIZED)
                        .checked(|c| c.bg(colors::ACCENT).border_color(colors::ACCENT))
                })
                .thumb_style(|s| {
                    s.w(px(18.0))
                        .h(px(18.0))
                        .rounded(radii::PILL)
                        .bg(colors::SWITCH_KNOB)
                }),
        )
}

/// Cozy styled single-line text input (`12px` radius, `36px` control height).
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
        .bg(colors::BACKGROUND_MUTED)
        .border_1()
        .border_color(colors::BORDER_EMPHASIZED)
        .focus(|s| s.border_color(colors::ACCENT))
}

/// Sidebar navigation pill inside the Floating Sage Sidebar (`12px` radius).
pub(crate) fn sidebar_nav_item(
    label: &'static str,
    selected: impl Fn() -> bool + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .on_press(on_press)
        .selected(selected)
        .h(px(38.0))
        .px_3()
        .items_start()
        .justify_center()
        .rounded(radii::ELEMENT)
        .hover(|s| s.bg(colors::SIDEBAR_ACTIVE).opacity(0.8))
        .selected_style(|s| s.bg(colors::SIDEBAR_ACTIVE))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT_PRIMARY),
        )
}

/// Warm status banner using WCAG AA `--color-text-{warning,error}` (`DESIGN.md` §4.1).
pub(crate) fn status_banner(
    is_error: bool,
    message: impl IntoReactive<SharedString>,
) -> impl IntoElement {
    let (bg, fg) = if is_error {
        (colors::ERROR_SOFT, colors::TEXT_ERROR)
    } else {
        (colors::WARNING_SOFT, colors::TEXT_WARNING)
    };
    view()
        .rounded(radii::ELEMENT)
        .bg(bg)
        .border_1()
        .border_color(colors::BORDER)
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
        .hover(|s| s.opacity(0.92))
        .active(|s| s.opacity(0.84))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::ON_ACCENT),
        )
}

/// Encodes `SUPPORT_URL` into a square boolean QR matrix (`true` = dark module).
pub(crate) fn build_support_qr_matrix() -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::new(SUPPORT_URL.as_bytes()).ok()?;
    let width = code.width();
    let modules = code
        .to_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    Some((width, modules))
}

/// Scannable Buy Me a Coffee QR code component with Cozy warm cream quiet zone and espresso modules.
pub(crate) fn support_qr_card() -> impl IntoElement {
    let qr_bg = rgb(0xFFFEF7);
    let qr_fg = rgb(0x261101);
    let module_px = 3.5_f32;

    let mut grid = view().gap(px(0.0));
    if let Some((width, modules)) = build_support_qr_matrix() {
        for row_idx in 0..width {
            let mut row = view().flex_row().gap(px(0.0));
            for col_idx in 0..width {
                let is_dark = modules
                    .get(row_idx * width + col_idx)
                    .copied()
                    .unwrap_or(false);
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
            assert!(width >= 21, "QR code width must be at least Version 1 (21)");
            assert_eq!(modules.len(), width * width);
            // Top-left finder pattern corner (0, 0) is always dark in standard QR codes.
            assert_eq!(modules.first().copied(), Some(true));
        }
    }

    #[test]
    fn embedded_app_icon_decodes_to_exact_rgba_buffer() {
        let pixels = app_icon_rgba();
        assert_eq!(pixels.len(), (APP_ICON_SIZE * APP_ICON_SIZE * 4) as usize);
        assert!(pixels.as_chunks::<4>().0.iter().any(|&[_, _, _, a]| a > 0));
    }
}
