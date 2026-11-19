use std::{sync::Arc, time::Duration};

use serde::Deserialize;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Rect, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tokio::{sync::mpsc, time::Instant};

use super::TrayState;
use crate::timer::{engine::TimerCommand, TimerController};

const LABEL_PREFIX: &str = "tray-panel-";
const WIDTH: f64 = 252.0;
const GAP: f64 = 8.0;
const READY_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Start,
    Pause,
    Resume,
    Skip,
    ResetRound,
    Show,
    Exit,
    Dismiss,
}

enum Request {
    Open(Rect),
    Ready(String, f64),
    Action(String, Action),
    Blur(String),
    Destroyed(String),
    Dismiss,
}

struct PanelQueue(mpsc::UnboundedSender<Request>);

struct Panel {
    window: WebviewWindow,
    anchor: Rect,
    deadline: Option<Instant>,
}

fn is_panel(label: &str) -> bool {
    label.starts_with(LABEL_PREFIX)
}

pub(crate) fn start(app: &AppHandle) {
    let (sender, receiver) = mpsc::unbounded_channel();
    app.manage(PanelQueue(sender));
    tauri::async_runtime::spawn(run(app.clone(), receiver));
}

pub(super) fn open(app: &AppHandle, rect: Rect) {
    let _ = send(app, Request::Open(rect));
}

pub(crate) fn dismiss(app: &AppHandle) {
    let _ = send(app, Request::Dismiss);
}

fn send(app: &AppHandle, request: Request) -> Result<(), String> {
    app.state::<PanelQueue>()
        .0
        .send(request)
        .map_err(|_| "tray panel is unavailable".into())
}

fn send_from_window(window: WebviewWindow, request: Request) -> Result<(), String> {
    if !is_panel(window.label()) {
        return Err("this command requires the tray panel window".into());
    }
    send(window.app_handle(), request)
}

#[tauri::command]
pub fn tray_panel_ready(window: WebviewWindow, height: f64) -> Result<(), String> {
    if !height.is_finite() || height <= 0.0 {
        return Err("invalid tray panel height".into());
    }
    let label = window.label().to_owned();
    send_from_window(window, Request::Ready(label, height))
}

#[tauri::command]
pub fn tray_panel_action(window: WebviewWindow, action: Action) -> Result<(), String> {
    let label = window.label().to_owned();
    send_from_window(window, Request::Action(label, action))
}

async fn run(app: AppHandle, mut requests: mpsc::UnboundedReceiver<Request>) {
    let mut panel: Option<Panel> = None;
    let mut sequence = 0_u64;

    loop {
        let request = match panel.as_ref().and_then(|current| current.deadline) {
            Some(deadline) => match tokio::time::timeout_at(deadline, requests.recv()).await {
                Ok(request) => request,
                Err(_) => {
                    log::warn!("[tray] panel did not signal readiness");
                    close(&mut panel);
                    continue;
                }
            },
            None => requests.recv().await,
        };

        match request {
            Some(Request::Open(anchor)) if available(&app) => {
                if let Some(current) = panel.as_mut().filter(|current| current.deadline.is_some()) {
                    current.anchor = anchor;
                    continue;
                }
                close(&mut panel);
                sequence += 1;
                let handle = app.clone();
                // Wry cannot create WebView2 synchronously inside a Windows event callback.
                match tauri::async_runtime::spawn_blocking(move || build(&handle, sequence)).await {
                    Ok(Ok(window)) => {
                        panel = Some(Panel {
                            window,
                            anchor,
                            deadline: Some(Instant::now() + READY_TIMEOUT),
                        });
                    }
                    Ok(Err(error)) => log::warn!("[tray] could not create panel: {error}"),
                    Err(error) => log::warn!("[tray] panel worker failed: {error}"),
                }
            }
            Some(Request::Ready(label, height)) => {
                let Some(current) = panel
                    .as_mut()
                    .filter(|current| current.window.label() == label)
                else {
                    continue;
                };
                if available(&app) {
                    match show(current, height) {
                        Ok(()) => {
                            current.deadline = None;
                            continue;
                        }
                        Err(error) => log::warn!("[tray] could not show panel: {error}"),
                    }
                }
                close(&mut panel);
            }
            Some(Request::Action(label, action)) => {
                if panel
                    .as_ref()
                    .is_some_and(|current| current.window.label() == label)
                {
                    let ready = panel
                        .as_ref()
                        .is_some_and(|current| current.deadline.is_none());
                    close(&mut panel);
                    if ready && available(&app) {
                        dispatch(&app, action);
                    }
                }
            }
            Some(Request::Blur(label)) => {
                if panel.as_ref().is_some_and(|current| {
                    current.window.label() == label && current.deadline.is_none()
                }) {
                    close(&mut panel);
                }
            }
            Some(Request::Destroyed(label)) => {
                if panel
                    .as_ref()
                    .is_some_and(|current| current.window.label() == label)
                {
                    panel = None;
                }
            }
            Some(Request::Dismiss) => close(&mut panel),
            Some(Request::Open(_)) => {}
            None => break,
        }
    }
}

fn available(app: &AppHandle) -> bool {
    app.state::<Arc<TrayState>>()
        .available
        .load(std::sync::atomic::Ordering::Acquire)
}

fn build(app: &AppHandle, sequence: u64) -> tauri::Result<WebviewWindow> {
    // The label is unique to this WebView so late events cannot affect its replacement.
    let label = format!("{LABEL_PREFIX}{sequence}");
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("tray".into()))
        .title("小番茄")
        .inner_size(WIDTH, 320.0)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .transparent(true)
        .background_color(tauri::window::Color(0, 0, 0, 0))
        .build()?;

    let app = app.clone();
    window.on_window_event(move |event| {
        let request = match event {
            WindowEvent::Focused(false) => Request::Blur(label.clone()),
            WindowEvent::Destroyed => Request::Destroyed(label.clone()),
            _ => return,
        };
        let _ = send(&app, request);
    });
    Ok(window)
}

fn show(panel: &Panel, height: f64) -> Result<(), String> {
    let window = &panel.window;
    let icon_position = panel.anchor.position.to_physical::<f64>(1.0);
    let icon_size = panel.anchor.size.to_physical::<f64>(1.0);
    let icon = Bounds::new(
        icon_position.x,
        icon_position.y,
        icon_size.width,
        icon_size.height,
    );
    let (x, y) = icon.center();
    let app = window.app_handle();
    let monitor = app
        .monitor_from_point(x, y)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "could not locate the tray monitor".to_owned())?;
    let work = monitor.work_area();
    let work = Bounds::new(
        f64::from(work.position.x),
        f64::from(work.position.y),
        f64::from(work.size.width),
        f64::from(work.size.height),
    );
    let display = Bounds::new(
        f64::from(monitor.position().x),
        f64::from(monitor.position().y),
        f64::from(monitor.size().width),
        f64::from(monitor.size().height),
    );
    let (position, size) = place(icon, work, display, monitor.scale_factor(), height);
    window
        .set_position(position)
        .and_then(|()| window.set_size(size))
        .map_err(|error| error.to_string())?;

    // Windows places the outer frame, while the requested size and anchor
    // describe the client. Read the frame insets after moving to the target DPI.
    let outer = window.outer_position().map_err(|error| error.to_string())?;
    let inner = window.inner_position().map_err(|error| error.to_string())?;
    let corrected = PhysicalPosition::new(
        position.x + outer.x - inner.x,
        position.y + outer.y - inner.y,
    );
    if corrected != outer {
        window
            .set_position(corrected)
            .map_err(|error| error.to_string())?;
    }

    if panel.deadline.is_some() {
        window.show().map_err(|error| error.to_string())?;
        if !window.is_visible().map_err(|error| error.to_string())? {
            return Err("tray panel window could not be shown".into());
        }
        window.set_focus().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn close(panel: &mut Option<Panel>) {
    if let Some(panel) = panel.take() {
        let _ = panel.window.hide();
        if let Err(error) = panel.window.destroy() {
            log::warn!("[tray] could not destroy panel: {error}");
        }
    }
}

fn dispatch(app: &AppHandle, action: Action) {
    match action {
        Action::Start => app.state::<TimerController>().send_command(TimerCommand::Start),
        Action::Pause => app.state::<TimerController>().send_command(TimerCommand::Pause),
        Action::Resume => app.state::<TimerController>().send_command(TimerCommand::Resume),
        Action::Skip | Action::ResetRound => {
            let timer = app.state::<TimerController>();
            let state = timer.get_snapshot();
            if state.is_running || state.is_paused {
                if matches!(action, Action::Skip) {
                    timer.skip();
                } else {
                    timer.restart_round();
                }
            }
        }
        Action::Show => crate::windows::request_show(app),
        Action::Exit => app.exit(0),
        Action::Dismiss => {}
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Bounds {
    fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        }
    }

    fn center(self) -> (f64, f64) {
        (
            (self.left + self.right) / 2.0,
            (self.top + self.bottom) / 2.0,
        )
    }

    fn insets(self, outer: Self) -> [f64; 4] {
        [
            self.bottom - outer.bottom,
            outer.top - self.top,
            outer.left - self.left,
            self.right - outer.right,
        ]
    }
}

fn place(
    icon: Bounds,
    work: Bounds,
    display: Bounds,
    scale: f64,
    height: f64,
) -> (PhysicalPosition<i32>, PhysicalSize<u32>) {
    let width = (WIDTH * scale).round().max(1.0).min(work.right - work.left);
    let height = (height * scale).ceil().max(1.0).min(work.bottom - work.top);
    let gap = (GAP * scale).round();
    let (x, y) = icon.center();
    // Prefer the edge of the tray. An overflow icon sits inside the work area, so
    // use the taskbar inset when present and the nearest edge for auto-hide bars.
    let distances = work.insets(icon);
    let reserved = work.insets(display);
    let side = (0..4)
        .min_by(|&a, &b| {
            let rank = |i: usize| (distances[i] >= 0.0, reserved[i] >= 0.0);
            rank(a)
                .cmp(&rank(b))
                .then_with(|| distances[a].total_cmp(&distances[b]))
        })
        .unwrap();
    let candidates = [
        (x - width / 2.0, icon.top - height - gap),
        (x - width / 2.0, icon.bottom + gap),
        (icon.right + gap, y - height / 2.0),
        (icon.left - width - gap, y - height / 2.0),
    ];
    let (mut x, mut y) = candidates[side];
    let opposite = [1, 0, 3, 2][side];
    let (other_x, other_y) = candidates[opposite];
    if side < 2 {
        if (y < work.top || y + height > work.bottom)
            && (work.top..=work.bottom - height).contains(&other_y)
        {
            y = other_y;
        }
    } else if (x < work.left || x + width > work.right)
        && (work.left..=work.right - width).contains(&other_x)
    {
        x = other_x;
    }
    let x = x.clamp(work.left, work.right - width).round() as i32;
    let y = y.clamp(work.top, work.bottom - height).round() as i32;
    (
        PhysicalPosition::new(x, y),
        PhysicalSize::new(width as u32, height as u32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_opens_inward_for_each_taskbar_edge() {
        let display = Bounds::new(0.0, 0.0, 1920.0, 1080.0);
        for (icon, work, expected) in [
            (
                Bounds::new(1840.0, 1048.0, 24.0, 24.0),
                Bounds::new(0.0, 0.0, 1920.0, 1040.0),
                (1668, 820),
            ),
            (
                Bounds::new(1840.0, 8.0, 24.0, 24.0),
                Bounds::new(0.0, 40.0, 1920.0, 1040.0),
                (1668, 40),
            ),
            (
                Bounds::new(8.0, 1000.0, 24.0, 24.0),
                Bounds::new(40.0, 0.0, 1880.0, 1080.0),
                (40, 860),
            ),
            (
                Bounds::new(1888.0, 1000.0, 24.0, 24.0),
                Bounds::new(0.0, 0.0, 1880.0, 1080.0),
                (1628, 860),
            ),
        ] {
            let (position, size) = place(icon, work, display, 1.0, 220.0);
            assert_eq!(position, expected.into());
            assert_eq!(size, (252, 220).into());
        }
    }

    #[test]
    fn overflow_icon_uses_its_monitors_taskbar_inset_and_scale() {
        let display = Bounds::new(-1920.0, -200.0, 1920.0, 1080.0);
        let work = Bounds::new(-1920.0, -200.0, 1920.0, 1020.0);
        let icon = Bounds::new(-62.0, 680.0, 28.0, 28.0);
        let (position, size) = place(icon, work, display, 1.5, 200.5);
        assert_eq!(position, (-378, 367).into());
        assert_eq!(size, (378, 301).into());
    }

    #[test]
    fn overflow_panel_flips_when_the_preferred_side_does_not_fit() {
        let display = Bounds::new(0.0, 0.0, 960.0, 540.0);
        let work = Bounds::new(0.0, 0.0, 960.0, 500.0);
        let icon = Bounds::new(850.0, 120.0, 24.0, 24.0);
        let (position, _) = place(icon, work, display, 1.0, 220.0);
        assert_eq!(position, (708, 152).into());
    }

    #[test]
    fn auto_hidden_taskbar_uses_the_nearest_edge() {
        let work = Bounds::new(0.0, 0.0, 1920.0, 1080.0);
        let icon = Bounds::new(1886.0, 600.0, 26.0, 24.0);
        let (position, _) = place(icon, work, work, 1.0, 240.0);
        assert_eq!(position, (1626, 492).into());
    }
}
