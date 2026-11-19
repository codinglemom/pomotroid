//! Notifications are dispatched by the timer thread, including with no WebViews.
use serde::Deserialize;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};

#[cfg(target_os = "windows")]
mod windows;

#[derive(Default)]
pub struct LocaleState(Mutex<String>);

impl LocaleState {
    pub fn set(&self, locale: &str) {
        *self.0.lock().unwrap() = normalize_locale(locale).to_string();
    }

    fn resolve(&self, language: &str) -> String {
        if language == "auto" {
            normalize_locale(&self.0.lock().unwrap()).to_string()
        } else {
            normalize_locale(language).to_string()
        }
    }
}

const CATALOGS: [(&str, &str); 8] = [
    ("en", include_str!("../../../src/messages/en.json")),
    ("de", include_str!("../../../src/messages/de.json")),
    ("es", include_str!("../../../src/messages/es.json")),
    ("fr", include_str!("../../../src/messages/fr.json")),
    ("ja", include_str!("../../../src/messages/ja.json")),
    ("pt", include_str!("../../../src/messages/pt.json")),
    ("tr", include_str!("../../../src/messages/tr.json")),
    ("zh", include_str!("../../../src/messages/zh.json")),
];

#[derive(Deserialize)]
struct Messages {
    notification_work_title: String,
    notification_work_body: String,
    notification_work_start_title: String,
    notification_work_start_body: String,
    notification_short_break_title: String,
    notification_short_break_body: String,
    notification_long_break_title: String,
    notification_long_break_body: String,
    notification_test_title: String,
    notification_test_body: String,
}

fn normalize_locale(locale: &str) -> &str {
    let prefix = locale.split('-').next().unwrap_or("");
    CATALOGS
        .iter()
        .find(|(tag, _)| *tag == prefix)
        .map(|(tag, _)| *tag)
        .unwrap_or("en")
}

fn messages(locale: &str) -> &'static Messages {
    static MESSAGES: OnceLock<Vec<Messages>> = OnceLock::new();
    let catalogs = MESSAGES.get_or_init(|| {
        CATALOGS
            .iter()
            .map(|(_, json)| {
                serde_json::from_str(json).expect("bundled notification messages must be valid")
            })
            .collect()
    });
    let index = CATALOGS
        .iter()
        .position(|(tag, _)| *tag == normalize_locale(locale))
        .unwrap_or(0);
    &catalogs[index]
}

fn round_notification(
    enabled: bool,
    locale: &str,
    next_round: &str,
    previous_round: &str,
) -> Option<(&'static str, &'static str)> {
    if !enabled {
        return None;
    }
    let m = messages(locale);
    let (title, body) = match next_round {
        "work" if matches!(previous_round, "short-break" | "long-break") => {
            (&m.notification_work_title, &m.notification_work_body)
        }
        "work" => (
            &m.notification_work_start_title,
            &m.notification_work_start_body,
        ),
        "short-break" => (
            &m.notification_short_break_title,
            &m.notification_short_break_body,
        ),
        "long-break" => (
            &m.notification_long_break_title,
            &m.notification_long_break_body,
        ),
        _ => return None,
    };
    Some((title, body))
}

/// Called once per Complete event, including manual skips, matching the old
/// frontend round-change behavior. Reset/start/tick do not send notifications.
pub fn show_round_change(
    app: &AppHandle,
    settings: &crate::settings::Settings,
    next: &str,
    previous: &str,
) {
    let locale = app.state::<LocaleState>().resolve(&settings.language);
    if let Some((title, body)) =
        round_notification(settings.notifications_enabled, &locale, next, previous)
    {
        dispatch(app, title, body);
    }
}

/// Compatibility entry point for callers supplying their own title and body.
pub fn show(app: &AppHandle, title: &str, body: &str) {
    dispatch(app, title, body);
}

fn test_notification(enabled: bool, locale: &str) -> Result<(&'static str, &'static str), String> {
    if !enabled {
        return Err("notifications_disabled".into());
    }
    let m = messages(locale);
    Ok((&m.notification_test_title, &m.notification_test_body))
}

/// A successful return means submission only; Windows may suppress delivery.
pub fn show_test(app: &AppHandle, settings: &crate::settings::Settings) -> Result<(), String> {
    let locale = app.state::<LocaleState>().resolve(&settings.language);
    let (title, body) = test_notification(settings.notifications_enabled, &locale)?;
    try_dispatch(app, title, body)
}

#[cfg(target_os = "windows")]
fn dispatch(app: &AppHandle, title: &str, body: &str) {
    let app = app.clone();
    let title = title.to_owned();
    let body = body.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = try_dispatch(&app, &title, &body);
    });
}

#[cfg(not(target_os = "windows"))]
fn dispatch(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = try_dispatch(app, title, body) {
        log::warn!("[notifications] failed to submit notification: {e}");
    }
}

// Linux uses notify-send to inherit the correct D-Bus session environment.
#[cfg(target_os = "linux")]
fn try_dispatch(_app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    match std::process::Command::new("notify-send")
        .args([
            "--app-name=小番茄",
            "--urgency=normal",
            "--expire-time=5000",
            title,
            body,
        ])
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("notify-send exited with {status}")),
        Err(e) => Err(format!("failed to launch notify-send: {e}")),
    }
}

#[cfg(target_os = "windows")]
fn try_dispatch(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    windows::submit(&app.config().identifier, title, body).map_err(|error| {
        log::warn!("[notifications] failed to submit Windows notification: {error}");
        error.code().into()
    })
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn try_dispatch(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    // The desktop plugin queues a background task and cannot report OS delivery.
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_locale_preserves_notification_text() {
        for (locale, json) in CATALOGS {
            let source: serde_json::Value = serde_json::from_str(json).unwrap();
            for (next, previous, key) in [
                ("work", "short-break", "work"),
                ("work", "long-break", "work"),
                ("work", "", "work_start"),
                ("short-break", "work", "short_break"),
                ("long-break", "work", "long_break"),
            ] {
                let (title, body) = round_notification(true, locale, next, previous).unwrap();
                assert_eq!(
                    title,
                    source[format!("notification_{key}_title")]
                        .as_str()
                        .unwrap(),
                    "{locale}: {next}"
                );
                assert_eq!(
                    body,
                    source[format!("notification_{key}_body")].as_str().unwrap(),
                    "{locale}: {next}"
                );
            }
        }
    }

    #[test]
    fn disabled_notifications_suppress_every_round() {
        for round in ["work", "short-break", "long-break"] {
            assert!(round_notification(false, "zh", round, "work").is_none());
        }
        assert!(round_notification(true, "en", "invalid", "work").is_none());
    }

    #[test]
    fn test_notification_obeys_toggle_and_uses_each_bundled_locale() {
        assert_eq!(
            test_notification(false, "zh").unwrap_err(),
            "notifications_disabled"
        );
        for (locale, json) in CATALOGS {
            let source: serde_json::Value = serde_json::from_str(json).unwrap();
            let (title, body) = test_notification(true, locale).unwrap();
            assert_eq!(title, source["notification_test_title"].as_str().unwrap());
            assert_eq!(body, source["notification_test_body"].as_str().unwrap());
            assert!(!title.is_empty() && !body.is_empty());
        }
        assert_eq!(
            test_notification(true, "zh-CN").unwrap(),
            test_notification(true, "zh").unwrap()
        );
        assert_eq!(
            test_notification(true, "unknown").unwrap(),
            test_notification(true, "en").unwrap()
        );
    }

    #[test]
    fn auto_locale_survives_frontend_destruction_and_setting_changes() {
        let locale = LocaleState::default();
        assert_eq!(locale.resolve("auto"), "en");
        locale.set("ja-JP");
        assert_eq!(locale.resolve("auto"), "ja");
        assert_eq!(locale.resolve("de"), "de");
        assert_eq!(locale.resolve("auto"), "ja");
        assert_eq!(locale.resolve("unknown"), "en");
        locale.set("pt-BR");
        assert_eq!(locale.resolve("auto"), "pt");
    }
}
