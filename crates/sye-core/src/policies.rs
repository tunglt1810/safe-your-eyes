use std::time::Duration;

use crate::config::{DoNotDisturbConfig, SkipLimitConfig, SmartPauseConfig};
use crate::scheduler::SchedulerState;

pub const ACTIVE_POSTPONE_SECS: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartPauseDecision {
    None,
    PauseResting,
    ResumeCreditBack { credit: Duration },
    ResumeResetCycle,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SmartPausePolicy {
    idle_paused: bool,
    idle_origin: Option<Duration>,
}

impl SmartPausePolicy {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            idle_paused: false,
            idle_origin: None,
        }
    }

    #[must_use]
    pub const fn is_idle_paused(&self) -> bool {
        self.idle_paused
    }

    pub const fn reset(&mut self) {
        self.idle_paused = false;
        self.idle_origin = None;
    }

    #[must_use]
    pub fn evaluate_idle(
        &mut self,
        cfg: &SmartPauseConfig,
        state: SchedulerState,
        now: Duration,
        current_idle: Duration,
        short_break_interval_secs: u64,
    ) -> SmartPauseDecision {
        if !cfg.enabled {
            self.reset();
            return SmartPauseDecision::None;
        }

        let threshold = Duration::from_secs(cfg.idle_threshold_secs);

        if state == SchedulerState::Waiting && !self.idle_paused {
            if current_idle >= threshold {
                self.idle_paused = true;
                self.idle_origin = Some(now.saturating_sub(current_idle));
                return SmartPauseDecision::PauseResting;
            }
            return SmartPauseDecision::None;
        }

        if state == SchedulerState::Resting && self.idle_paused {
            if current_idle < threshold {
                let total_idle = self
                    .idle_origin
                    .map_or(Duration::ZERO, |origin| now.saturating_sub(origin));
                self.reset();
                if total_idle < Duration::from_secs(short_break_interval_secs) {
                    return SmartPauseDecision::ResumeCreditBack { credit: total_idle };
                }
                return SmartPauseDecision::ResumeResetCycle;
            }
            return SmartPauseDecision::None;
        }

        SmartPauseDecision::None
    }

    #[must_use]
    pub fn should_postpone_if_active(
        cfg: &SmartPauseConfig,
        current_idle: Duration,
    ) -> Option<Duration> {
        let limit = Duration::from_secs(ACTIVE_POSTPONE_SECS);
        if cfg.enabled && cfg.postpone_if_active && current_idle < limit {
            Some(limit)
        } else {
            None
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DndContext {
    pub is_fullscreen: bool,
    pub active_window_class: Option<String>,
    pub active_process_name: Option<String>,
    pub active_executable_path: Option<String>,
    pub on_battery: bool,
    pub system_inhibited: bool,
    pub game_detected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DndReason {
    OnBattery,
    SkipWindow,
    Fullscreen,
    Gaming,
    SystemInhibited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DndDecision {
    Allow,
    Suppress(DndReason),
}

impl DndDecision {
    #[must_use]
    pub const fn should_suppress(self) -> bool {
        matches!(self, Self::Suppress(_))
    }
}

const EXCLUDED_GAME_PROCESSES: &[&str] = &[
    "steam",
    "steamwebhelper",
    "epicgameslauncher",
    "riotclientservices",
    "riotclientux",
    "battle.net",
    "goggalaxy",
    "upc",
    "eadesktop",
    "unity",
    "unity hub",
    "unrealeditor",
    "ue4editor",
    "ue5editor",
    "godot",
];

const GAME_PATH_SEGMENTS: &[&str] = &[
    "/steamapps/common/",
    "/epic games/",
    "/riot games/",
    "/battle.net/",
    "/xboxgames/",
    "/gog galaxy/games/",
    "/gog games/",
    "/ubisoft game launcher/games/",
    "/ea games/",
    "/origin games/",
    "/heroic/games/",
    "/lutris/",
];

const GAME_WINDOW_CLASSES: &[&str] = &[
    "unrealwindow",
    "unitywndclass",
    "riotwindowclass",
    "valve001",
    "cryengine",
    "gamescope",
    "retroarch",
    "lwjgl",
];

const KNOWN_GAME_PROCESSES: &[&str] = &[
    "league of legends",
    "valorant-win64-shipping",
    "genshinimpact",
    "starrail",
    "zenlesszonezero",
    "cs2",
    "dota2",
    "overwatch",
    "r5apex",
    "fortniteclient-win64-shipping",
    "robloxplayerbeta",
    "eldenring",
    "baldursgate3",
    "bg3",
    "bg3_dx11",
    "cyberpunk2077",
    "gta5",
    "hl2",
    "wow",
    "wowclassic",
    "ffxiv_dx11",
    "hearthstone",
    "starcraft ii",
    "diablo iv",
    "minecraft.windows",
];

#[must_use]
pub fn is_gaming_candidate(
    window_class: Option<&str>,
    process_name: Option<&str>,
    executable_path: Option<&str>,
) -> bool {
    let process_stem = process_name
        .map(normalize_process_stem)
        .filter(|s| !s.is_empty());

    if let Some(ref stem) = process_stem
        && EXCLUDED_GAME_PROCESSES.contains(&stem.as_str())
    {
        return false;
    }

    if let Some(path) = executable_path {
        let normalized_path = path.trim().replace('\\', "/").to_lowercase();
        if GAME_PATH_SEGMENTS
            .iter()
            .any(|seg| normalized_path.contains(seg))
        {
            return true;
        }
    }

    if let Some(window_class) = window_class {
        let lower = window_class.trim().to_lowercase();
        if lower.starts_with("steam_app_") || GAME_WINDOW_CLASSES.contains(&lower.as_str()) {
            return true;
        }
    }

    process_stem
        .as_deref()
        .is_some_and(|stem| KNOWN_GAME_PROCESSES.contains(&stem))
}

#[must_use]
pub fn is_gaming_context(ctx: &DndContext) -> bool {
    ctx.game_detected
        || is_gaming_candidate(
            ctx.active_window_class.as_deref(),
            ctx.active_process_name.as_deref(),
            ctx.active_executable_path.as_deref(),
        )
}

fn normalize_process_stem(name: &str) -> String {
    let trimmed = name.trim();
    let lower = trimmed.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_owned()
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DoNotDisturbPolicy;

impl DoNotDisturbPolicy {
    #[must_use]
    pub fn evaluate(cfg: &DoNotDisturbConfig, ctx: &DndContext) -> DndDecision {
        if !cfg.enabled && !cfg.while_gaming {
            return DndDecision::Allow;
        }
        if cfg.enabled && cfg.while_on_battery && ctx.on_battery {
            return DndDecision::Suppress(DndReason::OnBattery);
        }
        let has_window_rules =
            !cfg.skip_break_windows.is_empty() || !cfg.take_break_windows.is_empty();
        if has_window_rules {
            if cfg.enabled && matches_context_window(ctx, &cfg.skip_break_windows) {
                return DndDecision::Suppress(DndReason::SkipWindow);
            }
            if matches_context_window(ctx, &cfg.take_break_windows) {
                return DndDecision::Allow;
            }
        }
        if cfg.enabled && ctx.is_fullscreen {
            return DndDecision::Suppress(DndReason::Fullscreen);
        }
        if cfg.while_gaming && is_gaming_context(ctx) {
            return DndDecision::Suppress(DndReason::Gaming);
        }
        if cfg.enabled && ctx.system_inhibited {
            return DndDecision::Suppress(DndReason::SystemInhibited);
        }
        DndDecision::Allow
    }
}

fn matches_context_window(ctx: &DndContext, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return false;
    }
    [
        ctx.active_window_class.as_deref(),
        ctx.active_process_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|candidate| !candidate.trim().is_empty())
    .any(|candidate| matches_any_window_lower(&candidate.to_lowercase(), patterns))
}

fn matches_any_window_lower(active_lower: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        let trimmed = pattern.trim();
        !trimmed.is_empty() && active_lower.contains(&trimmed.to_lowercase())
    })
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SkipLimitPolicy {
    consecutive_skips: u32,
}

impl SkipLimitPolicy {
    #[must_use]
    pub const fn new(initial_count: u32) -> Self {
        Self {
            consecutive_skips: initial_count,
        }
    }

    #[must_use]
    pub const fn count(&self) -> u32 {
        self.consecutive_skips
    }

    pub const fn record_skip(&mut self) {
        self.consecutive_skips = self.consecutive_skips.saturating_add(1);
    }

    pub const fn record_postpone(&mut self) {
        self.consecutive_skips = self.consecutive_skips.saturating_add(1);
    }

    pub const fn record_completed_break(&mut self) {
        self.consecutive_skips = 0;
    }

    #[must_use]
    pub const fn controls_disabled(&self, cfg: &SkipLimitConfig) -> bool {
        cfg.enabled && self.consecutive_skips >= cfg.max_skips
    }

    #[must_use]
    pub fn status_label(&self, cfg: &SkipLimitConfig) -> Option<String> {
        if !cfg.enabled {
            return None;
        }
        if self.consecutive_skips == 0 && cfg.max_skips > 0 {
            return None;
        }
        Some(format!(
            "Skipped or postponed {}/{} breaks in a row",
            self.consecutive_skips, cfg.max_skips
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_pause_disabled_is_noop() {
        let mut policy = SmartPausePolicy::new();
        let cfg = SmartPauseConfig {
            enabled: false,
            idle_threshold_secs: 5,
            postpone_if_active: true,
        };
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Waiting,
                Duration::from_secs(100),
                Duration::from_secs(30),
                900,
            ),
            SmartPauseDecision::None
        );
        assert_eq!(
            SmartPausePolicy::should_postpone_if_active(&cfg, Duration::ZERO),
            None
        );
    }

    #[test]
    fn smart_pause_enters_resting_and_credits_or_resets() {
        let mut policy = SmartPausePolicy::new();
        assert_eq!(SmartPauseConfig::default().idle_threshold_secs, 300);
        let cfg = SmartPauseConfig {
            idle_threshold_secs: 5,
            ..SmartPauseConfig::default()
        };

        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Waiting,
                Duration::from_secs(100),
                Duration::from_secs(4),
                900,
            ),
            SmartPauseDecision::None
        );
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Waiting,
                Duration::from_secs(105),
                Duration::from_secs(5),
                900,
            ),
            SmartPauseDecision::PauseResting
        );
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Resting,
                Duration::from_secs(220),
                Duration::from_secs(0),
                900,
            ),
            SmartPauseDecision::ResumeCreditBack {
                credit: Duration::from_secs(120),
            }
        );

        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Waiting,
                Duration::from_secs(500),
                Duration::from_secs(5),
                900,
            ),
            SmartPauseDecision::PauseResting
        );
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Resting,
                Duration::from_secs(1395),
                Duration::from_secs(0),
                900,
            ),
            SmartPauseDecision::ResumeResetCycle
        );
    }

    #[test]
    fn smart_pause_does_not_resume_manual_stop_or_external_resting() {
        let mut policy = SmartPausePolicy::new();
        let cfg = SmartPauseConfig::default();

        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Resting,
                Duration::from_secs(50),
                Duration::from_secs(0),
                900,
            ),
            SmartPauseDecision::None
        );
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Stopped,
                Duration::from_secs(50),
                Duration::from_secs(0),
                900,
            ),
            SmartPauseDecision::None
        );

        let _ = policy.evaluate_idle(
            &cfg,
            SchedulerState::Waiting,
            Duration::from_secs(100),
            Duration::from_secs(10),
            900,
        );
        policy.reset();
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Resting,
                Duration::from_secs(120),
                Duration::from_secs(0),
                900,
            ),
            SmartPauseDecision::None
        );
    }

    #[test]
    fn smart_pause_postpone_if_active_and_no_underflow() {
        let mut policy = SmartPausePolicy::new();
        let cfg = SmartPauseConfig {
            enabled: true,
            idle_threshold_secs: 5,
            postpone_if_active: true,
        };
        assert_eq!(
            SmartPausePolicy::should_postpone_if_active(&cfg, Duration::from_secs(0)),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            SmartPausePolicy::should_postpone_if_active(&cfg, Duration::from_secs(1)),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            SmartPausePolicy::should_postpone_if_active(&cfg, Duration::from_secs(2)),
            None
        );
        assert_eq!(
            policy.evaluate_idle(
                &cfg,
                SchedulerState::Waiting,
                Duration::ZERO,
                Duration::from_secs(10),
                900,
            ),
            SmartPauseDecision::PauseResting
        );
    }

    #[test]
    fn dnd_precedence_and_matching_rules() {
        let disabled = DoNotDisturbConfig {
            enabled: false,
            while_on_battery: true,
            while_gaming: false,
            skip_break_windows: vec!["vlc".to_owned()],
            take_break_windows: Vec::new(),
        };
        let full_ctx = DndContext {
            is_fullscreen: true,
            active_window_class: Some("VLC Media Player".to_owned()),
            on_battery: true,
            system_inhibited: true,
            game_detected: true,
            ..DndContext::default()
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&disabled, &full_ctx),
            DndDecision::Allow
        );

        let cfg = DoNotDisturbConfig {
            enabled: true,
            while_on_battery: true,
            while_gaming: true,
            skip_break_windows: vec!["vlc".to_owned(), String::new(), "   ".to_owned()],
            take_break_windows: vec!["vlc".to_owned(), "code".to_owned()],
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&cfg, &full_ctx),
            DndDecision::Suppress(DndReason::OnBattery)
        );

        let both_lists = DndContext {
            active_window_class: Some("VLC Media Player".to_owned()),
            ..DndContext::default()
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&cfg, &both_lists),
            DndDecision::Suppress(DndReason::SkipWindow)
        );

        let allowed_fullscreen = DndContext {
            is_fullscreen: true,
            active_window_class: Some("Code-OSS".to_owned()),
            system_inhibited: true,
            ..DndContext::default()
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&cfg, &allowed_fullscreen),
            DndDecision::Allow
        );

        let firefox_full = DndContext {
            is_fullscreen: true,
            active_window_class: Some("Firefox".to_owned()),
            ..DndContext::default()
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&cfg, &firefox_full),
            DndDecision::Suppress(DndReason::Fullscreen)
        );

        let inhibited = DndContext {
            system_inhibited: true,
            ..DndContext::default()
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&cfg, &inhibited),
            DndDecision::Suppress(DndReason::SystemInhibited)
        );
    }

    #[test]
    fn gaming_auto_detection_handles_os_signals_paths_classes_and_overrides() {
        let gaming_only = DoNotDisturbConfig {
            enabled: false,
            while_gaming: true,
            while_on_battery: true,
            skip_break_windows: Vec::new(),
            take_break_windows: vec!["allowedgame".to_owned()],
        };

        let os_signal = DndContext {
            game_detected: true,
            ..DndContext::default()
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&gaming_only, &os_signal),
            DndDecision::Suppress(DndReason::Gaming)
        );

        let steam_windowed = DndContext {
            active_process_name: Some("Hades2.exe".to_owned()),
            active_executable_path: Some(
                r"C:\Program Files (x86)\Steam\steamapps\common\Hades II\Ship\Hades2.exe"
                    .to_owned(),
            ),
            ..DndContext::default()
        };
        assert!(is_gaming_context(&steam_windowed));
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&gaming_only, &steam_windowed),
            DndDecision::Suppress(DndReason::Gaming)
        );

        let steam_client = DndContext {
            active_process_name: Some("steamwebhelper.exe".to_owned()),
            active_executable_path: Some(
                r"C:\Program Files (x86)\Steam\steamapps\common\steamwebhelper.exe".to_owned(),
            ),
            ..DndContext::default()
        };
        assert!(!is_gaming_context(&steam_client));
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&gaming_only, &steam_client),
            DndDecision::Allow
        );

        let unity_editor = DndContext {
            active_process_name: Some("Unity.exe".to_owned()),
            active_window_class: Some("UnityWndClass".to_owned()),
            ..DndContext::default()
        };
        assert!(!is_gaming_context(&unity_editor));

        let linux_steam_class = DndContext {
            active_window_class: Some("steam_app_1091500".to_owned()),
            ..DndContext::default()
        };
        assert!(is_gaming_context(&linux_steam_class));

        let known_exe = DndContext {
            active_process_name: Some("VALORANT-Win64-Shipping.exe".to_owned()),
            ..DndContext::default()
        };
        assert!(is_gaming_context(&known_exe));

        let non_game_fullscreen = DndContext {
            is_fullscreen: true,
            active_process_name: Some("firefox".to_owned()),
            on_battery: true,
            ..DndContext::default()
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&gaming_only, &non_game_fullscreen),
            DndDecision::Allow
        );

        let overridden_game = DndContext {
            active_process_name: Some("AllowedGame.exe".to_owned()),
            game_detected: true,
            ..DndContext::default()
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&gaming_only, &overridden_game),
            DndDecision::Allow
        );

        let gaming_disabled = DoNotDisturbConfig {
            enabled: false,
            while_gaming: false,
            ..DoNotDisturbConfig::default()
        };
        assert_eq!(
            DoNotDisturbPolicy::evaluate(&gaming_disabled, &os_signal),
            DndDecision::Allow
        );
    }

    #[test]
    fn skip_limit_tracks_locks_and_resets() {
        let disabled = SkipLimitConfig {
            enabled: false,
            max_skips: 2,
        };
        let mut policy = SkipLimitPolicy::new(0);
        for _ in 0..5 {
            policy.record_skip();
        }
        assert!(!policy.controls_disabled(&disabled));
        assert_eq!(policy.status_label(&disabled), None);

        let enabled = SkipLimitConfig {
            enabled: true,
            max_skips: 2,
        };
        let mut policy = SkipLimitPolicy::new(0);
        assert!(!policy.controls_disabled(&enabled));
        assert_eq!(policy.status_label(&enabled), None);

        policy.record_skip();
        assert!(!policy.controls_disabled(&enabled));
        assert_eq!(
            policy.status_label(&enabled).as_deref(),
            Some("Skipped or postponed 1/2 breaks in a row")
        );

        policy.record_postpone();
        assert!(policy.controls_disabled(&enabled));
        assert_eq!(
            policy.status_label(&enabled).as_deref(),
            Some("Skipped or postponed 2/2 breaks in a row")
        );

        policy.record_completed_break();
        assert_eq!(policy.count(), 0);
        assert!(!policy.controls_disabled(&enabled));
        assert_eq!(policy.status_label(&enabled), None);

        let mut max_policy = SkipLimitPolicy::new(u32::MAX);
        max_policy.record_skip();
        assert_eq!(max_policy.count(), u32::MAX);

        let zero_cfg = SkipLimitConfig {
            enabled: true,
            max_skips: 0,
        };
        assert!(SkipLimitPolicy::new(0).controls_disabled(&zero_cfg));
    }
}
