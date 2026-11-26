use std::sync::{Arc, OnceLock};

use nsh_base::{button, input, link, switch};
use nsh_core::canvas::{LineCap, LineJoin};
use nsh_core::element::{ImageSrc, Painter, Timing};
use nsh_core::prelude::*;
use sye_core::config::ThemeMode;

use crate::theme::{SUPPORT_URL, colors, motion, radii, theme_mode_label};

include!(concat!(env!("OUT_DIR"), "/app_icon.rs"));

const SIDEBAR_ITEM_HEIGHT: f32 = 38.0;
const SIDEBAR_ITEM_GAP: f32 = 4.0;

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

pub(crate) fn app_icon(size_px: f32) -> impl IntoElement {
    let src = ImageSrc::rgba(APP_ICON_SIZE, APP_ICON_SIZE, app_icon_rgba())
        .size(Size::new(px(size_px), px(size_px)));
    image(src).w(px(size_px)).h(px(size_px)).flex_shrink_0()
}

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

pub(crate) fn page_heading(
    title: impl IntoReactive<SharedString>,
    highlight_scale: &AnimatedValue,
) -> impl IntoElement {
    let (bx1, by1, bx2, by2) = motion::COZY_STANDARD_BEZIER;
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
                .transition_colors()
                .duration(motion::THEME_COLOR_MS)
                .ease_bezier(bx1, by1, bx2, by2)
                .origin_left()
                .bind_transform(nsh_core::TransformKind::ScaleX, highlight_scale),
        )
}

pub(crate) fn stagger_item(
    index: usize,
    animate: bool,
    child: impl IntoElement,
) -> impl IntoElement {
    let container = view().child(child);
    if !animate || reduce_motion() {
        return container;
    }
    let delay_ms = motion::row_stagger_delay_ms(index);
    let timing = Timing {
        duration_ms: motion::ROW_ENTER_MS,
        easing: motion::COZY_EASE,
        delay_ms,
        frame_rate: None,
    };
    let opacity = animated_value(0.0);
    let y = animated_value(motion::ROW_ENTER_Y);
    opacity.timing_with(1.0, timing);
    y.timing_with(0.0, timing);
    container.bind_opacity(&opacity).bind_translate_y(&y)
}

pub(crate) fn theme_toggle_button(
    mode: impl Fn() -> ThemeMode + Copy + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    let (bx1, by1, bx2, by2) = motion::COZY_STANDARD_BEZIER;

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

    view().window_drag(false).child(
        button()
            .on_press(on_press)
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
            .duration(motion::THEME_COLOR_MS)
            .ease_bezier(bx1, by1, bx2, by2)
            .hover(|s| s.bg(colors::OVERLAY_HOVER))
            .active(|s| s.scale(motion::PRESS_SCALE).opacity(0.88))
            .child(icon)
            .child(
                text(move || theme_mode_label(mode()))
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors::TEXT_PRIMARY),
            ),
    )
}

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

pub(crate) fn section_divider() -> impl IntoElement {
    let (bx1, by1, bx2, by2) = motion::COZY_STANDARD_BEZIER;
    view()
        .h(px(1.0))
        .bg(colors::BORDER)
        .transition_colors()
        .duration(motion::THEME_COLOR_MS)
        .ease_bezier(bx1, by1, bx2, by2)
}

pub(crate) fn primary_button_with_disabled(
    label: impl IntoReactive<SharedString>,
    disabled: impl Fn() -> bool + Copy + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .disabled(disabled)
        .on_press(move || {
            if !disabled() {
                on_press();
            }
        })
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
        .disabled_style(|s| {
            s.bg(colors::BACKGROUND_MUTED)
                .border_1()
                .border_color(colors::BORDER)
                .opacity(0.55)
        })
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || {
                    if disabled() {
                        colors::TEXT_SECONDARY
                    } else {
                        colors::ON_ACCENT
                    }
                }),
        )
}

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

pub(crate) fn pastel_kpi_card(
    tone: KpiTone,
    label: &'static str,
    value: impl IntoReactive<SharedString>,
) -> impl IntoElement {
    let (bx1, by1, bx2, by2) = motion::COZY_STANDARD_BEZIER;
    view()
        .flex_1()
        .rounded(radii::KPI)
        .bg(tone.bg())
        .border_1()
        .border_color(colors::BORDER)
        .transition_colors()
        .duration(motion::THEME_COLOR_MS)
        .ease_bezier(bx1, by1, bx2, by2)
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

pub(crate) fn cozy_switch(
    label: &'static str,
    checked: impl Fn() -> bool + 'static,
    on_change: impl Fn(bool) + 'static,
) -> impl IntoElement {
    cozy_switch_with_disabled(label, checked, || false, on_change)
}

pub(crate) fn cozy_switch_with_disabled(
    label: &'static str,
    checked: impl Fn() -> bool + 'static,
    disabled: impl Fn() -> bool + Copy + 'static,
    on_change: impl Fn(bool) + 'static,
) -> impl IntoElement {
    view()
        .flex_row()
        .h(px(36.0))
        .items_center()
        .justify_between()
        .gap_3()
        .opacity(move || if disabled() { 0.45_f32 } else { 1.0_f32 })
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors::TEXT_PRIMARY),
        )
        .child(
            switch()
                .checked(checked)
                .disabled(disabled)
                .on_change(move |val: &bool| {
                    if !disabled() {
                        on_change(*val);
                    }
                })
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

pub(crate) fn cozy_input(
    placeholder: &'static str,
    value: impl Fn() -> String + 'static,
    on_change: impl Fn(&str) + 'static,
) -> impl IntoElement {
    cozy_input_with_disabled(placeholder, value, || false, on_change)
}

pub(crate) fn cozy_input_with_disabled(
    placeholder: &'static str,
    value: impl Fn() -> String + 'static,
    disabled: impl Fn() -> bool + Copy + 'static,
    on_change: impl Fn(&str) + 'static,
) -> impl IntoElement {
    input()
        .value(value)
        .disabled(disabled)
        .readonly(disabled)
        .on_change(move |raw: &str| {
            if !disabled() {
                on_change(raw);
            }
        })
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
        .disabled_style(|s| s.opacity(0.45))
}

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
        .transition()
        .ease_spring(motion::COZY_SPRING)
}

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
    fn theme_toggle_button_cycles_mode_on_press() -> Result<(), String> {
        use nsh_engine::{EngineConfig, Points, Root, Size};
        use nsh_testing::Headless;

        #[component]
        fn ToggleHost() -> impl IntoElement {
            let mode = signal(ThemeMode::System);
            view().p_3().child(theme_toggle_button(
                move || mode.get(),
                move || mode.update(|m| *m = crate::theme::next_theme_mode(*m)),
            ))
        }

        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(Points(400.0), Points(200.0)),
            ..EngineConfig::new(Root::of::<ToggleHost>())
        })
        .map_err(|e| e.to_string())?;
        app.wait_for("Theme: System")?;

        app.press("Theme: System")?;
        app.wait_for("Theme: Light")?;
        app.press("Theme: Light")?;
        app.wait_for("Theme: Dark")?;
        app.press("Theme: Dark")?;
        app.wait_for("Theme: System")?;
        Ok(())
    }
}
