use sye_core::config::{BreakType, ThemeMode};
use sye_core::{HOMEBREW_UPGRADE_COMMAND, LanguageMode, Locale};

#[derive(Debug, Clone, Copy)]
pub(crate) struct UiText {
    pub settings_window_title: &'static str,
    pub overlay_window_title: &'static str,
    pub sidebar_subtitle: &'static str,
    pub tab_timing: &'static str,
    pub tab_policies: &'static str,
    pub tab_exercises: &'static str,
    pub tab_about: &'static str,

    pub kpi_short_break: &'static str,
    pub kpi_long_break: &'static str,
    pub kpi_exercises: &'static str,
    pub kpi_skip_limit: &'static str,
    pub kpi_strict: &'static str,
    pub kpi_off: &'static str,

    pub unit_secs: &'static str,
    pub unit_ms: &'static str,
    pub unit_skips: &'static str,
    pub unit_volume_range: &'static str,

    pub timing_schedule_title: &'static str,
    pub timing_schedule_caption: &'static str,
    pub short_break_interval: &'static str,
    pub short_break_duration: &'static str,
    pub long_break_interval: &'static str,
    pub long_break_duration: &'static str,

    pub timing_controls_title: &'static str,
    pub timing_controls_caption: &'static str,
    pub pre_break_warning: &'static str,
    pub fade_in_duration: &'static str,
    pub shortcut_unlock_delay: &'static str,
    pub postpone_duration: &'static str,
    pub randomize_exercise_order: &'static str,
    pub persist_session: &'static str,
    pub launch_at_login: &'static str,

    pub policies_enforcement_title: &'static str,
    pub policies_enforcement_caption: &'static str,
    pub strict_break: &'static str,
    pub allow_postpone: &'static str,
    pub limit_consecutive_skips: &'static str,
    pub max_consecutive_skips: &'static str,

    pub policies_smart_dnd_title: &'static str,
    pub policies_smart_dnd_caption: &'static str,
    pub smart_pause_on_idle: &'static str,
    pub idle_threshold: &'static str,
    pub dnd_fullscreen: &'static str,
    pub pause_while_gaming: &'static str,
    pub pause_while_on_battery: &'static str,

    pub policies_chime_title: &'static str,
    pub policies_chime_caption: &'static str,
    pub pre_break_chime: &'static str,
    pub post_break_chime: &'static str,
    pub chime_volume: &'static str,

    pub short_exercises_title: &'static str,
    pub short_exercises_caption: &'static str,
    pub short_exercises_placeholder: &'static str,
    pub long_exercises_title: &'static str,
    pub long_exercises_caption: &'static str,
    pub long_exercises_placeholder: &'static str,
    pub add_button: &'static str,
    pub remove_button: &'static str,

    pub updates_title: &'static str,
    pub updates_caption: &'static str,
    pub check_updates_auto: &'static str,
    pub update_and_restart: &'static str,
    pub updating_button: &'static str,
    pub download_page: &'static str,
    pub download_update: &'static str,
    pub check_for_updates: &'static str,
    pub checking_button: &'static str,

    pub support_title: &'static str,
    pub support_caption: &'static str,
    pub support_button: &'static str,
    pub support_qr_caption: &'static str,

    pub autosave_note: &'static str,
    pub reset_to_defaults: &'static str,

    pub countdown_disabled: &'static str,
    pub countdown_disabled_a11y: &'static str,
    pub countdown_short: &'static str,
    pub countdown_long: &'static str,
    pub countdown_next: &'static str,
    pub countdown_starting: &'static str,
    pub countdown_starting_a11y: &'static str,
    pub countdown_break_active: &'static str,
    pub countdown_break_active_a11y: &'static str,
    pub countdown_paused: &'static str,
    pub countdown_paused_a11y: &'static str,
    pub countdown_initializing: &'static str,
    pub countdown_initializing_a11y: &'static str,

    pub overlay_short_badge: &'static str,
    pub overlay_long_badge: &'static str,
    pub overlay_postpone_button: &'static str,
    pub overlay_skip_button: &'static str,
    pub overlay_progress_a11y: &'static str,
    pub overlay_strict_hint: &'static str,
    pub overlay_relax_hint: &'static str,

    pub err_exercise_empty: &'static str,
    pub err_exercise_last_remaining: &'static str,
    pub err_exercise_none_configured: &'static str,
    pub window_list_skip: &'static str,
    pub window_list_take: &'static str,
}

const EN_TEXT: UiText = UiText {
    settings_window_title: "Safe Your Eyes — Settings",
    overlay_window_title: "Safe Your Eyes — Break",
    sidebar_subtitle: "Cozy Eye Care",
    tab_timing: "Timing",
    tab_policies: "Policies",
    tab_exercises: "Exercises",
    tab_about: "About & Support",

    kpi_short_break: "Short Break",
    kpi_long_break: "Long Break",
    kpi_exercises: "Exercises",
    kpi_skip_limit: "Skip Limit",
    kpi_strict: "Strict",
    kpi_off: "Off",

    unit_secs: "secs",
    unit_ms: "ms",
    unit_skips: "skips",
    unit_volume_range: "0–100",

    timing_schedule_title: "Break Schedule",
    timing_schedule_caption: "Configure short and long break cadence and durations.",
    short_break_interval: "Short break interval",
    short_break_duration: "Short break duration",
    long_break_interval: "Long break interval",
    long_break_duration: "Long break duration",

    timing_controls_title: "Transitions & Controls",
    timing_controls_caption: "Fine-tune warnings, fade-in speed, postpone length, session persistence, and OS startup.",
    pre_break_warning: "Pre-break warning",
    fade_in_duration: "Fade-in duration",
    shortcut_unlock_delay: "Shortcut unlock delay",
    postpone_duration: "Postpone duration",
    randomize_exercise_order: "Randomize exercise order",
    persist_session: "Persist session across restarts",
    launch_at_login: "Launch at login (startup)",

    policies_enforcement_title: "Break Enforcement",
    policies_enforcement_caption: "Control whether breaks can be skipped, postponed, or strictly enforced.",
    strict_break: "Strict break (hide Skip)",
    allow_postpone: "Allow postpone (Space)",
    limit_consecutive_skips: "Limit consecutive skips",
    max_consecutive_skips: "Max consecutive skips",

    policies_smart_dnd_title: "Smart Pause & Do Not Disturb",
    policies_smart_dnd_caption: "Automatically pause timers while idle, gaming, on battery, or in fullscreen apps.",
    smart_pause_on_idle: "Smart Pause on idle",
    idle_threshold: "Idle threshold",
    dnd_fullscreen: "Do Not Disturb in fullscreen",
    pause_while_gaming: "Pause while gaming (auto-detect)",
    pause_while_on_battery: "Pause while on battery",

    policies_chime_title: "Audible Chime",
    policies_chime_caption: "Play a gentle synthesized chime before or after breaks.",
    pre_break_chime: "Pre-break chime",
    post_break_chime: "Post-break chime",
    chime_volume: "Chime volume",

    short_exercises_title: "Short Break Exercises",
    short_exercises_caption: "Quick 15-second prompts cycled during work intervals.",
    short_exercises_placeholder: "Add a short exercise...",
    long_exercises_title: "Long Break Exercises",
    long_exercises_caption: "Restorative prompts shown after every four short breaks.",
    long_exercises_placeholder: "Add a long exercise...",
    add_button: "Add",
    remove_button: "Remove",

    updates_title: "Updates",
    updates_caption: "Checks GitHub Releases for newer versions and verifies SHA-256 before installing.",
    check_updates_auto: "Check for updates automatically",
    update_and_restart: "Update & Restart",
    updating_button: "Updating...",
    download_page: "Download Page",
    download_update: "Download Update",
    check_for_updates: "Check for Updates",
    checking_button: "Checking...",

    support_title: "Support Safe Your Eyes",
    support_caption: "Built in Rust with Native Shell (nsh) and the Cozy Design System for minimal footprint.",
    support_button: "Buy Me a Coffee",
    support_qr_caption: "Scan to Buy Me a Coffee",

    autosave_note: "Changes are saved automatically",
    reset_to_defaults: "Reset to defaults",

    countdown_disabled: "Disabled",
    countdown_disabled_a11y: "Break timer disabled",
    countdown_short: "Short",
    countdown_long: "Long",
    countdown_next: "Next",
    countdown_starting: "Starting",
    countdown_starting_a11y: "Break starting soon",
    countdown_break_active: "Break active",
    countdown_break_active_a11y: "Break active",
    countdown_paused: "Paused (idle)",
    countdown_paused_a11y: "Break timer paused while idle",
    countdown_initializing: "Initializing",
    countdown_initializing_a11y: "Break timer initializing",

    overlay_short_badge: "SHORT BREAK",
    overlay_long_badge: "LONG BREAK",
    overlay_postpone_button: "Postpone (Space)",
    overlay_skip_button: "Skip (Esc)",
    overlay_progress_a11y: "Break time remaining",
    overlay_strict_hint: "Strict break active — rest your eyes",
    overlay_relax_hint: "Relax your gaze until the timer completes",

    err_exercise_empty: "Exercise name must not be empty",
    err_exercise_last_remaining: "At least one break exercise must remain configured",
    err_exercise_none_configured: "At least one break exercise must be configured",
    window_list_skip: "Skip-break",
    window_list_take: "Take-break",
};

const VI_TEXT: UiText = UiText {
    settings_window_title: "Safe Your Eyes — Cài đặt",
    overlay_window_title: "Safe Your Eyes — Giờ nghỉ",
    sidebar_subtitle: "Chăm sóc mắt dịu nhẹ",
    tab_timing: "Thời gian",
    tab_policies: "Chính sách",
    tab_exercises: "Bài tập",
    tab_about: "Thông tin & Ủng hộ",

    kpi_short_break: "Nghỉ ngắn",
    kpi_long_break: "Nghỉ dài",
    kpi_exercises: "Bài tập",
    kpi_skip_limit: "Giới hạn bỏ qua",
    kpi_strict: "Nghiêm ngặt",
    kpi_off: "Tắt",

    unit_secs: "giây",
    unit_ms: "ms",
    unit_skips: "lần",
    unit_volume_range: "0–100",

    timing_schedule_title: "Lịch nghỉ",
    timing_schedule_caption: "Thiết lập chu kỳ và thời lượng cho giờ nghỉ ngắn và nghỉ dài.",
    short_break_interval: "Chu kỳ nghỉ ngắn",
    short_break_duration: "Thời lượng nghỉ ngắn",
    long_break_interval: "Chu kỳ nghỉ dài",
    long_break_duration: "Thời lượng nghỉ dài",

    timing_controls_title: "Chuyển tiếp & Điều khiển",
    timing_controls_caption: "Tùy chỉnh cảnh báo trước, tốc độ hiện mờ dần, thời gian hoãn, lưu phiên và khởi động cùng hệ thống.",
    pre_break_warning: "Cảnh báo trước khi nghỉ",
    fade_in_duration: "Thời gian hiện mờ dần",
    shortcut_unlock_delay: "Khóa phím tắt ban đầu",
    postpone_duration: "Thời gian hoãn nghỉ",
    randomize_exercise_order: "Trộn ngẫu nhiên thứ tự bài tập",
    persist_session: "Giữ tiến độ phiên sau khi khởi động lại",
    launch_at_login: "Khởi động cùng hệ thống",

    policies_enforcement_title: "Quy tắc giờ nghỉ",
    policies_enforcement_caption: "Kiểm soát việc bỏ qua, hoãn nghỉ hoặc áp dụng chế độ nghỉ nghiêm ngặt.",
    strict_break: "Nghỉ nghiêm ngặt (ẩn nút Bỏ qua)",
    allow_postpone: "Cho phép hoãn nghỉ (Space)",
    limit_consecutive_skips: "Giới hạn số lần bỏ qua liên tiếp",
    max_consecutive_skips: "Số lần bỏ qua tối đa",

    policies_smart_dnd_title: "Tạm dừng thông minh & Không làm phiền",
    policies_smart_dnd_caption: "Tự động tạm dừng khi không thao tác, chơi game, dùng pin hoặc mở ứng dụng toàn màn hình.",
    smart_pause_on_idle: "Tự động tạm dừng khi rảnh",
    idle_threshold: "Ngưỡng không thao tác",
    dnd_fullscreen: "Không làm phiền khi toàn màn hình",
    pause_while_gaming: "Tạm dừng khi chơi game (tự nhận diện)",
    pause_while_on_battery: "Tạm dừng khi dùng pin",

    policies_chime_title: "Âm báo",
    policies_chime_caption: "Phát tiếng chuông dịu nhẹ trước hoặc sau giờ nghỉ.",
    pre_break_chime: "Chuông trước giờ nghỉ",
    post_break_chime: "Chuông sau giờ nghỉ",
    chime_volume: "Âm lượng chuông",

    short_exercises_title: "Bài tập nghỉ ngắn",
    short_exercises_caption: "Gợi ý nhanh 15 giây xuất hiện xen kẽ trong lúc làm việc.",
    short_exercises_placeholder: "Thêm bài tập nghỉ ngắn...",
    long_exercises_title: "Bài tập nghỉ dài",
    long_exercises_caption: "Gợi ý thư giãn sâu hơn sau mỗi bốn lần nghỉ ngắn.",
    long_exercises_placeholder: "Thêm bài tập nghỉ dài...",
    add_button: "Thêm",
    remove_button: "Xóa",

    updates_title: "Cập nhật",
    updates_caption: "Kiểm tra bản phát hành mới trên GitHub Releases và xác minh SHA-256 trước khi cài đặt.",
    check_updates_auto: "Tự động kiểm tra cập nhật",
    update_and_restart: "Cập nhật & Khởi động lại",
    updating_button: "Đang cập nhật...",
    download_page: "Trang tải xuống",
    download_update: "Tải bản cập nhật",
    check_for_updates: "Kiểm tra cập nhật",
    checking_button: "Đang kiểm tra...",

    support_title: "Ủng hộ Safe Your Eyes",
    support_caption: "Xây dựng bằng Rust với Native Shell (nsh) và Cozy Design System để tối ưu gọn nhẹ.",
    support_button: "Buy Me a Coffee",
    support_qr_caption: "Quét mã Buy Me a Coffee",

    autosave_note: "Mọi thay đổi được tự động lưu",
    reset_to_defaults: "Khôi phục mặc định",

    countdown_disabled: "Đang tắt",
    countdown_disabled_a11y: "Bộ hẹn giờ nghỉ đang tắt",
    countdown_short: "Nghỉ ngắn",
    countdown_long: "Nghỉ dài",
    countdown_next: "Tiếp theo",
    countdown_starting: "Sắp nghỉ",
    countdown_starting_a11y: "Sắp đến giờ nghỉ",
    countdown_break_active: "Đang nghỉ",
    countdown_break_active_a11y: "Đang trong giờ nghỉ",
    countdown_paused: "Tạm dừng",
    countdown_paused_a11y: "Bộ hẹn giờ nghỉ tạm dừng khi rảnh",
    countdown_initializing: "Đang khởi tạo",
    countdown_initializing_a11y: "Đang khởi tạo bộ hẹn giờ nghỉ",

    overlay_short_badge: "NGHỈ NGẮN",
    overlay_long_badge: "NGHỈ DÀI",
    overlay_postpone_button: "Hoãn (Space)",
    overlay_skip_button: "Bỏ qua (Esc)",
    overlay_progress_a11y: "Thời gian nghỉ còn lại",
    overlay_strict_hint: "Chế độ nghỉ nghiêm ngặt — hãy cho mắt nghỉ ngơi",
    overlay_relax_hint: "Thả lỏng mắt cho đến khi hết giờ",

    err_exercise_empty: "Tên bài tập không được để trống",
    err_exercise_last_remaining: "Phải giữ lại ít nhất một bài tập nghỉ",
    err_exercise_none_configured: "Phải cấu hình ít nhất một bài tập nghỉ",
    window_list_skip: "Bỏ qua nghỉ",
    window_list_take: "Luôn nghỉ",
};

#[must_use]
pub(crate) const fn ui_text(locale: Locale) -> UiText {
    match locale {
        Locale::En => EN_TEXT,
        Locale::Vi => VI_TEXT,
    }
}

#[must_use]
pub(crate) const fn language_mode_label(mode: LanguageMode, locale: Locale) -> &'static str {
    match (mode, locale) {
        (LanguageMode::System, Locale::En) => "Auto",
        (LanguageMode::System, Locale::Vi) => "Tự động",
        (LanguageMode::En, _) => "EN",
        (LanguageMode::Vi, _) => "VI",
    }
}

#[must_use]
pub(crate) const fn language_mode_a11y_label(mode: LanguageMode, resolved: Locale) -> &'static str {
    match (mode, resolved) {
        (LanguageMode::System, Locale::En) => "Language: System (English)",
        (LanguageMode::System, Locale::Vi) => "Ngôn ngữ: Hệ thống (Tiếng Việt)",
        (LanguageMode::En, _) => "Language: English",
        (LanguageMode::Vi, _) => "Ngôn ngữ: Tiếng Việt",
    }
}

#[must_use]
pub(crate) const fn theme_mode_label_localized(mode: ThemeMode, locale: Locale) -> &'static str {
    match (locale, mode) {
        (Locale::En, ThemeMode::System) => "Theme: System",
        (Locale::En, ThemeMode::Light) => "Theme: Light",
        (Locale::En, ThemeMode::Dark) => "Theme: Dark",
        (Locale::Vi, ThemeMode::System) => "Giao diện: Hệ thống",
        (Locale::Vi, ThemeMode::Light) => "Giao diện: Sáng",
        (Locale::Vi, ThemeMode::Dark) => "Giao diện: Tối",
    }
}

#[must_use]
pub(crate) fn format_kpi_cadence(interval_secs: u64, duration_secs: u64, locale: Locale) -> String {
    let mins = interval_secs / 60;
    match locale {
        Locale::En => format!("{mins}m / {duration_secs}s"),
        Locale::Vi => format!("{mins}p / {duration_secs}g"),
    }
}

#[must_use]
pub(crate) fn format_kpi_max_skips(max_skips: u32, locale: Locale) -> String {
    match locale {
        Locale::En => format!("{max_skips} max"),
        Locale::Vi => format!("Tối đa {max_skips}"),
    }
}

#[must_use]
pub(crate) fn format_spoken_duration(total_secs: u64, locale: Locale) -> String {
    if total_secs < 60 {
        return match locale {
            Locale::En => "less than a minute".to_owned(),
            Locale::Vi => "dưới 1 phút".to_owned(),
        };
    }
    let mins = total_secs.div_ceil(60);
    match locale {
        Locale::En => {
            let min_unit = if mins == 1 { "minute" } else { "minutes" };
            format!("{mins} {min_unit}")
        }
        Locale::Vi => format!("{mins} phút"),
    }
}

#[must_use]
pub(crate) fn format_waiting_a11y(
    kind: Option<BreakType>,
    total_secs: u64,
    locale: Locale,
) -> String {
    let spoken_time = format_spoken_duration(total_secs, locale);
    match (locale, kind) {
        (Locale::En, Some(BreakType::Short)) => format!("Next short break in {spoken_time}"),
        (Locale::En, Some(BreakType::Long)) => format!("Next long break in {spoken_time}"),
        (Locale::En, None) => format!("Next break in {spoken_time}"),
        (Locale::Vi, Some(BreakType::Short)) => format!("Nghỉ ngắn tiếp theo sau {spoken_time}"),
        (Locale::Vi, Some(BreakType::Long)) => format!("Nghỉ dài tiếp theo sau {spoken_time}"),
        (Locale::Vi, None) => format!("Lần nghỉ tiếp theo sau {spoken_time}"),
    }
}

#[must_use]
pub(crate) fn format_overlay_unlock_hint(secs: u64, locale: Locale) -> String {
    match locale {
        Locale::En => format!("Controls unlock in {secs}s"),
        Locale::Vi => format!("Mở khóa nút sau {secs} giây"),
    }
}

#[must_use]
pub(crate) fn format_version_summary(
    current_version: &str,
    latest_version: Option<&str>,
    locale: Locale,
) -> String {
    match (locale, latest_version) {
        (Locale::En, Some(latest)) => format!("Current: v{current_version}  ->  Latest: v{latest}"),
        (Locale::En, None) => format!("Current version: v{current_version}"),
        (Locale::Vi, Some(latest)) => {
            format!("Hiện tại: v{current_version}  ->  Mới nhất: v{latest}")
        }
        (Locale::Vi, None) => format!("Phiên bản hiện tại: v{current_version}"),
    }
}

#[must_use]
pub(crate) fn format_homebrew_upgrade_hint(locale: Locale) -> String {
    match locale {
        Locale::En => format!("Homebrew upgrade: {HOMEBREW_UPGRADE_COMMAND}"),
        Locale::Vi => format!("Cập nhật qua Homebrew: {HOMEBREW_UPGRADE_COMMAND}"),
    }
}

#[must_use]
pub(crate) fn format_support_link_caption(url: &str, locale: Locale) -> String {
    match locale {
        Locale::En => format!("Support link: {url}"),
        Locale::Vi => format!("Liên kết ủng hộ: {url}"),
    }
}

#[must_use]
pub(crate) fn format_exercise_index_oob(index: usize, locale: Locale) -> String {
    match locale {
        Locale::En => format!("Exercise index {index} out of bounds"),
        Locale::Vi => format!("Chỉ số bài tập {index} không hợp lệ"),
    }
}

#[must_use]
pub(crate) fn format_window_pattern_empty(label: &str, locale: Locale) -> String {
    match locale {
        Locale::En => format!("{label} window pattern must not be empty"),
        Locale::Vi => format!("Mẫu cửa sổ {label} không được để trống"),
    }
}

#[must_use]
pub(crate) fn format_window_index_oob(label: &str, index: usize, locale: Locale) -> String {
    match locale {
        Locale::En => format!("{label} window index {index} out of bounds"),
        Locale::Vi => format!("Chỉ số cửa sổ {label} {index} không hợp lệ"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateStatusMessage {
    Available(String),
    UpToDate(String),
    CheckFailed(String),
    RestartingDaemon,
    UpdatedStandalone,
    SelfUpdateFailed(String),
    WorkerDisconnected,
    Downloading(String),
    Checking,
    OpenedDownloadPage,
    OpenedDownloadPageHomebrew,
    OpenDownloadFailed(String),
}

impl UpdateStatusMessage {
    #[must_use]
    pub(crate) fn format(&self, locale: Locale) -> String {
        match (locale, self) {
            (Locale::En, Self::Available(ver)) => format!("Version v{ver} is available."),
            (Locale::Vi, Self::Available(ver)) => format!("Đã có phiên bản v{ver}."),
            (Locale::En, Self::UpToDate(ver)) => format!("You are up to date (v{ver})."),
            (Locale::Vi, Self::UpToDate(ver)) => {
                format!("Bạn đang dùng phiên bản mới nhất (v{ver}).")
            }
            (Locale::En, Self::CheckFailed(err)) => format!("Update check failed: {err}"),
            (Locale::Vi, Self::CheckFailed(err)) => format!("Kiểm tra cập nhật thất bại: {err}"),
            (Locale::En, Self::RestartingDaemon) => {
                "Update verified. Restarting Safe Your Eyes...".to_owned()
            }
            (Locale::Vi, Self::RestartingDaemon) => {
                "Đã xác minh bản cập nhật. Đang khởi động lại Safe Your Eyes...".to_owned()
            }
            (Locale::En, Self::UpdatedStandalone) => {
                "Update installed and verified. Restart Safe Your Eyes to use the new version."
                    .to_owned()
            }
            (Locale::Vi, Self::UpdatedStandalone) => {
                "Đã cài đặt và xác minh bản cập nhật. Hãy khởi động lại Safe Your Eyes để dùng phiên bản mới."
                    .to_owned()
            }
            (Locale::En, Self::SelfUpdateFailed(err)) => {
                format!("Self-update failed: {err}. Use Download Page instead.")
            }
            (Locale::Vi, Self::SelfUpdateFailed(err)) => {
                format!("Tự cập nhật thất bại: {err}. Hãy dùng Trang tải xuống thay thế.")
            }
            (Locale::En, Self::WorkerDisconnected) => {
                "Update task stopped unexpectedly.".to_owned()
            }
            (Locale::Vi, Self::WorkerDisconnected) => {
                "Tác vụ cập nhật đã dừng đột ngột.".to_owned()
            }
            (Locale::En, Self::Downloading(ver)) => format!("Downloading and verifying v{ver}..."),
            (Locale::Vi, Self::Downloading(ver)) => format!("Đang tải và xác minh v{ver}..."),
            (Locale::En, Self::Checking) => "Checking for updates...".to_owned(),
            (Locale::Vi, Self::Checking) => "Đang kiểm tra cập nhật...".to_owned(),
            (Locale::En, Self::OpenedDownloadPage) => "Opened download page.".to_owned(),
            (Locale::Vi, Self::OpenedDownloadPage) => "Đã mở trang tải xuống.".to_owned(),
            (Locale::En, Self::OpenedDownloadPageHomebrew) => {
                format!("Opened download page. Or run: {HOMEBREW_UPGRADE_COMMAND}")
            }
            (Locale::Vi, Self::OpenedDownloadPageHomebrew) => {
                format!("Đã mở trang tải xuống. Hoặc chạy: {HOMEBREW_UPGRADE_COMMAND}")
            }
            (Locale::En, Self::OpenDownloadFailed(err)) => {
                format!("Failed to open download page: {err}")
            }
            (Locale::Vi, Self::OpenDownloadFailed(err)) => {
                format!("Không thể mở trang tải xuống: {err}")
            }
        }
    }
}
