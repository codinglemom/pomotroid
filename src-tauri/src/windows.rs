//! The Rust process owns the timer; WebViews exist only while the UI is in use.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

use crate::{
    db::DbState,
    settings::{self, Settings},
    timer::TimerController,
    tray::TrayState,
};

const WIDGET_SIZE: LogicalSize<f64> = LogicalSize::new(196.0, 112.0);

// The per-window acrylic accent does not follow the activation policy of the
// Windows 11 transient system backdrop. Use the same material at either focus.
#[cfg(target_os = "windows")]
fn acrylic_accent(hwnd: *mut std::ffi::c_void, enabled: bool) -> bool {
    use std::ffi::c_void;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient_color: u32,
        animation: u32,
    }
    #[repr(C)]
    struct AttributeData {
        attribute: u32,
        policy: *mut AccentPolicy,
        size: usize,
    }
    type Setter = unsafe extern "system" fn(*mut c_void, *mut AttributeData) -> i32;
    // SAFETY: user32 is already loaded by the live window. The dynamically
    // resolved setter receives stack-owned, C-layout data for a synchronous call.
    unsafe {
        let module = GetModuleHandleA(c"user32.dll".as_ptr().cast());
        if module.is_null() {
            return false;
        }
        let Some(proc) = GetProcAddress(module, c"SetWindowCompositionAttribute".as_ptr().cast())
        else {
            return false;
        };
        let set: Setter = std::mem::transmute(proc);
        let mut policy = AccentPolicy {
            state: if enabled { 4 } else { 0 }, // Acrylic blur-behind / disabled
            flags: 0,
            gradient_color: 0x01000000, // Acrylic needs nonzero alpha; CSS supplies the tint.
            animation: 0,
        };
        let mut data = AttributeData {
            attribute: 19, // WCA_ACCENT_POLICY
            policy: &mut policy,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        set(hwnd, &mut data) != 0
    }
}

/// True means Windows accepted a material request.
pub fn apply_material(
    window: &WebviewWindow,
    theme_mode: &str,
    enabled: bool,
) -> Result<bool, String> {
    let theme = match theme_mode {
        "auto" => None,
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => return Err("window_invalid_theme_mode".into()),
    };
    #[cfg(target_os = "windows")]
    {
        use tauri::window::{Effect, EffectsBuilder};

        // Prevent native show/hide transitions from exposing an unblurred backdrop.
        // This controls window transitions; DWM still owns Acrylic composition.
        #[link(name = "dwmapi")]
        extern "system" {
            fn DwmSetWindowAttribute(
                hwnd: *mut std::ffi::c_void,
                attribute: u32,
                value: *const std::ffi::c_void,
                size: u32,
            ) -> i32;
        }
        let disabled: i32 = 1;
        let hwnd = window.hwnd().map_err(|e| e.to_string())?;
        // SAFETY: HWND comes from this live Tauri window; BOOL remains valid for
        // the synchronous call and has the required four-byte Windows layout.
        let result =
            unsafe { DwmSetWindowAttribute(hwnd.0, 3, (&disabled as *const i32).cast(), 4) };
        if result < 0 {
            log::warn!("[window] could not disable native transitions: {result:#x}");
        }

        window.set_theme(theme).map_err(|e| e.to_string())?;
        if !acrylic_accent(hwnd.0, enabled) && enabled {
            log::warn!("[window] acrylic accent unavailable; using system Acrylic");
            window
                .set_effects(EffectsBuilder::new().effect(Effect::Acrylic).build())
                .map_err(|e| e.to_string())?;
        } else if !enabled {
            window.set_effects(None).map_err(|e| e.to_string())?;
        }
        Ok(enabled)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, theme, enabled);
        Ok(false)
    }
}

pub struct WindowState {
    // Only blocking workers acquire this lock. The event loop must remain free
    // to service the native window operations those workers are waiting for.
    lifecycle: Mutex<()>,
    background: AtomicBool,
    initializing: AtomicBool,
    destroying: AtomicBool,
    reopen_after_destroy: AtomicBool,
    widget_active: AtomicBool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            lifecycle: Mutex::new(()),
            background: AtomicBool::new(false),
            initializing: AtomicBool::new(true),
            destroying: AtomicBool::new(false),
            reopen_after_destroy: AtomicBool::new(false),
            widget_active: AtomicBool::new(false),
        }
    }
}

impl WindowState {
    fn with_lifecycle<T>(&self, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let _guard = self.lifecycle.lock().map_err(|e| e.to_string())?;
        action()
    }

    fn defer_show_until_destroyed(&self) -> bool {
        if self.destroying.load(Ordering::Acquire) {
            self.reopen_after_destroy.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }

    fn finish_destroy(&self) -> bool {
        self.destroying.store(false, Ordering::Release);
        self.reopen_after_destroy.swap(false, Ordering::AcqRel)
    }
}

fn load_settings(app: &AppHandle) -> Result<Settings, String> {
    let db = app.state::<DbState>();
    let conn = db.lock().map_err(|e| e.to_string())?;
    settings::load(&conn).map_err(|e| e.to_string())
}

fn tray_available(app: &AppHandle) -> bool {
    app.try_state::<Arc<TrayState>>()
        .is_some_and(|state| state.available.load(Ordering::Acquire))
}

/// Serialize lifecycle work on blocking workers, never inside event handlers.
/// Tauri dispatches native operations to its event loop itself. In particular,
/// WebView2 creation from a Windows event handler can deadlock (WRY #583).
fn queue_lifecycle(
    app: &AppHandle,
    action: impl FnOnce(&AppHandle) -> Result<(), String> + Send + 'static,
) {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<WindowState>();
        if let Err(e) = state.with_lifecycle(|| action(&handle)) {
            log::error!("[window] lifecycle action failed: {e}");
        }
    });
}

pub fn request_show(app: &AppHandle) {
    queue_lifecycle(app, show_main);
}

pub fn toggle_from_tray(app: &AppHandle) {
    queue_lifecycle(app, |handle| {
        if handle.state::<WindowState>().defer_show_until_destroyed() {
            return Ok(());
        }
        if handle.get_webview_window("widget").is_some() {
            return show_main(handle);
        }
        let visible = handle.get_webview_window("main").is_some_and(|window| {
            window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false)
        });
        if visible {
            enter_background(handle)
        } else {
            show_main(handle)
        }
    });
}

/// Create a dedicated WebView while preserving the main window's geometry and visibility.
pub fn open_widget(main: &WebviewWindow) -> Result<(), String> {
    let app = main.app_handle();
    let state = app.state::<WindowState>();
    state.with_lifecycle(|| {
        if state.background.load(Ordering::Acquire) || state.initializing.load(Ordering::Acquire) {
            return Err("main window is unavailable".into());
        }
        let visible = main.is_visible().map_err(|e| e.to_string())?;
        if app.get_webview_window("widget").is_some() {
            return Ok(());
        }
        if !visible || main.is_minimized().map_err(|e| e.to_string())? {
            return Err("main window is not visible".into());
        }
        let monitor = main.current_monitor().map_err(|e| e.to_string())?;
        let main_position = main.outer_position().map_err(|e| e.to_string())?;
        let main_width = main.outer_size().map_err(|e| e.to_string())?.width as i32;

        let builder = WebviewWindowBuilder::new(app, "widget", WebviewUrl::default())
            .title("小番茄")
            .inner_size(WIDGET_SIZE.width, WIDGET_SIZE.height)
            .visible(false)
            .focused(false)
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true);
        #[cfg(not(target_os = "macos"))]
        let builder = builder
            .transparent(true)
            .background_color(tauri::window::Color(0, 0, 0, 0));
        let widget = builder.build().map_err(|e| e.to_string())?;

        if let Some(monitor) = monitor {
            let work_area = monitor.work_area();
            let size = WIDGET_SIZE.to_physical::<u32>(monitor.scale_factor());
            let x = work_area.position.x;
            let y = work_area.position.y;
            let position = PhysicalPosition::new(
                (main_position.x + main_width - size.width as i32).clamp(
                    x,
                    x + work_area.size.width.saturating_sub(size.width) as i32,
                ),
                main_position.y.clamp(
                    y,
                    y + work_area.size.height.saturating_sub(size.height) as i32,
                ),
            );
            // Apply physical coordinates while hidden to preserve logical size at mixed DPI.
            if let Err(error) = widget
                .set_position(position)
                .and_then(|()| widget.set_size(size))
            {
                discard_widget(app);
                return Err(error.to_string());
            }
        }
        Ok(())
    })
}

/// The widget page signals readiness after it has rendered its first timer state.
pub fn widget_ready(widget: &WebviewWindow) -> Result<(), String> {
    let app = widget.app_handle();
    let state = app.state::<WindowState>();
    state.with_lifecycle(|| {
        // Query the invoking window itself so an old ready request cannot open
        // or hide anything after the widget label has been reused.
        widget.inner_size().map_err(|e| e.to_string())?;
        if state.background.load(Ordering::Acquire) {
            return Err("main window is unavailable".into());
        }
        if state.widget_active.load(Ordering::Acquire) {
            return Ok(());
        }

        let main = app
            .get_webview_window("main")
            .ok_or("main window missing")?;
        let activate = || -> Result<(), String> {
            if !main.is_visible().map_err(|e| e.to_string())?
                || main.is_minimized().map_err(|e| e.to_string())?
            {
                return Err("main window is not visible".into());
            }
            widget.show().map_err(|e| e.to_string())?;
            #[cfg(target_os = "windows")]
            if !widget.is_visible().map_err(|e| e.to_string())? {
                return Err("widget window could not be shown".into());
            }
            state.widget_active.store(true, Ordering::Release);
            main.hide().map_err(|e| e.to_string())?;
            #[cfg(target_os = "windows")]
            if main.is_visible().map_err(|e| e.to_string())? {
                return Err("main window could not be hidden".into());
            }
            Ok(())
        };
        if let Err(error) = activate() {
            if state.widget_active.swap(false, Ordering::AcqRel) {
                if let Err(error) = main.show() {
                    log::error!(
                        "[window] could not restore main after failed widget activation: {error}"
                    );
                }
            }
            discard_widget(app);
            return Err(error);
        }
        Ok(())
    })
}

pub fn restore_main(widget: &WebviewWindow) -> Result<(), String> {
    let app = widget.app_handle();
    let state = app.state::<WindowState>();
    state.with_lifecycle(|| {
        if widget.inner_size().is_ok() {
            show_main(app)?;
        }
        Ok(())
    })
}

fn destroy_widget(app: &AppHandle) -> Result<(), String> {
    let Some(widget) = app.get_webview_window("widget") else {
        return Ok(());
    };
    let (tx, rx) = mpsc::channel();
    widget.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            let _ = tx.send(());
        }
    });
    widget.destroy().map_err(|e| e.to_string())?;
    // Await removal of the old label on this worker before another open can reuse it.
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|error| format!("widget destruction did not finish: {error}"))?;
    Ok(())
}

fn discard_widget(app: &AppHandle) {
    if let Err(error) = destroy_widget(app) {
        log::error!("[window] could not discard widget: {error}");
    }
}

/// Called only by a blocking worker holding the lifecycle lock. A newly built
/// window stays hidden until its page applies its theme and calls
/// window_set_visibility(true). No database lock spans any window operations.
fn show_main(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<WindowState>();
    if state.defer_show_until_destroyed() {
        return Ok(());
    }
    if let Some(window) = app.get_webview_window("main") {
        if !state.initializing.load(Ordering::Acquire) {
            window.show().map_err(|e| e.to_string())?;
            window.unminimize().map_err(|e| e.to_string())?;
            #[cfg(target_os = "windows")]
            if !window.is_visible().map_err(|e| e.to_string())? {
                return Err("main window could not be shown".into());
            }
            window.set_focus().map_err(|e| e.to_string())?;
            state.background.store(false, Ordering::Release);
            state.widget_active.store(false, Ordering::Release);
            destroy_widget(app)?;
        }
        return Ok(());
    }

    let settings = load_settings(app)?;
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == "main")
        .ok_or("main window configuration missing")?;
    state.initializing.store(true, Ordering::Release);
    log::info!("[window] rebuilding main on blocking worker");
    let window = WebviewWindowBuilder::from_config(app, config)
        .map_err(|e| e.to_string())?
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    configure_main(&window, &settings);
    log::info!("[window] rebuilt main (waiting for theme)");
    Ok(())
}

/// Runs on a blocking worker, never the event loop.
pub fn set_visibility(app: &AppHandle, visible: bool) -> Result<(), String> {
    let state = app.state::<WindowState>();
    state.with_lifecycle(|| {
        if visible {
            // The page has applied its theme and is ready to appear.
            state.initializing.store(false, Ordering::Release);
            if state.widget_active.load(Ordering::Acquire) {
                Ok(())
            } else {
                show_main(app)
            }
        } else {
            load_settings(app).and_then(|settings| {
                if settings.min_to_tray && tray_available(app) {
                    enter_background(app)
                } else if let Some(window) = app.get_webview_window("main") {
                    if state.widget_active.load(Ordering::Acquire) {
                        show_main(app)?;
                    } else {
                        destroy_widget(app)?;
                    }
                    window.minimize().map_err(|e| e.to_string())
                } else {
                    Ok(())
                }
            })
        }
    })
}

fn enter_background(app: &AppHandle) -> Result<(), String> {
    if !tray_available(app) {
        return Err("system tray is unavailable".into());
    }
    let state = app.state::<WindowState>();
    // Repeated close/minimize requests must not overwrite an in-flight main
    // destruction after Tauri has already removed its old window handle.
    if state.destroying.load(Ordering::Acquire) {
        return Ok(());
    }
    // Set this BEFORE destroying the last window. Explicit app.exit still wins.
    state.background.store(true, Ordering::Release);
    state.initializing.store(true, Ordering::Release);
    state.widget_active.store(false, Ordering::Release);
    state
        .destroying
        .store(app.get_webview_window("main").is_some(), Ordering::Release);
    let mut failure = destroy_widget(app).err();
    for (label, window) in app.webview_windows() {
        if label == "widget" {
            continue;
        }
        if let Err(e) = window.destroy() {
            log::error!("[window] failed to destroy {}: {e}", window.label());
            failure = Some(e.to_string());
        }
    }
    log::info!("[window] entered Rust-only background");
    failure.map_or(Ok(()), Err)
}

pub fn configure_main(window: &WebviewWindow, settings: &Settings) {
    // The config sets decorations:true so macOS renders the window correctly
    // (decorations:false prevents event processing on macOS). On every other
    // platform, restore the decorations-free window immediately; the window is
    // still hidden at this point so there is no visible flash.
    #[cfg(not(target_os = "macos"))]
    let _ = window.set_decorations(false);

    // Enable macOS window tiling/arrangement.
    //
    // Two things are required for the full native experience:
    //
    // 1. NSWindowCollectionBehaviorManaged on the NSWindow — enables the
    //    tiling popup on the green traffic-light button AND tells macOS
    //    this window participates in Spaces / tiling.
    //
    // 2. NSApplication.setWindowsMenu: — registers the "Window" menu from
    //    the menu bar as the official windows menu. macOS then dynamically
    //    injects the standard window-management items (Fill, Center,
    //    Move & Resize with Halves/Quarters/Arrange, Full Screen Tile,
    //    Bring All to Front, etc.) when the menu is opened. Tauri may or
    //    may not wire this up, so we do it explicitly here.
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        if let Ok(handle) = window.window_handle() {
            if let RawWindowHandle::AppKit(h) = handle.as_raw() {
                let ns_view = h.ns_view.as_ptr() as *mut AnyObject;
                // SAFETY: ns_view is a valid NSView* supplied by Tauri/WRY.
                let ns_window: *mut AnyObject = unsafe { msg_send![ns_view, window] };

                // NSWindowCollectionBehaviorManaged        = 1 << 2  (tiling)
                // NSWindowCollectionBehaviorFullScreenPrimary = 1 << 10 (full-screen)
                let behavior: u64 = (1 << 2) | (1 << 10);
                unsafe {
                    let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
                }

                // Walk the main menu bar looking for the "Window" menu and
                // register it with NSApplication. This causes macOS to
                // auto-populate it with all standard window-management items.
                unsafe {
                    let ns_app_class = AnyClass::get(
                        std::ffi::CStr::from_bytes_with_nul_unchecked(b"NSApplication\0"),
                    );
                    if let Some(cls) = ns_app_class {
                        let ns_app: *mut AnyObject = msg_send![cls, sharedApplication];
                        let main_menu: *mut AnyObject = msg_send![ns_app, mainMenu];
                        if !main_menu.is_null() {
                            let count: isize = msg_send![main_menu, numberOfItems];
                            for i in 0..count {
                                let item: *mut AnyObject = msg_send![main_menu, itemAtIndex: i];
                                let submenu: *mut AnyObject = msg_send![item, submenu];
                                if submenu.is_null() {
                                    continue;
                                }
                                let title: *mut AnyObject = msg_send![item, title];
                                let utf8: *const std::ffi::c_char = msg_send![title, UTF8String];
                                if utf8.is_null() {
                                    continue;
                                }
                                let s = std::ffi::CStr::from_ptr(utf8).to_str().unwrap_or("");
                                if s == "Window" {
                                    let _: () = msg_send![
                                    ns_app,
                                    setWindowsMenu: submenu
                                        ];
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let app = window.app_handle();
    let is_break = app
        .try_state::<TimerController>()
        .is_some_and(|timer| timer.get_snapshot().round_type != "work");
    let _ = window
        .set_always_on_top(settings.always_on_top && !(settings.break_always_on_top && is_break));

    if let (Some(wx), Some(wy), Some(ww), Some(wh)) = (
        settings.window_x,
        settings.window_y,
        settings.window_width,
        settings.window_height,
    ) {
        let on_screen = app
            .available_monitors()
            .unwrap_or_default()
            .into_iter()
            .any(|monitor| {
                let pos = monitor.position();
                let size = monitor.size();
                (wx as i64) < pos.x as i64 + size.width as i64
                    && wx as i64 + ww as i64 > pos.x as i64
                    && (wy as i64) < pos.y as i64 + size.height as i64
                    && wy as i64 + wh as i64 > pos.y as i64
            });
        if on_screen {
            let _ = window.set_position(tauri::PhysicalPosition::new(wx, wy));
            let _ = window.set_size(tauri::PhysicalSize::new(ww, wh));
        }
    }
}

/// Global label routing also covers windows created after setup.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let app = window.app_handle();
    let Some(state) = app.try_state::<WindowState>() else {
        return;
    };
    if window.label() == "widget" {
        match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let widget = window.clone();
                queue_lifecycle(app, move |app| {
                    if widget.inner_size().is_ok() {
                        show_main(app)?;
                    }
                    Ok(())
                });
            }
            WindowEvent::Destroyed => {
                queue_lifecycle(app, |app| {
                    if app
                        .state::<WindowState>()
                        .widget_active
                        .load(Ordering::Acquire)
                        && app.get_webview_window("widget").is_none()
                    {
                        show_main(app)?;
                    }
                    Ok(())
                });
            }
            _ => {}
        }
        return;
    }
    if window.label() != "main" {
        return;
    }
    match event {
        WindowEvent::Destroyed => {
            // Never take the lifecycle lock in a callback: the worker holding
            // it may be waiting for this event loop. Serialize completion with
            // show requests so none can miss the deferred reopen flag.
            queue_lifecycle(app, |app| {
                // Fence runtime/manager cleanup of the old "main" label before
                // allowing a worker to attach its replacement. This callback
                // only acknowledges the fence; it does no lifecycle work.
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                app.run_on_main_thread(move || {
                    let _ = tx.send(());
                })
                .map_err(|e| e.to_string())?;
                rx.recv().map_err(|e| e.to_string())?;
                if app.state::<WindowState>().finish_destroy() {
                    show_main(app)?;
                }
                Ok(())
            });
        }
        WindowEvent::CloseRequested { api, .. } => {
            let to_tray = load_settings(app)
                .map(|settings| settings.min_to_tray_on_close)
                .unwrap_or(false)
                && tray_available(app);
            api.prevent_close();
            if to_tray {
                queue_lifecycle(app, enter_background);
            } else {
                // A normal close exits even when secondary windows are open.
                app.exit(0);
            }
        }
        WindowEvent::Moved(pos) => {
            if state.background.load(Ordering::Acquire)
                || state.initializing.load(Ordering::Acquire)
                || state.widget_active.load(Ordering::Acquire)
                || window.is_minimized().unwrap_or(false)
            {
                return;
            }
            if let Some(db) = app.try_state::<DbState>() {
                if let Ok(conn) = db.lock() {
                    let _ = settings::save_setting(&conn, "window_x", &pos.x.to_string());
                    let _ = settings::save_setting(&conn, "window_y", &pos.y.to_string());
                }
            }
        }
        WindowEvent::Resized(size) => {
            if state.background.load(Ordering::Acquire)
                || state.initializing.load(Ordering::Acquire)
                || state.widget_active.load(Ordering::Acquire)
            {
                return;
            }
            if window.is_minimized().unwrap_or(false) {
                if load_settings(app).is_ok_and(|settings| settings.min_to_tray)
                    && tray_available(app)
                {
                    queue_lifecycle(app, enter_background);
                }
                return;
            }
            // Fetch position before taking the DB lock: outer_position can
            // block on the event loop when requested from another thread.
            let position = window.outer_position().ok();
            if let Some(db) = app.try_state::<DbState>() {
                if let Ok(conn) = db.lock() {
                    let _ = settings::save_setting(&conn, "window_width", &size.width.to_string());
                    let _ =
                        settings::save_setting(&conn, "window_height", &size.height.to_string());
                    if let Some(pos) = position {
                        let _ = settings::save_setting(&conn, "window_x", &pos.x.to_string());
                        let _ = settings::save_setting(&conn, "window_y", &pos.y.to_string());
                    }
                }
            }
        }
        _ => {}
    }
}

fn should_prevent_exit(background: bool, code: Option<i32>) -> bool {
    background && code.is_none()
}

pub fn prevent_exit(app: &AppHandle, code: Option<i32>) -> bool {
    let background = app
        .try_state::<WindowState>()
        .is_some_and(|state| state.background.load(Ordering::Acquire));
    should_prevent_exit(background, code)
}

#[cfg(test)]
mod tests {
    use super::{should_prevent_exit, WindowState};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    #[test]
    fn concurrent_show_requests_serialize_window_creation() {
        let state = Arc::new(WindowState::default());
        let exists = Arc::new(AtomicBool::new(false));
        let builds = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(8));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let (state, exists, builds, start) = (
                Arc::clone(&state),
                Arc::clone(&exists),
                Arc::clone(&builds),
                Arc::clone(&start),
            );
            workers.push(std::thread::spawn(move || {
                start.wait();
                state
                    .with_lifecycle(|| {
                        if !exists.load(Ordering::Acquire) {
                            builds.fetch_add(1, Ordering::AcqRel);
                            std::thread::yield_now();
                            exists.store(true, Ordering::Release);
                        }
                        Ok(())
                    })
                    .unwrap();
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(builds.load(Ordering::Acquire), 1);
    }

    #[test]
    fn concurrent_show_and_destroy_completion_never_lose_reopen() {
        for _ in 0..32 {
            let state = Arc::new(WindowState::default());
            state.destroying.store(true, Ordering::Release);
            let reopens = Arc::new(AtomicUsize::new(0));
            let start = Arc::new(Barrier::new(2));
            let mut workers = Vec::new();
            for complete_destroy in [false, true] {
                let (state, reopens, start) =
                    (Arc::clone(&state), Arc::clone(&reopens), Arc::clone(&start));
                workers.push(std::thread::spawn(move || {
                    start.wait();
                    state
                        .with_lifecycle(|| {
                            let reopen = if complete_destroy {
                                state.finish_destroy()
                            } else {
                                !state.defer_show_until_destroyed()
                            };
                            if reopen {
                                reopens.fetch_add(1, Ordering::AcqRel);
                            }
                            Ok(())
                        })
                        .unwrap();
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
            assert_eq!(reopens.load(Ordering::Acquire), 1);
            assert!(!state.destroying.load(Ordering::Acquire));
            assert!(!state.reopen_after_destroy.load(Ordering::Acquire));
        }
    }

    #[test]
    fn show_during_destruction_is_replayed_once_after_destroyed() {
        let state = WindowState::default();
        assert!(!state.defer_show_until_destroyed());
        state.destroying.store(true, Ordering::Release);
        assert!(state.defer_show_until_destroyed());
        assert!(state.defer_show_until_destroyed());
        assert!(state.finish_destroy());
        assert!(!state.finish_destroy());
        assert!(!state.defer_show_until_destroyed());
    }

    #[test]
    fn last_window_exit_is_prevented_only_in_background() {
        assert!(should_prevent_exit(true, None));
        assert!(!should_prevent_exit(false, None));
    }

    #[test]
    fn explicit_exit_and_restart_always_finish() {
        for code in [0, 1, i32::MAX] {
            assert!(!should_prevent_exit(true, Some(code)));
            assert!(!should_prevent_exit(false, Some(code)));
        }
    }
}
