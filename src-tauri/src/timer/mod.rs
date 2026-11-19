pub mod engine;
pub mod sequence;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::{AudioCue, AudioManager};
use crate::db::{queries, wallet, DbState};
use crate::settings::Settings;
use crate::tray::{self, TrayState};
use crate::websocket::{self, WsState};

use engine::{EngineHandle, TimerCommand, TimerEvent};
use sequence::{RoundType, SequenceState};

// ---------------------------------------------------------------------------
// Snapshot — serialized to JSON for the frontend
// ---------------------------------------------------------------------------

/// Full timer state snapshot. Sent as the payload of Tauri events and
/// returned by the `timer_get_state` IPC command.
#[derive(Debug, Clone, Serialize)]
pub struct TimerSnapshot {
    /// "work" | "short-break" | "long-break"
    pub round_type: String,
    /// Round type that was active before this one. Empty string on the first round of a session.
    pub previous_round_type: String,
    pub elapsed_secs: u32,
    pub total_secs: u32,
    pub is_running: bool,
    /// True if the timer was paused or suspended, including before the first tick.
    pub is_paused: bool,
    pub work_round_number: u32,
    pub work_rounds_total: u32,
    /// Monotonically-increasing focus round count since last reset. Used as a
    /// session counter when long breaks are disabled.
    pub session_work_count: u32,
}

// ---------------------------------------------------------------------------
// Shared mutable state between the controller and the event-listener thread
// ---------------------------------------------------------------------------

#[derive(PartialEq, Eq)]
enum TimerPhase {
    Idle,
    Running,
    Paused,
}

impl TimerPhase {
    fn toggle_command(&self) -> TimerCommand {
        match self {
            Self::Running => {
                log::info!("[timer] pause");
                TimerCommand::Pause
            }
            Self::Paused => {
                log::info!("[timer] resume");
                TimerCommand::Resume
            }
            Self::Idle => {
                log::info!("[timer] start");
                TimerCommand::Start
            }
        }
    }
}

struct TimerShared {
    elapsed_secs: u32,
    // The active engine duration can differ from newly edited preferences.
    total_secs: u32,
    phase: TimerPhase,
    active_session_id: Option<i64>,
}

impl TimerShared {
    fn pause(&mut self, elapsed_secs: u32) {
        self.elapsed_secs = elapsed_secs;
        self.phase = TimerPhase::Paused;
    }

    fn set_idle(&mut self, total_secs: u32) {
        self.elapsed_secs = 0;
        self.total_secs = total_secs;
        self.phase = TimerPhase::Idle;
    }
}

// ---------------------------------------------------------------------------
// TimerController — public API registered as Tauri state
// ---------------------------------------------------------------------------

pub struct TimerController {
    engine: EngineHandle,
    sequence: Arc<Mutex<SequenceState>>,
    settings: Arc<Mutex<Settings>>,
    shared: Arc<Mutex<TimerShared>>,
    /// Kept alive so TrayState is not dropped if lib.rs forgets its copy.
    #[allow(dead_code)]
    tray: Arc<TrayState>,
}

impl TimerController {
    /// Construct and start the background threads.
    /// Call once from `lib.rs` during Tauri `setup`.
    pub fn new(
        app: AppHandle,
        settings: Settings,
        tray: Arc<TrayState>,
        db: DbState,
    ) -> Self {
        let seq = SequenceState::new(settings.long_break_interval);
        let duration = seq.current_duration_secs(&settings);

        let (engine, event_rx) = engine::spawn(duration, Duration::from_secs(1));

        let sequence = Arc::new(Mutex::new(seq));
        let settings_arc = Arc::new(Mutex::new(settings));
        let shared = Arc::new(Mutex::new(TimerShared {
            elapsed_secs: 0,
            total_secs: duration,
            phase: TimerPhase::Idle,
            active_session_id: None,
        }));

        // Clone handles for the event-listener thread.
        let seq_thread = Arc::clone(&sequence);
        let settings_thread = Arc::clone(&settings_arc);
        let shared_thread = Arc::clone(&shared);
        let engine_thread = engine.clone();
        let tray_thread = Arc::clone(&tray);

        std::thread::Builder::new()
            .name("timer-events".to_string())
            .spawn(move || {
                listen_events(
                    app,
                    event_rx,
                    ListenContext {
                        sequence: seq_thread,
                        settings: settings_thread,
                        shared: shared_thread,
                        engine: engine_thread,
                        tray: tray_thread,
                        db,
                    },
                );
            })
            .expect("failed to spawn timer event listener");

        Self {
            engine,
            sequence,
            settings: settings_arc,
            shared,
            tray,
        }
    }

    // --- Commands ---

    /// Toggle: start a fresh timer if idle, resume if paused, pause if running.
    pub fn toggle(&self) {
        let s = self.shared.lock().unwrap();
        self.send_command(s.phase.toggle_command());
    }

    pub(crate) fn send_command(&self, command: TimerCommand) {
        self.engine.send(command);
    }

    pub fn reset(&self) {
        log::info!("[timer] reset");
        self.sequence.lock().unwrap().reset();
        // Send only Reset — the event listener's Reset handler will follow up
        // with Prime once the engine is confirmed Idle. Sending a duration
        // update here first would race the Reset and can leave the UI stale.
        self.engine.send(TimerCommand::Reset);
    }

    /// Restart only the current round's timer without touching the sequence.
    /// Round type, round number, and position in the work/break cycle are all
    /// preserved — only the elapsed time is zeroed.
    pub fn restart_round(&self) {
        log::info!("[timer] restart round");
        self.engine.send(TimerCommand::Reset);
    }

    pub fn skip(&self) {
        log::info!("[timer] skip");
        self.engine.send(TimerCommand::Skip);
    }

    pub fn suspend(&self) {
        self.engine.send(TimerCommand::Suspend);
    }

    pub fn wake_resume(&self) {
        self.engine.send(TimerCommand::WakeResume);
    }

    /// Update the duration for the current round when settings change.
    /// Only takes effect after the next Start/Resume (current countdown is not interrupted).
    pub fn reconfigure(&self) {
        let duration = {
            let seq = self.sequence.lock().unwrap();
            let settings = self.settings.lock().unwrap();
            seq.current_duration_secs(&settings)
        };
        self.engine.send(TimerCommand::Reconfigure { duration_secs: duration });
    }

    // --- Query ---

    /// Read while holding the database lock when clearing session history.
    pub fn active_session_id(&self) -> Option<i64> {
        self.shared.lock().unwrap().active_session_id
    }

    pub fn get_snapshot(&self) -> TimerSnapshot {
        build_snapshot(&self.sequence, &self.shared)
    }

    /// Apply new settings values. Updates the in-memory copy and reconfigures
    /// an idle engine so the next Start uses the new duration.
    ///
    /// When the timer is running or paused, the current countdown is left
    /// untouched; the new duration takes effect at the start of the next
    /// round or after a manual reset.  Sending Reconfigure to a running
    /// engine transitions it to Idle, which would freeze the timer.
    pub fn apply_settings(&self, new: Settings) {
        // Sync work_rounds_total so the round counter and advance() logic both
        // reflect the new long_break_interval immediately.
        self.sequence.lock().unwrap().work_rounds_total = new.long_break_interval;
        *self.settings.lock().unwrap() = new;
        let duration = {
            let seq = self.sequence.lock().unwrap();
            let settings = self.settings.lock().unwrap();
            seq.current_duration_secs(&settings)
        };
        let mut s = self.shared.lock().unwrap();
        let is_idle = s.phase == TimerPhase::Idle;
        if is_idle {
            s.total_secs = duration;
        }
        drop(s);
        if is_idle {
            self.reconfigure();
        }
    }
}

// ---------------------------------------------------------------------------
// Background event listener thread
// ---------------------------------------------------------------------------

struct ListenContext {
    sequence: Arc<Mutex<SequenceState>>,
    settings: Arc<Mutex<Settings>>,
    shared: Arc<Mutex<TimerShared>>,
    engine: EngineHandle,
    tray: Arc<TrayState>,
    db: DbState,
}

fn listen_events(
    app: AppHandle,
    event_rx: std::sync::mpsc::Receiver<TimerEvent>,
    ctx: ListenContext,
) {
    let ListenContext { sequence, settings, shared, engine, tray, db } = ctx;
    // Track last tray progress to throttle redraws to ≥ 1% delta.
    let mut last_tray_progress: f32 = -1.0;
    // Active session row ID for recording (None = not started yet).
    let mut current_session_id: Option<i64> = None;
    let mut current_reward_rate: Option<i64> = None;

    while let Ok(event) = event_rx.recv() {
        match event {
            TimerEvent::Started { total_secs } => {
                log::info!("[timer] started total={total_secs}s");
                // Read once at Started. Pausing/resuming never changes this snapshot.
                current_reward_rate = match db.lock() {
                    Ok(conn) => match wallet::get_rate(&conn) {
                        Ok(rate) => rate,
                        Err(e) => {
                            log::error!("[wallet] failed to capture round rate; this round will not earn a reward: {e}");
                            let _ = app.emit("wallet:error", ());
                            None
                        }
                    },
                    Err(e) => {
                        log::error!("[wallet] failed to lock database for rate snapshot: {e}");
                        let _ = app.emit("wallet:error", ());
                        None
                    }
                };
                {
                    let mut state = shared.lock().unwrap();
                    state.phase = TimerPhase::Running;
                    state.total_secs = total_secs;
                }
                let _ = app.emit("timer:started", serde_json::json!({ "total_secs": total_secs }));
                if let Some(ws) = app.try_state::<Arc<WsState>>() {
                    websocket::broadcast_started(&ws, total_secs);
                }
                tray::update_menu_items(&tray, true, false);
            }

            TimerEvent::Tick { elapsed_secs, total_secs } => {
                {
                    let mut s = shared.lock().unwrap();
                    s.elapsed_secs = elapsed_secs;
                    s.total_secs = total_secs;
                    s.phase = TimerPhase::Running;
                }
                let _ = app.emit(
                    "timer:tick",
                    serde_json::json!({ "elapsed_secs": elapsed_secs, "total_secs": total_secs }),
                );

                // --- Session recording: start on first tick of a new round ---
                if elapsed_secs == 1 && current_session_id.is_none() {
                    let rt = sequence.lock().unwrap().current_round.as_str().to_string();
                    match db.lock() {
                        Ok(conn) => match queries::insert_session_with_reward_rate(
                            &conn, &rt, total_secs, current_reward_rate,
                        ) {
                            Ok(id) => {
                                current_session_id = Some(id);
                                shared.lock().unwrap().active_session_id = Some(id);
                            }
                            Err(e) => {
                                log::error!("[timer] failed to record session; no reward can be credited: {e}");
                                let _ = app.emit("wallet:error", ());
                            }
                        },
                        Err(e) => {
                            log::error!("[timer] failed to lock database to record session: {e}");
                            let _ = app.emit("wallet:error", ());
                        }
                    }
                }

                // --- Tick sound ---
                let rt = sequence.lock().unwrap().current_round.as_str().to_string();
                if let Some(audio) = app.try_state::<Arc<AudioManager>>() {
                    if audio.tick_enabled_for(&rt) {
                        audio.play_cue(AudioCue::Tick);
                    }
                }

                // Update tray arc — throttle to 1% visual change.
                let progress = if total_secs > 0 {
                    elapsed_secs as f32 / total_secs as f32
                } else {
                    0.0
                };
                if (progress - last_tray_progress).abs() >= 0.01 {
                    tray::update_icon(&tray, &rt, false, progress);
                    last_tray_progress = progress;
                }
            }

            TimerEvent::Complete { skipped: was_skipped } => {
                let completed_round = sequence.lock().unwrap().current_round.as_str().to_string();
                log::info!(
                    "[timer] round complete type={completed_round} skipped={was_skipped}"
                );

                // --- Session recording: mark the completed round ---
                if let Some(session_id) = current_session_id.take() {
                    match db.lock() {
                        Ok(conn) => match queries::complete_session(&conn, session_id, !was_skipped) {
                            Ok(true) => {
                                log::info!("[wallet] reward committed for session={session_id}");
                                let _ = app.emit("wallet:changed", ());
                            }
                            Ok(false) => {}
                            Err(e) => {
                                log::error!("[wallet] failed to finalize session={session_id}; completion and reward were rolled back: {e}");
                                let _ = app.emit("wallet:error", ());
                            }
                        },
                        Err(e) => {
                            log::error!("[wallet] failed to lock database to finalize session={session_id}: {e}");
                            let _ = app.emit("wallet:error", ());
                        }
                    }
                }
                current_reward_rate = None;
                shared.lock().unwrap().active_session_id = None;

                // Advance sequence.
                let (next_round, next_duration, auto_start_work, auto_start_break) = {
                    let mut seq = sequence.lock().unwrap();
                    let s = settings.lock().unwrap();
                    let (rt, dur) = seq.advance(&s);
                    (rt, dur, s.auto_start_work, s.auto_start_break)
                };

                // Reset shared state for the new round.
                shared.lock().unwrap().set_idle(next_duration);

                // Arm the next round's duration without risking a late
                // reconfigure that kicks a freshly-started timer back to Idle.
                engine.send(TimerCommand::Prime {
                    duration_secs: next_duration,
                });

                // Emit round-change with the new snapshot.
                let snapshot = build_snapshot(&sequence, &shared);
                let _ = app.emit("timer:round-change", snapshot);

                // Notify once from Rust even when all WebViews are destroyed.
                // Manual skips also notify, matching the previous UI behavior.
                let notification_settings = settings.lock().unwrap().clone();
                crate::notifications::show_round_change(
                    &app, &notification_settings, next_round.as_str(), &completed_round,
                );

                // Audio alert for the new round.
                if let Some(audio) = app.try_state::<Arc<AudioManager>>() {
                    let cue = match next_round {
                        RoundType::Work => AudioCue::WorkAlert,
                        RoundType::ShortBreak => AudioCue::ShortBreakAlert,
                        RoundType::LongBreak => AudioCue::LongBreakAlert,
                    };
                    audio.play_cue(cue);
                }

                // Lower-priority-during-breaks: when always_on_top is on and
                // break_always_on_top is enabled, disable always-on-top for
                // breaks and restore it when work resumes.
                let (always_on_top, break_always_on_top) = {
                    let s = settings.lock().unwrap();
                    (s.always_on_top, s.break_always_on_top)
                };
                if always_on_top {
                    if let Some(window) = app.get_webview_window("main") {
                        let is_break = next_round != RoundType::Work;
                        let _ = window.set_always_on_top(!(break_always_on_top && is_break));
                    }
                }

                // Update tray to reflect new round type and reset progress.
                // Use -1.0 (same as initialisation and Reset) so the very
                // first tick of the new round always passes the ≥1% threshold,
                // regardless of how long the round is.  Using 0.0 here caused
                // a ≥15-second blank period before the arc started animating.
                let rt = sequence.lock().unwrap().current_round.as_str().to_string();
                tray::update_icon(&tray, &rt, false, 0.0);
                last_tray_progress = -1.0;

                // Broadcast round-change to any connected WebSocket clients.
                if let Some(ws) = app.try_state::<Arc<WsState>>() {
                    let snap = build_snapshot(&sequence, &shared);
                    websocket::broadcast_round_change(&ws, snap);
                }

                // Auto-start if configured.
                let should_auto = match next_round {
                    RoundType::Work => auto_start_work,
                    _ => auto_start_break,
                };
                if should_auto {
                    log::debug!("[timer] auto-starting {}", next_round.as_str());
                    engine.send(TimerCommand::Start);
                } else {
                    // Timer is idle waiting for the user to start the new round.
                    // Reset the tray menu to "Start" so it doesn't keep showing
                    // "Pause" from the round that just completed.
                    tray::update_menu_items(&tray, false, false);
                }
            }

            TimerEvent::Paused { elapsed_secs } => {
                log::info!("[timer] paused elapsed={elapsed_secs}s");
                shared.lock().unwrap().pause(elapsed_secs);
                let _ = app.emit("timer:paused", serde_json::json!({ "elapsed_secs": elapsed_secs }));
                if let Some(ws) = app.try_state::<Arc<WsState>>() {
                    websocket::broadcast_paused(&ws, elapsed_secs);
                }

                // Show pause bars in tray.
                let rt = sequence.lock().unwrap().current_round.as_str().to_string();
                let total = shared.lock().unwrap().total_secs;
                let progress = if total > 0 { elapsed_secs as f32 / total as f32 } else { 0.0 };
                tray::update_icon(&tray, &rt, true, progress);
                tray::update_menu_items(&tray, false, true);
            }

            TimerEvent::Resumed { elapsed_secs } => {
                log::info!("[timer] resumed elapsed={elapsed_secs}s");
                shared.lock().unwrap().phase = TimerPhase::Running;
                let _ = app.emit("timer:resumed", serde_json::json!({ "elapsed_secs": elapsed_secs }));
                if let Some(ws) = app.try_state::<Arc<WsState>>() {
                    websocket::broadcast_resumed(&ws, elapsed_secs);
                }

                // Restore arc in tray.
                let rt = sequence.lock().unwrap().current_round.as_str().to_string();
                let total = shared.lock().unwrap().total_secs;
                let progress = if total > 0 { elapsed_secs as f32 / total as f32 } else { 0.0 };
                tray::update_icon(&tray, &rt, false, progress);
                last_tray_progress = progress;
                tray::update_menu_items(&tray, true, false);
            }

            TimerEvent::Reset => {
                log::debug!("[timer] idle");
                // Abandon the active session (leave DB row as-is).
                current_session_id = None;
                current_reward_rate = None;

                let duration = {
                    let seq = sequence.lock().unwrap();
                    let settings = settings.lock().unwrap();
                    seq.current_duration_secs(&settings)
                };

                {
                    let mut s = shared.lock().unwrap();
                    s.set_idle(duration);
                    s.active_session_id = None;
                }
                let snapshot = build_snapshot(&sequence, &shared);
                let _ = app.emit("timer:reset", snapshot);
                if let Some(ws) = app.try_state::<Arc<WsState>>() {
                    websocket::broadcast_reset(&ws);
                }

                // Prime the engine with the current round's duration so the
                // next Start uses the correct (possibly settings-updated)
                // total. Using the lighter-weight command here avoids a race
                // where a fast user click on Start is immediately clobbered by
                // a late follow-up duration update.
                engine.send(TimerCommand::Prime { duration_secs: duration });

                // Reset tray to idle (empty arc).
                let rt = sequence.lock().unwrap().current_round.as_str().to_string();
                tray::update_icon(&tray, &rt, false, 0.0);
                last_tray_progress = -1.0;
                tray::update_menu_items(&tray, false, false);
            }

            TimerEvent::Suspended { elapsed_secs } => {
                log::info!("[timer] suspended by system elapsed={elapsed_secs}s");
                shared.lock().unwrap().pause(elapsed_secs);
                let _ = app.emit(
                    "timer:suspended",
                    serde_json::json!({ "elapsed_secs": elapsed_secs }),
                );

                // Show pause bars while suspended.
                let rt = sequence.lock().unwrap().current_round.as_str().to_string();
                let total = shared.lock().unwrap().total_secs;
                let progress = if total > 0 { elapsed_secs as f32 / total as f32 } else { 0.0 };
                tray::update_icon(&tray, &rt, true, progress);
            }
        }
    }
}

fn build_snapshot(
    sequence: &Arc<Mutex<SequenceState>>,
    shared: &Arc<Mutex<TimerShared>>,
) -> TimerSnapshot {
    let seq = sequence.lock().unwrap();
    let sh = shared.lock().unwrap();

    TimerSnapshot {
        round_type: seq.current_round.as_str().to_string(),
        previous_round_type: seq.previous_round.map(|r| r.as_str().to_string()).unwrap_or_default(),
        elapsed_secs: sh.elapsed_secs,
        total_secs: sh.total_secs,
        is_running: sh.phase == TimerPhase::Running,
        is_paused: sh.phase == TimerPhase::Paused,
        work_round_number: seq.work_round_number,
        work_rounds_total: seq.work_rounds_total,
        session_work_count: seq.session_work_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_elapsed_pause_can_resume_and_reset() {
        let sequence = Arc::new(Mutex::new(SequenceState::new(4)));
        let duration = 1500;
        let (engine, events) = engine::spawn(duration, Duration::from_secs(60));
        let shared = Arc::new(Mutex::new(TimerShared {
            elapsed_secs: 0,
            total_secs: duration,
            phase: TimerPhase::Idle,
            active_session_id: None,
        }));
        let toggle = || engine.send(shared.lock().unwrap().phase.toggle_command());
        let next_event = || events.recv_timeout(Duration::from_secs(2)).unwrap();

        toggle();
        assert!(matches!(next_event(), TimerEvent::Started { .. }));
        shared.lock().unwrap().phase = TimerPhase::Running;

        toggle();
        let TimerEvent::Paused { elapsed_secs } = next_event() else {
            panic!("expected a pause event");
        };
        shared.lock().unwrap().pause(elapsed_secs);
        let paused = build_snapshot(&sequence, &shared);
        assert_eq!(paused.elapsed_secs, 0);
        assert!(paused.is_paused);
        assert!(!paused.is_running);

        toggle();
        assert!(matches!(next_event(), TimerEvent::Resumed { elapsed_secs: 0 }));
        shared.lock().unwrap().phase = TimerPhase::Running;

        toggle();
        let TimerEvent::Paused { elapsed_secs } = next_event() else {
            panic!("expected a pause event");
        };
        shared.lock().unwrap().pause(elapsed_secs);

        engine.send(TimerCommand::Reset);
        assert!(matches!(next_event(), TimerEvent::Reset));
        shared.lock().unwrap().set_idle(duration);
        let reset = build_snapshot(&sequence, &shared);
        assert_eq!(reset.elapsed_secs, 0);
        assert!(!reset.is_paused);
        assert!(!reset.is_running);
        toggle();
        assert!(matches!(next_event(), TimerEvent::Started { .. }));
    }
}
