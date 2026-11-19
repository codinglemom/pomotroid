use std::{fmt, io, path::Path};
use windows::{
    core::{Error, HSTRING},
    Data::Xml::Dom::XmlDocument,
    Win32::{
        Foundation::{ERROR_NOT_FOUND, RPC_E_CHANGED_MODE},
        System::WinRT::{
            RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED, RO_INIT_SINGLETHREADED,
        },
    },
    UI::Notifications::{
        NotificationSetting, ToastNotification, ToastNotificationManager, ToastTemplateType,
    },
};

const DEVELOPMENT_APP_ID: &str =
    r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

#[derive(Debug)]
pub(super) enum SubmitError {
    ExePath(io::Error),
    Notifier(&'static str, Error),
    Disabled(NotificationSetting),
    Native(&'static str, Error),
}

impl SubmitError {
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::Notifier(_, error) if error.code() == ERROR_NOT_FOUND.to_hresult() => {
                "notifications_windows_unregistered"
            }
            Self::Disabled(_) => "notifications_windows_disabled",
            _ => "notifications_windows_failed",
        }
    }
}

impl fmt::Display for SubmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExePath(error) => write!(f, "resolving executable path: {error}"),
            Self::Notifier(operation, error) | Self::Native(operation, error) => {
                write!(f, "Windows toast {operation}: {error}")
            }
            Self::Disabled(setting) => write!(f, "Windows toast disabled (setting {})", setting.0),
        }
    }
}

struct WinRtApartment(std::marker::PhantomData<*mut ()>);

impl WinRtApartment {
    fn initialize() -> windows::core::Result<Self> {
        // SAFETY: A successful initialization is balanced by Drop on this thread.
        // The timer thread can use MTA; Tauri's UI thread may already use STA.
        unsafe {
            match RoInitialize(RO_INIT_MULTITHREADED) {
                Err(error) if error.code() == RPC_E_CHANGED_MODE => {
                    RoInitialize(RO_INIT_SINGLETHREADED)?;
                }
                result => result?,
            }
        }
        Ok(Self(std::marker::PhantomData))
    }
}

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        // SAFETY: This guard is local to the thread that initialized WinRT.
        unsafe { RoUninitialize() }
    }
}

fn notification_id<'a>(identifier: &'a str, executable: &Path) -> &'a str {
    // Preserve the Tauri notification plugin's existing host for Cargo builds.
    if executable
        .parent()
        .is_some_and(|dir| dir.ends_with(r"target\debug") || dir.ends_with(r"target\release"))
    {
        DEVELOPMENT_APP_ID
    } else {
        identifier
    }
}

pub(super) fn submit(identifier: &str, title: &str, body: &str) -> Result<(), SubmitError> {
    let executable = tauri::utils::platform::current_exe().map_err(SubmitError::ExePath)?;
    let identifier = notification_id(identifier, &executable);
    let _apartment = WinRtApartment::initialize()
        .map_err(|error| SubmitError::Native("initializing WinRT", error))?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(identifier))
        .map_err(|error| SubmitError::Notifier("creating notifier", error))?;
    let setting = notifier.Setting();
    if let Ok(setting) = setting.as_ref() {
        check_disabled(*setting)?;
    }

    let xml = toast_xml(title, body)
        .map_err(|error| SubmitError::Native("building XML payload", error))?;
    let toast = ToastNotification::CreateToastNotification(&xml)
        .map_err(|error| SubmitError::Native("creating notification", error))?;
    notifier
        .Show(&toast)
        .map_err(|error| SubmitError::Notifier("submitting notification", error))?;

    // A failed lookup before Show can occur on first use. Check again after
    // submission so an unregistered ID cannot appear to have passed the test.
    if setting.is_err() {
        match notifier.Setting() {
            Ok(setting) => check_disabled(setting)?,
            Err(error) if error.code() == ERROR_NOT_FOUND.to_hresult() => {
                return Err(SubmitError::Notifier("querying setting after submission", error));
            }
            Err(error) => log::warn!(
                "[notifications] Windows accepted the toast for {identifier}, but querying its setting failed: {error}"
            ),
        }
    }
    Ok(())
}

fn check_disabled(setting: NotificationSetting) -> Result<(), SubmitError> {
    match setting {
        NotificationSetting::DisabledForApplication
        | NotificationSetting::DisabledForUser
        | NotificationSetting::DisabledByGroupPolicy
        | NotificationSetting::DisabledByManifest => Err(SubmitError::Disabled(setting)),
        _ => Ok(()),
    }
}

fn toast_xml(title: &str, body: &str) -> windows::core::Result<XmlDocument> {
    let xml = ToastNotificationManager::GetTemplateContent(ToastTemplateType::ToastText02)?;
    let text = xml.GetElementsByTagName(&HSTRING::from("text"))?;
    text.Item(0)?
        .AppendChild(&xml.CreateTextNode(&HSTRING::from(title))?)?;
    text.Item(1)?
        .AppendChild(&xml.CreateTextNode(&HSTRING::from(body))?)?;

    // Pomotroid plays the configured round cue independently of the toast.
    let audio = xml.CreateElement(&HSTRING::from("audio"))?;
    audio.SetAttribute(&HSTRING::from("silent"), &HSTRING::from("true"))?;
    xml.DocumentElement()?.AppendChild(&audio)?;
    Ok(xml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_builds_keep_development_identity_and_other_paths_use_configured_identity() {
        let identifier = "com.splode.pomotroid";
        for path in [
            r"D:\project\target\debug\pomotroid.exe",
            r"D:\project\target\release\pomotroid.exe",
        ] {
            assert_eq!(
                notification_id(identifier, Path::new(path)),
                DEVELOPMENT_APP_ID
            );
        }
        for path in [
            r"D:\portable\pomotroid.exe",
            r"C:\Program Files\Pomotroid\pomotroid.exe",
        ] {
            assert_eq!(notification_id(identifier, Path::new(path)), identifier);
        }
    }

    #[test]
    fn windows_notifier_errors_have_specific_feedback_codes() {
        let missing = ERROR_NOT_FOUND.to_hresult();
        assert_eq!(
            SubmitError::Notifier("querying setting", missing.into()).code(),
            "notifications_windows_unregistered"
        );
        assert_eq!(
            SubmitError::Native("building XML payload", missing.into()).code(),
            "notifications_windows_failed"
        );
        for setting in [
            NotificationSetting::DisabledForApplication,
            NotificationSetting::DisabledForUser,
            NotificationSetting::DisabledByGroupPolicy,
            NotificationSetting::DisabledByManifest,
        ] {
            assert_eq!(
                check_disabled(setting).unwrap_err().code(),
                "notifications_windows_disabled"
            );
        }
        assert!(check_disabled(NotificationSetting::Enabled).is_ok());
    }

    #[test]
    fn toast_payload_keeps_xml_characters_unicode_and_silence() {
        let _apartment = WinRtApartment::initialize().unwrap();
        let title = "Focused & ready <work>";
        let body = "休憩 🍅\nTime to \"rest\" & reset";
        let xml = toast_xml(title, body).unwrap();
        let text = xml.GetElementsByTagName(&HSTRING::from("text")).unwrap();
        assert_eq!(text.Item(0).unwrap().InnerText().unwrap(), title);
        assert_eq!(text.Item(1).unwrap().InnerText().unwrap(), body);
        assert!(xml
            .GetXml()
            .unwrap()
            .to_string_lossy()
            .contains("<audio silent=\"true\""));
        ToastNotification::CreateToastNotification(&xml).unwrap();
    }
}
