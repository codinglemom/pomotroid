pub mod audio;
pub mod commands;
pub mod db;
pub mod notifications;
pub mod settings;
pub mod shortcuts;
pub mod themes;
pub mod timer;
pub mod tray;
pub mod websocket;
pub mod windows;

use std::sync::Arc;

use log::LevelFilter;
use tauri::Manager;
use tauri_plugin_log::{Builder as LogBuilder, RotationStrategy, Target, TargetKind};

use commands::{
    accessibility_trusted,
    tray_supported,
    app_version,
    check_update,
    install_update,
    audio_clear_custom, audio_get_custom_info, audio_preview, audio_set_custom, audio_set_preset,
    get_log_dir, open_log_dir,
    notification_show, notification_set_locale, notification_test,
    settings_get, settings_reset_defaults, settings_set,
    shortcuts_reload,
    sessions_clear,
    wallet_get_summary, wallet_get_income_history, wallet_get_ledger, wallet_set_rate, wallet_spend,
    stats_get_detailed, stats_get_heatmap,
    themes_list,
    timer_get_state, timer_reset, timer_restart_round, timer_skip, timer_toggle,
    window_apply_material, window_open_widget, window_restore_main, window_set_visibility,
    window_widget_ready,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            LogBuilder::new()
                .targets([Target::new(TargetKind::LogDir { file_name: None })])
                .max_file_size(5 * 1024 * 1024)
                .rotation_strategy(RotationStrategy::KeepOne)
                .level(LevelFilter::Debug)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // Capture Rust panics to the log file before the process terminates.
            std::panic::set_hook(Box::new(|info| {
                log::error!("PANIC: {info}");
            }));

            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data directory");

            std::fs::create_dir_all(&app_data_dir)
                .expect("failed to create app data directory");

            // --- Database ---
            let db = match db::open(&app_data_dir) {
                Ok(d) => {
                    log::info!(
                        "[app] version={} sha={}",
                        env!("APP_BUILD_VERSION"),
                        env!("APP_BUILD_SHA")
                    );
                    log::info!(
                        "小番茄 v{} — data dir: {}",
                        env!("CARGO_PKG_VERSION"),
                        app_data_dir.display()
                    );
                    log::info!("Database opened successfully");
                    d
                }
                Err(e) => {
                    log::error!("Failed to open database: {e}");
                    panic!("failed to open database: {e}");
                }
            };
            {
                let conn = db.lock().unwrap();
                settings::seed_defaults(&conn).expect("failed to seed default settings");
            }
            app.manage(db.clone());

            // --- Tray state (always created; icon populated only when min_to_tray is on) ---
            let tray_state = tray::TrayState::new();
            app.manage(Arc::clone(&tray_state));
            #[cfg(target_os = "windows")]
            tray::panel::start(app.handle());

            // --- Load settings once (used by Timer, Audio, etc.) ---
            let initial_settings = {
                let conn = db.lock().unwrap();
                settings::load(&conn).expect("failed to load settings")
            };

            // Apply the persisted log level before any further setup.
            if initial_settings.verbose_logging {
                log::set_max_level(LevelFilter::Debug);
                log::info!("Verbose logging enabled — log level set to DEBUG");
            } else {
                log::set_max_level(LevelFilter::Info);
            }

            // Sync tray state from saved settings.
            *tray_state.countdown_mode.lock().unwrap() = initial_settings.dial_countdown;
            let tray_theme_name = match initial_settings.theme_mode.as_str() {
                "dark" => &initial_settings.theme_dark,
                _ => &initial_settings.theme_light,
            };
            if let Some(theme) = themes::find(&app_data_dir, tray_theme_name) {
                *tray_state.colors.lock().unwrap() = tray::TrayColors::from_colors_map(&theme.colors);
            }

            // --- Audio engine (opens the output device on demand) ---
            let alert_sounds = {
                let conn = db.lock().unwrap();
                audio::AlertSounds::load(&conn, &app_data_dir.join("audio"))
            };
            app.manage(audio::AudioManager::new(&initial_settings, alert_sounds));

            // --- Timer controller (needs settings + AppHandle + TrayState + DB) ---
            let timer = timer::TimerController::new(
                app.handle().clone(),
                initial_settings.clone(),
                Arc::clone(&tray_state),
                db.clone(),
            );
            app.manage(timer);

            // Create initial tray icon if tray_icon_enabled is on, or if an
            // existing user has min_to_tray enabled (backwards compatibility).
            //
            // On Linux, TrayIconBuilder::build() can block the main thread
            // indefinitely on KDE Plasma 6 / Wayland while waiting for the
            // StatusNotifierWatcher D-Bus service to respond.  Spawning on a
            // background thread lets setup() return so the event loop starts
            // and the window can appear while the tray registers asynchronously.
            if initial_settings.tray_icon_enabled || initial_settings.min_to_tray {
                #[cfg(target_os = "linux")]
                {
                    let app_handle = app.handle().clone();
                    let ts = Arc::clone(&tray_state);
                    std::thread::spawn(move || {
                        tray::create_tray(&app_handle, &ts);
                    });
                }
                #[cfg(not(target_os = "linux"))]
                tray::create_tray(app.handle(), &tray_state);
            }

            // --- Theme hot-reload watcher ---
            // The watcher must stay alive for the duration of the app.
            // Wrap in a Mutex so it satisfies Send + Sync for Tauri manage.
            if let Some(watcher) = themes::watcher::spawn_watcher(
                app_data_dir.clone(),
                app.handle().clone(),
            ) {
                app.manage(std::sync::Mutex::new(watcher));
            }

            // --- Global shortcuts ---
            shortcuts::register_all(app.handle(), &initial_settings);

            // --- WebSocket server (opt-in) ---
            let ws_state = websocket::WsState::new();
            app.manage(Arc::clone(&ws_state));

            if initial_settings.websocket_enabled {
                let port = initial_settings.websocket_port;
                let app_clone = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    websocket::start(port, app_clone, &ws_state).await;
                });
            }

            // Shared lifecycle handlers apply to the initial and rebuilt main window.
            app.manage(windows::WindowState::default());
            app.manage(notifications::LocaleState::default());
            let main_window = app.get_webview_window("main").expect("main window not found");
            windows::configure_main(&main_window, &initial_settings);

            Ok(())
        })
        .on_window_event(windows::on_window_event)
        .invoke_handler(tauri::generate_handler![
            // Timer
            timer_toggle,
            timer_reset,
            timer_restart_round,
            timer_skip,
            timer_get_state,
            // Settings
            settings_get,
            settings_set,
            settings_reset_defaults,
            // Themes
            themes_list,
            // Sessions
            sessions_clear,
            wallet_get_summary,
            wallet_get_income_history,
            wallet_get_ledger,
            wallet_set_rate,
            wallet_spend,
            // Stats
            stats_get_detailed,
            stats_get_heatmap,
            // Window
            #[cfg(target_os = "windows")]
            tray::panel::tray_panel_ready,
            #[cfg(target_os = "windows")]
            tray::panel::tray_panel_action,
            window_set_visibility,
            window_apply_material,
            window_open_widget,
            window_widget_ready,
            window_restore_main,
            // Shortcuts
            shortcuts_reload,
            // Audio
            audio_set_custom,
            audio_clear_custom,
            audio_get_custom_info,
            audio_set_preset,
            audio_preview,
            // Notifications
            notification_show, notification_set_locale, notification_test,
            // Diagnostics
            open_log_dir,
            get_log_dir,
            accessibility_trusted,
            tray_supported,
            app_version,
            // Updater
            check_update,
            install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if windows::prevent_exit(app, code) {
                    api.prevent_exit();
                }
            }
        });
}
