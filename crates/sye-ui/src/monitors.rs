//! Monitor geometry detection and multi-monitor viewport planning for the break overlay.

/// Screen geometry of a single display monitor in logical/virtual desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub is_primary: bool,
}

impl MonitorRect {
    const fn is_valid(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }
}

/// Planned root (primary) and secondary overlay viewports derived from detected monitors.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct OverlayViewportPlan {
    pub primary: Option<MonitorRect>,
    pub secondaries: Vec<MonitorRect>,
}

/// Filters invalid monitor rectangles and assigns the primary and secondary overlay viewports.
#[must_use]
pub fn plan_overlay_viewports(monitors: &[MonitorRect]) -> OverlayViewportPlan {
    let mut valid: Vec<MonitorRect> = monitors
        .iter()
        .copied()
        .filter(MonitorRect::is_valid)
        .collect();
    if valid.is_empty() {
        return OverlayViewportPlan::default();
    }

    let primary_idx = valid.iter().position(|m| m.is_primary).unwrap_or(0);
    let primary = Some(valid.remove(primary_idx));

    OverlayViewportPlan {
        primary,
        secondaries: valid,
    }
}

/// Parses `xrandr --listmonitors` stdout into a list of valid `MonitorRect` entries.
#[must_use]
pub fn parse_xrandr_monitors(stdout: &str) -> Vec<MonitorRect> {
    let mut monitors = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Monitors:") {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(index_tok) = parts.next() else {
            continue;
        };
        if !index_tok.ends_with(':') {
            continue;
        }
        let Some(flags_tok) = parts.next() else {
            continue;
        };
        let Some(geom_tok) = parts.next() else {
            continue;
        };
        let is_primary = flags_tok.contains('*');
        if let Some(rect) = parse_xrandr_geometry(geom_tok, is_primary)
            && rect.is_valid()
        {
            monitors.push(rect);
        }
    }
    monitors
}

fn parse_xrandr_geometry(geom: &str, is_primary: bool) -> Option<MonitorRect> {
    let (w_raw, rest) = geom.split_once('x')?;
    let width = parse_dimension_px(w_raw)?;

    let offset_start = rest.find(['+', '-'])?;
    let h_raw = &rest[..offset_start];
    let height = parse_dimension_px(h_raw)?;

    let offsets = &rest[offset_start..];
    let second_sign_rel = offsets[1..].find(['+', '-'])?;
    let second_sign_idx = 1 + second_sign_rel;

    let x_str = &offsets[..second_sign_idx];
    let y_str = &offsets[second_sign_idx..];

    let x: f32 = x_str.parse().ok()?;
    let y: f32 = y_str.parse().ok()?;

    Some(MonitorRect {
        x,
        y,
        width,
        height,
        is_primary,
    })
}

fn parse_dimension_px(raw: &str) -> Option<f32> {
    let px_str = raw.split('/').next()?;
    let val: f32 = px_str.parse().ok()?;
    if val.is_finite() && val > 0.0 {
        Some(val)
    } else {
        None
    }
}

/// Detects active monitors on the current desktop environment.
///
/// On Linux X11 sessions (when `DISPLAY` is set), queries `xrandr --listmonitors`.
/// Returns an empty list on any failure or unsupported environment.
#[must_use]
pub fn detect_monitors() -> Vec<MonitorRect> {
    #[cfg(target_os = "linux")]
    {
        let has_display = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
        if !has_display {
            return Vec::new();
        }
        let output = std::process::Command::new("xrandr")
            .arg("--listmonitors")
            .output();
        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                parse_xrandr_monitors(&stdout)
            }
            _ => Vec::new(),
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_empty_monitors_returns_empty_plan() {
        let plan = plan_overlay_viewports(&[]);
        assert_eq!(plan.primary, None);
        assert!(plan.secondaries.is_empty());
    }

    #[test]
    fn plan_filters_invalid_rects_and_picks_primary() {
        let monitors = [
            MonitorRect {
                x: f32::NAN,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
                is_primary: true,
            },
            MonitorRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 1080.0,
                is_primary: false,
            },
            MonitorRect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
                is_primary: false,
            },
            MonitorRect {
                x: 1920.0,
                y: 0.0,
                width: 2560.0,
                height: 1440.0,
                is_primary: true,
            },
            MonitorRect {
                x: -1280.0,
                y: 100.0,
                width: 1280.0,
                height: 1024.0,
                is_primary: false,
            },
        ];

        let plan = plan_overlay_viewports(&monitors);
        assert_eq!(plan.primary, Some(monitors[3]));
        assert_eq!(plan.secondaries, vec![monitors[2], monitors[4]]);
    }

    #[test]
    fn plan_defaults_primary_to_first_valid_when_none_flagged() {
        let monitors = [
            MonitorRect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
                is_primary: false,
            },
            MonitorRect {
                x: 1920.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
                is_primary: false,
            },
        ];

        let plan = plan_overlay_viewports(&monitors);
        assert_eq!(plan.primary, Some(monitors[0]));
        assert_eq!(plan.secondaries, vec![monitors[1]]);
    }

    #[test]
    fn plan_single_monitor_has_no_secondaries() {
        let m = MonitorRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
            is_primary: true,
        };
        let plan = plan_overlay_viewports(&[m]);
        assert_eq!(plan.primary, Some(m));
        assert!(plan.secondaries.is_empty());
    }

    #[test]
    fn parse_xrandr_monitors_handles_mm_primary_and_negative_offsets() {
        let sample = "\
Monitors: 3
 0: +*DP-1 1920/527x1080/296+0+0  DP-1
 1: +HDMI-1 2560/597x1440/336+1920-180  HDMI-1
 2: +DP-2 1280x1024-1280+56  DP-2
";
        let parsed = parse_xrandr_monitors(sample);
        assert_eq!(
            parsed,
            vec![
                MonitorRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                    is_primary: true,
                },
                MonitorRect {
                    x: 1920.0,
                    y: -180.0,
                    width: 2560.0,
                    height: 1440.0,
                    is_primary: false,
                },
                MonitorRect {
                    x: -1280.0,
                    y: 56.0,
                    width: 1280.0,
                    height: 1024.0,
                    is_primary: false,
                },
            ]
        );
    }

    #[test]
    fn parse_xrandr_monitors_skips_malformed_lines() {
        let sample = "\
Monitors: 2
garbage line
 0: +*DP-1 invalid_geom DP-1
 1: +HDMI-1 0/0x1080/296+0+0 HDMI-1
 2: +eDP-1 1920/344x1080/194+0+0 eDP-1
";
        let parsed = parse_xrandr_monitors(sample);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].width, 1920.0);
        assert_eq!(parsed[0].height, 1080.0);
        assert!(!parsed[0].is_primary);
    }
}
