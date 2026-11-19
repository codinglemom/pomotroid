/// All #[tauri::command] functions exposed to the Svelte frontend via Tauri IPC.
///
/// Commands are grouped by domain: Timer, Settings, Themes, Stats.
/// Each command returns `Result<T, String>` so errors surface cleanly in JS.
use log::LevelFilter;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use std::sync::Arc;

use crate::audio::{AlertSounds, AudioManager, CustomAudioInfo};
use crate::notifications;
use crate::db::{queries, wallet, DbState};
use crate::settings::{self, Settings};
use crate::shortcuts;
use crate::themes::{self, Theme};
use crate::timer::{TimerController, TimerSnapshot};
use crate::tray::{self, TrayState};
use crate::websocket::{self, WsState};

// ---------------------------------------------------------------------------
// CMD-01 — Timer commands
// ---------------------------------------------------------------------------

/// Toggle the timer: start if idle, resume if paused, pause if running.
/// This is the primary action bound to the space bar and the play/pause button.
#[tauri::command]
pub fn timer_toggle(timer: State<'_, TimerController>) {
    timer.toggle();
}

/// Reset the current round's timer without advancing the sequence.
#[tauri::command]
pub fn timer_reset(timer: State<'_, TimerController>) {
    timer.reset();
}

/// Skip the current round: fires Complete immediately and advances to the next.
#[tauri::command]
pub fn timer_skip(timer: State<'_, TimerController>) {
    timer.skip();
}

/// Restart the current round from zero without advancing the sequence.
/// Round type and round number are preserved; only elapsed time is reset.
#[tauri::command]
pub fn timer_restart_round(timer: State<'_, TimerController>) {
    timer.restart_round();
}

/// Return a full snapshot of the current timer state.
/// Called once on frontend mount to hydrate stores.
#[tauri::command]
pub fn timer_get_state(timer: State<'_, TimerController>) -> TimerSnapshot {
    timer.get_snapshot()
}

// ---------------------------------------------------------------------------
// CMD-02 — Settings commands
// ---------------------------------------------------------------------------

/// Return all current settings.
#[tauri::command]
pub fn settings_get(db: State<'_, DbState>) -> Result<Settings, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    settings::load(&conn).map_err(|e| {
        log::error!("[settings] failed to load settings: {e}");
        e.to_string()
    })
}

/// Persist a single setting and emit `settings:changed` with the updated set.
///
/// `key` must be one of the DB column names (see `settings::defaults::DEFAULTS`).
/// `value` is always a string; the loader converts it to the appropriate type.
#[tauri::command]
pub fn settings_set(
    key: String,
    value: String,
    db: State<'_, DbState>,
    timer: State<'_, TimerController>,
    tray_state: State<'_, Arc<TrayState>>,
    ws_state: State<'_, Arc<WsState>>,
    app: AppHandle,
) -> Result<Settings, String> {
    if key == wallet::RATE_KEY {
        return Err("Use wallet_set_rate to change the reward rate".into());
    }
    if key == "acrylic_transparency" {
        settings::validate_acrylic_transparency(&value)?;
    }
    if key == "timer_style" {
        value.parse::<settings::TimerStyle>()?;
    }
    log::debug!("[settings] set {key}={value}");
    let new_settings = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        settings::save_setting(&conn, &key, &value).map_err(|e| {
            log::error!("[settings] failed to save '{key}': {e}");
            e.to_string()
        })?;
        // When SIT is turned off, cascade-reset the dependent tray settings so
        // the close-to-tray handler cannot hide the window with no icon to
        // restore from.
        if key == "tray_icon_enabled" && value == "false" {
            settings::save_setting(&conn, "min_to_tray", "false").map_err(|e| e.to_string())?;
            settings::save_setting(&conn, "min_to_tray_on_close", "false").map_err(|e| e.to_string())?;
        }
        settings::load(&conn).map_err(|e| {
            log::error!("[settings] failed to reload after save: {e}");
            e.to_string()
        })?
    };

    if key == "timer_style" {
        app.emit("settings:changed", &new_settings).ok();
        return Ok(new_settings);
    }

    // Apply verbose_logging change immediately without a restart.
    if key == "verbose_logging" {
        if new_settings.verbose_logging {
            log::set_max_level(LevelFilter::Debug);
            log::info!("Verbose logging enabled — log level set to DEBUG");
        } else {
            log::set_max_level(LevelFilter::Info);
            log::info!("Verbose logging disabled — log level set to INFO");
        }
    }

    // Keep the timer engine in sync when time-related settings change.
    timer.apply_settings(new_settings.clone());

    // Broadcast an updated snapshot so the frontend immediately reflects any
    // changed settings (round count, durations, etc.) regardless of timer
    // state.  The timer:reset handler only calls timerState.set(), so emitting
    // while running does not interrupt the countdown; the next timer:tick
    // event will reconcile total_secs from the engine within one second.
    app.emit("timer:reset", &timer.get_snapshot()).ok();

    // Propagate volume and tick-sound changes to the audio engine (optional state).
    if let Some(audio) = app.try_state::<Arc<AudioManager>>() {
        audio.apply_settings(&new_settings);
    }

    // Apply always-on-top window flag immediately when the setting changes,
    // accounting for the current round type so break_always_on_top takes
    // effect without waiting for the next round transition.
    if matches!(key.as_str(), "always_on_top" | "break_always_on_top") {
        if let Some(window) = app.get_webview_window("main") {
            let snap = timer.get_snapshot();
            let is_break = snap.round_type != "work";
            let effective_aot = new_settings.always_on_top
                && !(new_settings.break_always_on_top && is_break);
            let _ = window.set_always_on_top(effective_aot);
        }
    }

    // Sync tray countdown mode when the dial setting changes, then immediately
    // re-render the icon so it matches the dial without waiting for a timer event.
    if key == "dial_countdown" {
        *tray_state.countdown_mode.lock().unwrap() = new_settings.dial_countdown;
        let snap = timer.get_snapshot();
        let progress = if snap.total_secs > 0 {
            snap.elapsed_secs as f32 / snap.total_secs as f32
        } else {
            0.0
        };
        tray::update_icon(&tray_state, &snap.round_type, snap.is_paused, progress);
    }

    // Update tray icon colors when the active theme changes.
    if matches!(key.as_str(), "theme_mode" | "theme_light" | "theme_dark") {
        let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let tray_theme_name = match new_settings.theme_mode.as_str() {
            "dark" => &new_settings.theme_dark,
            _ => &new_settings.theme_light,
        };
        if let Some(theme) = themes::find(&data_dir, tray_theme_name) {
            *tray_state.colors.lock().unwrap() = tray::TrayColors::from_colors_map(&theme.colors);
            let snap = timer.get_snapshot();
            let progress = if snap.total_secs > 0 {
                snap.elapsed_secs as f32 / snap.total_secs as f32
            } else {
                0.0
            };
            tray::update_icon(&tray_state, &snap.round_type, snap.is_paused, progress);
        }
    }

    // Create or destroy the tray when tray_icon_enabled or min_to_tray changes.
    // The tray exists when either flag is true.
    // On Linux, spawn tray creation on a background thread to avoid blocking
    // the main thread on KDE Plasma 6 / Wayland (D-Bus StatusNotifier hang).
    if matches!(key.as_str(), "tray_icon_enabled" | "min_to_tray") {
        if new_settings.tray_icon_enabled || new_settings.min_to_tray {
            #[cfg(target_os = "linux")]
            {
                let app_handle = app.clone();
                let ts = Arc::clone(&tray_state);
                std::thread::spawn(move || {
                    tray::create_tray(&app_handle, &ts);
                });
            }
            #[cfg(not(target_os = "linux"))]
            tray::create_tray(&app, &tray_state);
        } else {
            tray::destroy_tray(&tray_state);
        }
    }

    // Re-register global shortcuts when any shortcut key changes or the enabled flag toggles.
    if matches!(key.as_str(), "shortcut_toggle" | "shortcut_reset" | "shortcut_skip" | "shortcut_restart" | "global_shortcuts_enabled") {
        shortcuts::register_all(&app, &new_settings);
    }

    // Start or stop the WebSocket server when the enabled flag or port changes.
    if matches!(key.as_str(), "websocket_enabled" | "websocket_port") {
        let ws = Arc::clone(&*ws_state);
        let port = new_settings.websocket_port;
        let enabled = new_settings.websocket_enabled;
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            // Always stop the old server first.
            websocket::stop(&ws).await;
            if enabled {
                websocket::start(port, app_clone, &ws).await;
            }
        });
    }

    app.emit("settings:changed", &new_settings).ok();
    Ok(new_settings)
}

// ---------------------------------------------------------------------------
// CMD-06 — Shortcuts command
// ---------------------------------------------------------------------------

/// Re-register all global shortcuts from the current settings.
/// The frontend can call this after bulk-updating shortcut settings.
#[tauri::command]
pub fn shortcuts_reload(db: State<'_, DbState>, app: AppHandle) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let s = settings::load(&conn).map_err(|e| e.to_string())?;
    shortcuts::register_all(&app, &s);
    Ok(())
}

/// Reset all settings to factory defaults and return the resulting settings.
#[tauri::command]
pub fn settings_reset_defaults(
    db: State<'_, DbState>,
    timer: State<'_, TimerController>,
    tray_state: State<'_, Arc<TrayState>>,
    audio: State<'_, Arc<AudioManager>>,
    app: AppHandle,
) -> Result<Settings, String> {
    log::info!("[settings] reset to defaults");
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let new_settings = {
        let mut conn = db.lock().map_err(|e| e.to_string())?;
        let mut alerts = audio.alerts.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        // Delete all rows so seed_defaults can insert fresh defaults.
        tx.execute("DELETE FROM settings", [])
            .map_err(|e| e.to_string())?;
        settings::seed_defaults(&tx).map_err(|e| e.to_string())?;
        AlertSounds::save_classic_presets(&tx).map_err(|e| e.to_string())?;
        let new_settings = settings::load(&tx).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        alerts.reset(&data_dir.join("audio"));
        audio.apply_settings(&new_settings);
        new_settings
    };

    timer.apply_settings(new_settings.clone());
    *tray_state.countdown_mode.lock().unwrap() = new_settings.dial_countdown;

    // Broadcast a reset snapshot so the frontend dial and display reflect the
    // restored default durations without requiring the user to manually reset.
    {
        let snap = timer.get_snapshot();
        if !snap.is_running && !snap.is_paused {
            app.emit("timer:reset", &snap).ok();
        }
    }

    // After reset, defaults have tray_icon_enabled=false and min_to_tray=false,
    // so destroy any active tray icon.
    tray::destroy_tray(&tray_state);

    let tray_theme_name = match new_settings.theme_mode.as_str() {
        "dark" => &new_settings.theme_dark,
        _ => &new_settings.theme_light,
    };
    if let Some(theme) = themes::find(&data_dir, tray_theme_name) {
        *tray_state.colors.lock().unwrap() = tray::TrayColors::from_colors_map(&theme.colors);
    }
    shortcuts::register_all(&app, &new_settings);
    app.emit("settings:changed", &new_settings).ok();
    Ok(new_settings)
}

// ---------------------------------------------------------------------------
// CMD-03 — Theme commands
// ---------------------------------------------------------------------------

/// List all available themes (17 bundled + any user-created ones).
#[tauri::command]
pub fn themes_list(app: AppHandle) -> Result<Vec<Theme>, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    Ok(themes::list_all(&data_dir))
}

// ---------------------------------------------------------------------------
// CMD-04 — Sessions commands
// ---------------------------------------------------------------------------

/// Deletes session history, retaining the active round until it can settle.
/// Emits `sessions:cleared` so any open stats window can refresh immediately.
#[tauri::command]
pub fn sessions_clear(
    db: State<'_, DbState>,
    timer: State<'_, TimerController>,
    app: AppHandle,
) -> Result<(), String> {
    log::info!("[sessions] clearing all session history");
    let conn = db.lock().map_err(|e| e.to_string())?;
    // Session creation publishes its ID under this same database lock.
    let n = queries::clear_session_history(&conn, timer.active_session_id()).map_err(|e| {
        log::error!("[sessions] failed to clear history: {e}");
        e.to_string()
    })?;
    log::info!("[sessions] cleared {n} rows");
    app.emit("sessions:cleared", ()).ok();
    Ok(())
}

// Reward wallet commands use their own setting to avoid reconfiguring the timer.
#[tauri::command]
pub fn wallet_get_summary(db: State<'_, DbState>) -> Result<wallet::WalletSummary, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    wallet::summary(&conn).inspect_err(|e| log::error!("[wallet] summary failed: {e}"))
}

#[tauri::command]
pub fn wallet_get_income_history(
    db: State<'_, DbState>,
    days: u32,
) -> Result<wallet::WalletIncomeHistory, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    wallet::income_history(&conn, days)
        .inspect_err(|e| log::error!("[wallet] income history failed: {e}"))
}

#[tauri::command]
pub fn wallet_get_ledger(db: State<'_, DbState>, before_id: Option<i64>) -> Result<wallet::LedgerPage, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    wallet::ledger(&conn, before_id).inspect_err(|e| log::error!("[wallet] ledger failed: {e}"))
}

#[tauri::command]
pub fn wallet_set_rate(db: State<'_, DbState>, app: AppHandle, rate_minor_per_hour: i64) -> Result<(), String> {
    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        wallet::set_rate(&conn, rate_minor_per_hour).inspect_err(|e| log::error!("[wallet] rate update failed: {e}"))?;
    }
    log::info!("[wallet] hourly rate updated to {rate_minor_per_hour} minor units");
    app.emit("wallet:changed", ()).ok();
    Ok(())
}

#[tauri::command]
pub fn wallet_spend(db: State<'_, DbState>, app: AppHandle, amount_minor: i64, note: String, request_id: String) -> Result<(), String> {
    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        wallet::spend(&conn, amount_minor, &note, &request_id).inspect_err(|e| log::error!("[wallet] expense failed: {e}"))?;
    }
    log::info!("[wallet] expense committed: {amount_minor} minor units");
    app.emit("wallet:changed", ()).ok();
    Ok(())
}

// CMD-05 — Stats commands
// ---------------------------------------------------------------------------

/// Batched stats for Today + This Week tabs (minimises IPC round-trips).
#[tauri::command]
pub fn stats_get_detailed(db: State<'_, DbState>) -> Result<DetailedStats, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let today = queries::get_daily_stats(&conn).map_err(|e| {
        log::error!("[stats] failed to query daily stats: {e}");
        e.to_string()
    })?;
    let week = queries::get_weekly_stats(&conn).map_err(|e| {
        log::error!("[stats] failed to query weekly stats: {e}");
        e.to_string()
    })?;
    let streak = queries::get_streak(&conn).map_err(|e| {
        log::error!("[stats] failed to query streak: {e}");
        e.to_string()
    })?;
    Ok(DetailedStats { today, week, streak })
}

/// Heatmap data + lifetime totals for the All Time tab.
#[tauri::command]
pub fn stats_get_heatmap(db: State<'_, DbState>) -> Result<HeatmapStats, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let entries = queries::get_heatmap_data(&conn).map_err(|e| {
        log::error!("[stats] failed to query heatmap data: {e}");
        e.to_string()
    })?;
    let raw = queries::get_all_time_stats(&conn).map_err(|e| {
        log::error!("[stats] failed to query all-time stats: {e}");
        e.to_string()
    })?;
    let streak = queries::get_streak(&conn).map_err(|e| {
        log::error!("[stats] failed to query streak for heatmap: {e}");
        e.to_string()
    })?;
    Ok(HeatmapStats {
        entries,
        total_rounds: raw.completed_work_sessions as u32,
        total_hours: (raw.total_work_secs / 3600) as u32,
        longest_streak: streak.longest,
    })
}

// ---------------------------------------------------------------------------
// CMD-05 — Window commands
// ---------------------------------------------------------------------------

/// Show or hide the main window.
#[tauri::command]
pub async fn window_set_visibility(visible: bool, window: WebviewWindow) -> Result<(), String> {
    require_window(&window, "main")?;
    log::debug!("[window] set visibility={visible}");
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || crate::windows::set_visibility(&app, visible))
        .await
        .map_err(|e| e.to_string())?
}

fn require_window(window: &WebviewWindow, label: &str) -> Result<(), String> {
    if window.label() != label {
        return Err(format!("this command requires the {label} window"));
    }
    Ok(())
}

#[tauri::command]
pub async fn window_open_widget(window: WebviewWindow) -> Result<(), String> {
    require_window(&window, "main")?;
    tauri::async_runtime::spawn_blocking(move || crate::windows::open_widget(&window))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn window_widget_ready(window: WebviewWindow) -> Result<(), String> {
    require_window(&window, "widget")?;
    tauri::async_runtime::spawn_blocking(move || crate::windows::widget_ready(&window))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn window_restore_main(window: WebviewWindow) -> Result<(), String> {
    require_window(&window, "widget")?;
    tauri::async_runtime::spawn_blocking(move || crate::windows::restore_main(&window))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn window_apply_material(
    window: WebviewWindow,
    theme_mode: String,
    enabled: bool,
) -> Result<bool, String> {
    if window.label() == "widget" {
        return Ok(false);
    }
    crate::windows::apply_material(&window, &theme_mode, enabled)
}

// ---------------------------------------------------------------------------
// CMD-07 — Audio commands
// ---------------------------------------------------------------------------

/// Import and select a custom sound. Return its original filename.
#[tauri::command]
pub fn audio_set_custom(
    cue: String,
    src_path: String,
    db: State<'_, DbState>,
    audio: State<'_, Arc<AudioManager>>,
    app: AppHandle,
) -> Result<String, String> {
    let cue = cue.parse()?;
    let audio_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("audio");
    let mut conn = db.lock().map_err(|e| e.to_string())?;
    let mut alerts = audio.alerts.lock().map_err(|e| e.to_string())?;
    alerts.import(&mut conn, &audio_dir, cue, std::path::Path::new(&src_path))
}

/// Delete a custom import and select the classic sound.
#[tauri::command]
pub fn audio_clear_custom(
    cue: String,
    db: State<'_, DbState>,
    audio: State<'_, Arc<AudioManager>>,
    app: AppHandle,
) -> Result<(), String> {
    let cue = cue.parse()?;
    let audio_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("audio");
    let mut conn = db.lock().map_err(|e| e.to_string())?;
    let mut alerts = audio.alerts.lock().map_err(|e| e.to_string())?;
    alerts.clear(&mut conn, &audio_dir, cue)
}

/// Return the selected presets and file names of any retained custom imports.
#[tauri::command]
pub fn audio_get_custom_info(
    audio: State<'_, Arc<AudioManager>>,
) -> Result<CustomAudioInfo, String> {
    let alerts = audio.alerts.lock().map_err(|e| e.to_string())?;
    Ok(alerts.info())
}

#[tauri::command]
pub fn audio_set_preset(
    cue: String,
    preset: String,
    db: State<'_, DbState>,
    audio: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    let cue = cue.parse()?;
    let preset = preset.parse()?;
    let mut conn = db.lock().map_err(|e| e.to_string())?;
    let mut alerts = audio.alerts.lock().map_err(|e| e.to_string())?;
    alerts.set_preset(&mut conn, cue, preset)
}

#[tauri::command]
pub async fn audio_preview(cue: String, audio: State<'_, Arc<AudioManager>>) -> Result<(), String> {
    audio.preview(cue.parse()?).await
}

// ---------------------------------------------------------------------------
// CMD-08 — Notification command
// ---------------------------------------------------------------------------

/// Show a desktop notification with the given title and body.
///
/// String construction (including translation) is the caller's (frontend's)
/// responsibility. This command is a thin platform-dispatch wrapper.
#[tauri::command]
pub fn notification_show(title: String, body: String, app: AppHandle) {
    notifications::show(&app, &title, &body);
}

/// Keep the resolved auto locale available after all WebViews are destroyed.
#[tauri::command]
pub fn notification_set_locale(locale: String, state: State<'_, notifications::LocaleState>) {
    state.set(&locale);
}

#[tauri::command]
pub fn notification_test(db: State<'_, DbState>, app: AppHandle) -> Result<(), String> {
    let settings = settings_get(db)?;
    notifications::show_test(&app, &settings)
}

// ---------------------------------------------------------------------------
// CMD-09 — Diagnostic log commands
// ---------------------------------------------------------------------------

/// Open the application log directory in the OS file manager.
#[tauri::command]
pub fn open_log_dir(app: AppHandle) {
    match app.path().app_log_dir() {
        Ok(log_dir) => {
            if let Err(e) = tauri_plugin_opener::open_path(&log_dir, None::<&str>) {
                log::warn!("[log] failed to open log dir {}: {e}", log_dir.display());
            }
        }
        Err(e) => log::warn!("[log] failed to resolve log dir: {e}"),
    }
}

/// Return the compile-time build version string.
#[tauri::command]
pub fn app_version() -> &'static str {
    env!("APP_BUILD_VERSION")
}

// ---------------------------------------------------------------------------
// CMD-10 — Platform commands
// ---------------------------------------------------------------------------

/// Returns whether the app has macOS Accessibility permission.
/// On macOS, calls AXIsProcessTrusted() from the ApplicationServices framework.
/// On all other platforms, always returns true.
#[tauri::command]
pub fn accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrusted() -> bool;
        }
        unsafe { AXIsProcessTrusted() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Returns whether the system tray is supported on this platform/install.
/// On Linux, probes for libayatana-appindicator3 / libappindicator3 at runtime.
/// On macOS and Windows, always returns true.
#[tauri::command]
pub fn tray_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        tray::appindicator_available()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Return the application log directory path as a string.
#[tauri::command]
pub fn get_log_dir(app: AppHandle) -> Result<String, String> {
    app.path()
        .app_log_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| {
            log::warn!("[log] failed to resolve log dir: {e}");
            e.to_string()
        })
}

// ---------------------------------------------------------------------------
// CMD-11 — Updater commands
// ---------------------------------------------------------------------------

/// Information about an available update returned to the frontend.
#[derive(serde::Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub body: Option<String>,
    pub date: Option<String>,
}

/// Check whether a newer version is available.
/// Returns `Some(UpdateInfo)` when an update is available, or `None` when
/// the running version is already the latest.
/// Errors (e.g. network failure) are surfaced as a string so the frontend
/// can display a non-blocking message.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;
    log::info!("[updater] checking for updates");
    let updater = app.updater().map_err(|e| {
        log::error!("[updater] failed to build updater: {e}");
        e.to_string()
    })?;
    match updater.check().await {
        Ok(Some(update)) => {
            log::info!("[updater] update available: v{}", update.version);
            Ok(Some(UpdateInfo {
                version: update.version.clone(),
                body: update.body.clone(),
                date: update.date.map(|d| d.to_string()),
            }))
        }
        Ok(None) => {
            log::info!("[updater] already up to date");
            Ok(None)
        }
        Err(e) => {
            log::warn!("[updater] update check failed: {e}");
            Err(e.to_string())
        }
    }
}

/// Download, verify, and install the pending update, then relaunch immediately.
/// Should only be called after `check_update` has returned `Some(UpdateInfo)`.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    log::info!("[updater] install requested — checking for update");
    let updater = app.updater().map_err(|e| {
        log::error!("[updater] failed to build updater: {e}");
        e.to_string()
    })?;
    let update = updater
        .check()
        .await
        .map_err(|e| {
            log::error!("[updater] update check failed during install: {e}");
            e.to_string()
        })?
        .ok_or_else(|| {
            log::warn!("[updater] install_update called but no update is available");
            "No update available".to_string()
        })?;
    log::info!("[updater] downloading and installing v{}", update.version);
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| {
            log::error!("[updater] download/install failed: {e}");
            e.to_string()
        })?;
    log::info!("[updater] install complete — relaunching");
    app.restart();
}

// ---------------------------------------------------------------------------
// Stats payload types
// ---------------------------------------------------------------------------

/// Batched payload for Today + This Week tabs.
#[derive(serde::Serialize)]
pub struct DetailedStats {
    pub today: queries::DailyStats,
    pub week: Vec<queries::DayStat>,
    pub streak: queries::StreakInfo,
}

/// Payload for the All Time tab.
#[derive(serde::Serialize)]
pub struct HeatmapStats {
    pub entries: Vec<queries::HeatmapEntry>,
    pub total_rounds: u32,
    pub total_hours: u32,
    pub longest_streak: u32,
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use crate::db::migrations;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn seed_sessions(conn: &Connection) {
        conn.execute_batch("
            INSERT INTO sessions (started_at, ended_at, round_type, duration_secs, completed)
            VALUES (1000, 1060, 'work', 60, 1),
                   (2000, 2300, 'short-break', 300, 1);
        ").unwrap();
    }

    #[test]
    fn sessions_clear_removes_all_rows() {
        let conn = setup();
        seed_sessions(&conn);
        let before: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(before, 2);

        let n = conn.execute("DELETE FROM sessions", []).unwrap();
        assert_eq!(n, 2);

        let after: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(after, 0);
    }

    #[test]
    fn sessions_clear_on_empty_table_returns_zero() {
        let conn = setup();
        let n = conn.execute("DELETE FROM sessions", []).unwrap();
        assert_eq!(n, 0);
    }
}

