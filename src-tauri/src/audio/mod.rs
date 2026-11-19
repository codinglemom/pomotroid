/// Rust audio playback via rodio.
///
/// Built-in audio assets are embedded at compile time so they are available
/// even when the app is hidden to the system tray.
///
/// Architecture:
///   A dedicated OS thread plays timer sounds received via a `SyncSender`.
///   Previews open their own output stream on a blocking worker.
///   Tauri manages `Arc<AudioManager>` as state; it is `Send + Sync`.
///
/// Custom audio:
///   Per-cue custom files are stored in `{app_data_dir}/audio/`. The audio
///   thread falls back to the classic sound if the custom file cannot play.
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::{Decoder, DeviceSinkBuilder, Player, Source};
use rusqlite::Connection;

use crate::settings::{self, Settings};

// ---------------------------------------------------------------------------
// Embedded audio assets
// ---------------------------------------------------------------------------

const ALERT_WORK: &[u8] = include_bytes!("../../../static/audio/alert-work.mp3");
const ALERT_SHORT_BREAK: &[u8] = include_bytes!("../../../static/audio/alert-short-break.mp3");
const ALERT_LONG_BREAK: &[u8] = include_bytes!("../../../static/audio/alert-long-break.mp3");
const TICK: &[u8] = include_bytes!("../../../static/audio/tick.mp3");
const SOFT_BELL: &[u8] = include_bytes!("../../../static/audio/soft-bell.wav");
const MARIMBA: &[u8] = include_bytes!("../../../static/audio/marimba.wav");
const DROPLET: &[u8] = include_bytes!("../../../static/audio/droplet.wav");
const PREVIEW_DURATION: Duration = Duration::from_secs(3);

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum AudioCue {
    WorkAlert,
    ShortBreakAlert,
    LongBreakAlert,
    Tick,
}

#[derive(Debug, Clone, Copy)]
pub enum AlertCue {
    Work,
    ShortBreak,
    LongBreak,
}

impl AlertCue {
    const ALL: [Self; 3] = [Self::Work, Self::ShortBreak, Self::LongBreak];

    fn stem(self) -> &'static str {
        match self {
            Self::Work => "custom_work_alert",
            Self::ShortBreak => "custom_short_break_alert",
            Self::LongBreak => "custom_long_break_alert",
        }
    }

    fn preset_key(self) -> String {
        format!("{}_preset", self.stem())
    }

    fn name_key(self) -> String {
        format!("{}_name", self.stem())
    }

    fn file_key(self) -> String {
        format!("{}_file", self.stem())
    }

    fn file_names(self) -> [String; 2] {
        [format!("{}_a", self.stem()), format!("{}_b", self.stem())]
    }
}

impl std::str::FromStr for AlertCue {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "work_alert" => Ok(Self::Work),
            "short_break_alert" => Ok(Self::ShortBreak),
            "long_break_alert" => Ok(Self::LongBreak),
            _ => Err(format!("unknown audio cue: {value}")),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioPreset {
    #[default]
    Classic,
    SoftBell,
    Marimba,
    Droplet,
    Custom,
}

impl std::str::FromStr for AudioPreset {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "classic" => Ok(Self::Classic),
            "soft-bell" => Ok(Self::SoftBell),
            "marimba" => Ok(Self::Marimba),
            "droplet" => Ok(Self::Droplet),
            "custom" => Ok(Self::Custom),
            _ => Err(format!("unknown audio preset: {value}")),
        }
    }
}

impl AudioPreset {
    fn as_str(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::SoftBell => "soft-bell",
            Self::Marimba => "marimba",
            Self::Droplet => "droplet",
            Self::Custom => "custom",
        }
    }

    fn embedded_for(self, cue: AudioCue) -> &'static [u8] {
        match self {
            Self::SoftBell => SOFT_BELL,
            Self::Marimba => MARIMBA,
            Self::Droplet => DROPLET,
            Self::Classic | Self::Custom => match cue {
                AudioCue::WorkAlert => ALERT_WORK,
                AudioCue::ShortBreakAlert => ALERT_SHORT_BREAK,
                AudioCue::LongBreakAlert => ALERT_LONG_BREAK,
                AudioCue::Tick => TICK,
            },
        }
    }
}

#[derive(Default)]
struct AlertSound {
    preset: AudioPreset,
    custom: Option<CustomSound>,
}

struct CustomSound {
    path: PathBuf,
    name: String,
}

/// The selected preset and retained import for each alert cue.
#[derive(Default)]
pub struct AlertSounds([AlertSound; 3]);

#[derive(serde::Serialize)]
pub struct AudioPresets {
    pub work_alert: AudioPreset,
    pub short_break_alert: AudioPreset,
    pub long_break_alert: AudioPreset,
}

/// Selected presets and the names of any retained custom files.
#[derive(serde::Serialize)]
pub struct CustomAudioInfo {
    pub work_alert: Option<String>,
    pub short_break_alert: Option<String>,
    pub long_break_alert: Option<String>,
    pub presets: AudioPresets,
}

struct PlayRequest {
    cue: AudioCue,
    preset: AudioPreset,
    custom_path: Option<PathBuf>,
    volume: f32,
    /// When the request was made; used to skip ticks that are already late.
    requested_at: Instant,
}

/// Settings subset relevant to the audio engine.
#[derive(Clone)]
struct AudioSettings {
    volume: f32,
    tick_sounds_work: bool,
    tick_sounds_break: bool,
}

impl From<&Settings> for AudioSettings {
    fn from(s: &Settings) -> Self {
        Self {
            volume: s.volume,
            tick_sounds_work: s.tick_sounds_during_work,
            tick_sounds_break: s.tick_sounds_during_break,
        }
    }
}

/// Thread-safe audio manager. Register as `Arc<AudioManager>` in Tauri state.
pub struct AudioManager {
    tx: mpsc::SyncSender<PlayRequest>,
    settings: Mutex<AudioSettings>,
    pub alerts: Mutex<AlertSounds>,
}

impl AudioManager {
    /// Spawn the audio thread and return an `Arc<AudioManager>`.
    pub fn new(initial: &Settings, alerts: AlertSounds) -> Arc<Self> {
        let (tx, rx) = mpsc::sync_channel::<PlayRequest>(8);

        if let Err(e) = std::thread::Builder::new()
            .name("audio".to_string())
            .spawn(move || audio_thread(rx))
        {
            log::warn!("[audio] failed to start audio thread: {e}");
        }

        log::info!("[audio] initialized: volume={:.0}%", initial.volume * 100.0);
        Arc::new(Self {
            tx,
            settings: Mutex::new(AudioSettings::from(initial)),
            alerts: Mutex::new(alerts),
        })
    }

    /// Update volume and tick-sound settings from a new `Settings` snapshot.
    pub fn apply_settings(&self, s: &Settings) {
        *self.settings.lock().unwrap() = AudioSettings::from(s);
    }

    /// Play the given cue at the current stored volume.
    /// Non-blocking: drops the request if the channel is full.
    pub fn play_cue(&self, cue: AudioCue) {
        if let Some(req) = self.play_request(cue) {
            let _ = self.tx.try_send(req);
        }
    }

    /// Play up to three seconds of an alert independently of the timer audio queue.
    pub async fn preview(&self, cue: AlertCue) -> Result<(), String> {
        self.preview_with(cue, |req| play(req, Some(PREVIEW_DURATION)))
            .await
    }

    async fn preview_with(
        &self,
        cue: AlertCue,
        playback: impl FnOnce(PlayRequest) -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        let cue = match cue {
            AlertCue::Work => AudioCue::WorkAlert,
            AlertCue::ShortBreak => AudioCue::ShortBreakAlert,
            AlertCue::LongBreak => AudioCue::LongBreakAlert,
        };
        let Some(req) = self.play_request(cue) else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || playback(req))
            .await
            .map_err(|e| format!("audio preview worker failed: {e}"))?
    }

    fn play_request(&self, cue: AudioCue) -> Option<PlayRequest> {
        let volume = self.settings.lock().unwrap().volume;
        if volume <= 0.0 {
            return None;
        }
        let alert = match cue {
            AudioCue::WorkAlert => Some(AlertCue::Work),
            AudioCue::ShortBreakAlert => Some(AlertCue::ShortBreak),
            AudioCue::LongBreakAlert => Some(AlertCue::LongBreak),
            AudioCue::Tick => None,
        };
        let (preset, custom_path) = alert
            .map(|alert| {
                let sounds = self.alerts.lock().unwrap();
                let sound = &sounds.0[alert as usize];
                let custom_path = sound.custom.as_ref().and_then(|custom| {
                    (sound.preset == AudioPreset::Custom).then(|| custom.path.clone())
                });
                (sound.preset, custom_path)
            })
            .unwrap_or_default();
        Some(PlayRequest {
            cue,
            preset,
            custom_path,
            volume,
            requested_at: Instant::now(),
        })
    }

    /// Returns true if tick sounds are enabled for the given round type string.
    pub fn tick_enabled_for(&self, round_type: &str) -> bool {
        let s = self.settings.lock().unwrap();
        match round_type {
            "work" => s.tick_sounds_work,
            _ => s.tick_sounds_break,
        }
    }
}

impl AlertSounds {
    pub fn load(conn: &Connection, dir: &Path) -> Self {
        Self(AlertCue::ALL.map(|cue| {
            let preset = settings::get_setting(conn, &cue.preset_key());
            let name = settings::get_setting(conn, &cue.name_key());
            let [first, second] = cue.file_names();
            let path = match settings::get_setting(conn, &cue.file_key()) {
                Some(file) if file == first || file == second => Some(dir.join(file)),
                None if preset.is_none() || name.is_some() => find_legacy_file(dir, cue),
                _ => None,
            };
            let custom = path.filter(|path| path.is_file()).map(|path| CustomSound {
                name: name.unwrap_or_else(|| {
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                }),
                path,
            });
            let preset = preset.as_deref().map_or_else(
                || {
                    if custom.is_some() {
                        AudioPreset::Custom
                    } else {
                        AudioPreset::Classic
                    }
                },
                |value| value.parse().unwrap_or_default(),
            );
            let preset = match (preset, &custom) {
                (AudioPreset::Custom, None) => AudioPreset::Classic,
                _ => preset,
            };
            AlertSound { preset, custom }
        }))
    }

    pub fn info(&self) -> CustomAudioInfo {
        let [work, short, long] = &self.0;
        let name = |sound: &AlertSound| sound.custom.as_ref().map(|custom| custom.name.clone());
        CustomAudioInfo {
            work_alert: name(work),
            short_break_alert: name(short),
            long_break_alert: name(long),
            presets: AudioPresets {
                work_alert: work.preset,
                short_break_alert: short.preset,
                long_break_alert: long.preset,
            },
        }
    }

    pub fn set_preset(
        &mut self,
        conn: &mut Connection,
        cue: AlertCue,
        preset: AudioPreset,
    ) -> Result<(), String> {
        let sound = &mut self.0[cue as usize];
        if preset == AudioPreset::Custom
            && !sound
                .custom
                .as_ref()
                .is_some_and(|custom| custom.path.is_file())
        {
            return Err("no custom file available for this cue".into());
        }
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        if let Some(custom) = &sound.custom {
            settings::save_setting(&tx, &cue.name_key(), &custom.name)
                .map_err(|e| e.to_string())?;
        }
        settings::save_setting(&tx, &cue.preset_key(), preset.as_str())
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        sound.preset = preset;
        Ok(())
    }

    pub fn import(
        &mut self,
        conn: &mut Connection,
        dir: &Path,
        cue: AlertCue,
        src: &Path,
    ) -> Result<String, String> {
        let sound = &mut self.0[cue as usize];
        let name = src
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let [first, second] = cue.file_names();
        // Copy to the inactive slot so the previous import survives validation and DB failures.
        let file = if sound
            .custom
            .as_ref()
            .is_some_and(|custom| custom.path.ends_with(&first))
        {
            second
        } else {
            first
        };
        let path = dir.join(&file);
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        if src
            .canonicalize()
            .ok()
            .is_some_and(|src| Some(src) == path.canonicalize().ok())
        {
            return Err("source file already belongs to this alert".into());
        }
        let result = (|| {
            std::fs::copy(src, &path).map_err(|e| e.to_string())?;
            probe_audio_file(&path)?;

            let tx = conn.transaction().map_err(|e| e.to_string())?;
            settings::save_setting(&tx, &cue.name_key(), &name).map_err(|e| e.to_string())?;
            settings::save_setting(&tx, &cue.file_key(), &file).map_err(|e| e.to_string())?;
            settings::save_setting(&tx, &cue.preset_key(), AudioPreset::Custom.as_str())
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())
        })();
        if let Err(error) = result {
            remove_audio_file(&path);
            return Err(error);
        }
        remove_unused_files(dir, cue, Some(&path));
        *sound = AlertSound {
            preset: AudioPreset::Custom,
            custom: Some(CustomSound {
                path,
                name: name.clone(),
            }),
        };
        Ok(name)
    }

    pub fn clear(
        &mut self,
        conn: &mut Connection,
        dir: &Path,
        cue: AlertCue,
    ) -> Result<(), String> {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM settings WHERE key IN (?1, ?2)",
            rusqlite::params![cue.name_key(), cue.file_key()],
        )
        .map_err(|e| e.to_string())?;
        settings::save_setting(&tx, &cue.preset_key(), AudioPreset::Classic.as_str())
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.0[cue as usize] = AlertSound::default();
        remove_unused_files(dir, cue, None);
        Ok(())
    }

    /// Explicit defaults keep an undeletable legacy file from reappearing after a reset.
    pub fn save_classic_presets(conn: &Connection) -> rusqlite::Result<()> {
        for cue in AlertCue::ALL {
            settings::save_setting(conn, &cue.preset_key(), AudioPreset::Classic.as_str())?;
        }
        Ok(())
    }

    pub fn reset(&mut self, dir: &Path) {
        *self = Self::default();
        for cue in AlertCue::ALL {
            remove_unused_files(dir, cue, None);
        }
    }
}

fn find_legacy_file(dir: &Path, cue: AlertCue) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.is_file() && path.file_stem().and_then(|s| s.to_str()) == Some(cue.stem())
        })
}

fn remove_unused_files(dir: &Path, cue: AlertCue, keep: Option<&Path>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        let [first, second] = cue.file_names();
        for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
            let legacy = path.file_stem().and_then(|stem| stem.to_str()) == Some(cue.stem());
            let managed = path.ends_with(&first) || path.ends_with(&second);
            if (legacy || managed) && Some(path.as_path()) != keep && path.is_file() {
                remove_audio_file(&path);
            }
        }
    }
}

fn remove_audio_file(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("[audio] could not remove {path:?}: {e}");
        }
    }
}

/// Probe `path` by attempting to construct a `rodio::Decoder`.
/// Returns `Ok(())` if the format and codec are recognised, or an error
/// string suitable for returning to the frontend.
fn probe_audio_file(path: &Path) -> Result<(), String> {
    let reader = std::fs::File::open(path)
        .map(std::io::BufReader::new)
        .map_err(|e| format!("cannot open file: {e}"))?;
    Decoder::new(reader)
        .map(|_| ())
        .map_err(|e| format!("audio format not supported: {e}"))
}

// ---------------------------------------------------------------------------
// Audio thread
// ---------------------------------------------------------------------------

fn audio_thread(rx: mpsc::Receiver<PlayRequest>) {
    // Open the audio device fresh for each request rather than holding a
    // single stream open indefinitely. This lets the audio thread recover
    // automatically after a sleep/wake cycle that resets the OS audio
    // subsystem, avoiding a flood of "buffer underrun/overrun" errors.
    while let Ok(req) = rx.recv() {
        // Sounds play one at a time, so ticks requested during a long alert
        // wait behind it and would then play back-to-back. Skip late ticks.
        if matches!(req.cue, AudioCue::Tick)
            && req.requested_at.elapsed() > Duration::from_millis(500)
        {
            continue;
        }

        if let Err(e) = play(req, None) {
            log::warn!("[audio] {e}");
        }
    }
}

fn play(req: PlayRequest, limit: Option<Duration>) -> Result<(), String> {
    let mut device_sink = DeviceSinkBuilder::open_default_sink()
        .map_err(|e| format!("cannot open audio output: {e}"))?;
    device_sink.log_on_drop(false);

    let player = Player::connect_new(device_sink.mixer());
    player.set_volume(req.volume);
    let source = decode_sound(&req)?;
    if let Some(limit) = limit {
        player.append(source.take_duration(limit));
    } else {
        player.append(source);
    }
    player.sleep_until_end();
    Ok(())
}

fn decode_sound(req: &PlayRequest) -> Result<Box<dyn Source + Send>, String> {
    if let Some(path) = &req.custom_path {
        match std::fs::File::open(path).map(std::io::BufReader::new) {
            Ok(reader) => match Decoder::new(reader) {
                Ok(source) => return Ok(Box::new(source)),
                Err(e) => log::warn!("[audio] decode error for {path:?}: {e}"),
            },
            Err(e) => log::warn!("[audio] cannot open {path:?}: {e}"),
        }
    }

    let bytes = req.preset.embedded_for(req.cue);
    let source =
        Decoder::new(Cursor::new(bytes)).map_err(|e| format!("embedded decode error: {e}"))?;
    Ok(Box::new(source))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_presets_decode() {
        for bytes in [
            ALERT_WORK,
            ALERT_SHORT_BREAK,
            ALERT_LONG_BREAK,
            TICK,
            SOFT_BELL,
            MARIMBA,
            DROPLET,
        ] {
            assert!(Decoder::new(Cursor::new(bytes)).unwrap().next().is_some());
        }
    }

    #[test]
    fn tick_enabled_for_round_types() {
        let settings = Settings {
            tick_sounds_during_work: true,
            tick_sounds_during_break: false,
            ..Settings::default()
        };
        let mgr = AudioManager {
            tx: mpsc::sync_channel(1).0,
            settings: Mutex::new(AudioSettings::from(&settings)),
            alerts: Mutex::new(AlertSounds::default()),
        };
        assert!(mgr.tick_enabled_for("work"));
        assert!(!mgr.tick_enabled_for("short-break"));
        assert!(!mgr.tick_enabled_for("long-break"));
    }

    #[test]
    fn preview_shares_sound_settings_and_leaves_room_for_a_timer_alert() {
        let settings = Settings {
            volume: 0.4,
            ..Settings::default()
        };
        let (tx, rx) = mpsc::sync_channel(2);
        let mgr = Arc::new(AudioManager {
            tx,
            settings: Mutex::new(AudioSettings::from(&settings)),
            alerts: Mutex::new(AlertSounds::default()),
        });
        let path = PathBuf::from("custom.wav");
        mgr.alerts.lock().unwrap().0[AlertCue::LongBreak as usize] = AlertSound {
            preset: AudioPreset::Custom,
            custom: Some(CustomSound {
                path: path.clone(),
                name: "custom.wav".into(),
            }),
        };
        mgr.play_cue(AudioCue::Tick);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (started, request) = tokio::sync::oneshot::channel();
            let (end_preview, release) = mpsc::channel();
            let preview_mgr = Arc::clone(&mgr);
            let preview = tokio::spawn(async move {
                preview_mgr
                    .preview_with(AlertCue::LongBreak, move |req| {
                        started.send(req).ok();
                        release.recv().map_err(|e| e.to_string())
                    })
                    .await
            });
            let preview_request = tokio::time::timeout(Duration::from_secs(2), request)
                .await
                .unwrap()
                .unwrap();
            assert!(!preview.is_finished());

            mgr.play_cue(AudioCue::LongBreakAlert);
            assert!(matches!(rx.try_recv().unwrap().cue, AudioCue::Tick));
            let alert_request = rx.try_recv().unwrap();
            assert!(matches!(alert_request.cue, AudioCue::LongBreakAlert));
            assert!(matches!(preview_request.cue, AudioCue::LongBreakAlert));
            assert_eq!(alert_request.preset, AudioPreset::Custom);
            assert_eq!(preview_request.preset, alert_request.preset);
            assert_eq!(alert_request.custom_path.as_deref(), Some(path.as_path()));
            assert_eq!(preview_request.custom_path, alert_request.custom_path);
            assert_eq!(alert_request.volume, 0.4);
            assert_eq!(preview_request.volume, alert_request.volume);

            end_preview.send(()).unwrap();
            preview.await.unwrap().unwrap();
        });
    }

    #[test]
    fn muted_preview_succeeds_without_a_playback_queue() {
        let settings = Settings {
            volume: 0.0,
            ..Settings::default()
        };
        let mgr = AudioManager {
            tx: mpsc::sync_channel(1).0,
            settings: Mutex::new(AudioSettings::from(&settings)),
            alerts: Mutex::new(AlertSounds::default()),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(mgr.preview(AlertCue::Work)).unwrap();
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pomotroid-audio-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            for entry in std::fs::read_dir(&self.0).unwrap().flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn legacy_import_and_independent_presets_survive_restart_and_clear() {
        let dir = TestDir::new();
        let db = dir.0.join("settings.db");
        let legacy = dir.0.join("custom_work_alert.wav");
        std::fs::write(&legacy, SOFT_BELL).unwrap();
        {
            let mut conn = Connection::open(&db).unwrap();
            crate::db::migrations::run(&conn).unwrap();
            settings::save_setting(&conn, "custom_work_alert_name", "My chime.wav").unwrap();
            let mut sounds = AlertSounds::load(&conn, &dir.0);
            assert_eq!(sounds.info().presets.work_alert, AudioPreset::Custom);
            sounds
                .set_preset(&mut conn, AlertCue::Work, AudioPreset::SoftBell)
                .unwrap();
            sounds
                .set_preset(&mut conn, AlertCue::ShortBreak, AudioPreset::Marimba)
                .unwrap();
            assert!(sounds
                .set_preset(&mut conn, AlertCue::LongBreak, AudioPreset::Custom)
                .is_err());
        }
        {
            let mut conn = Connection::open(&db).unwrap();
            let mut sounds = AlertSounds::load(&conn, &dir.0);
            let info = serde_json::to_value(sounds.info()).unwrap();
            assert_eq!(
                info,
                serde_json::json!({
                    "work_alert": "My chime.wav",
                    "short_break_alert": null,
                    "long_break_alert": null,
                    "presets": {
                        "work_alert": "soft-bell",
                        "short_break_alert": "marimba",
                        "long_break_alert": "classic",
                    }
                })
            );
            assert!(legacy.is_file());
            sounds
                .set_preset(&mut conn, AlertCue::Work, AudioPreset::Custom)
                .unwrap();
            assert_eq!(sounds.info().presets.work_alert, AudioPreset::Custom);
            sounds.clear(&mut conn, &dir.0, AlertCue::Work).unwrap();
            assert!(!legacy.exists());
            let cleared = AlertSounds::load(&conn, &dir.0).info();
            assert!(cleared.work_alert.is_none());
            assert_eq!(cleared.presets.work_alert, AudioPreset::Classic);
            assert_eq!(cleared.presets.short_break_alert, AudioPreset::Marimba);
        }
    }

    #[test]
    fn replacement_preserves_previous_import_on_failure() {
        let dir = TestDir::new();
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::run(&conn).unwrap();
        let mut sounds = AlertSounds::load(&conn, &dir.0);
        let original = dir.0.join("bell.wav");
        let new_file = dir.0.join("droplet.wav");
        std::fs::write(&original, SOFT_BELL).unwrap();
        std::fs::write(&new_file, DROPLET).unwrap();
        assert_eq!(
            sounds
                .import(&mut conn, &dir.0, AlertCue::Work, &original)
                .unwrap(),
            "bell.wav"
        );
        let bad = dir.0.join("bad.wav");
        std::fs::write(&bad, b"invalid").unwrap();
        assert!(sounds
            .import(&mut conn, &dir.0, AlertCue::Work, &bad)
            .is_err());

        conn.execute_batch(
            "CREATE TRIGGER fail_file_update BEFORE UPDATE ON settings
            WHEN NEW.key = 'custom_work_alert_file'
            BEGIN SELECT RAISE(ABORT, 'test write failure'); END;",
        )
        .unwrap();
        assert!(sounds
            .import(&mut conn, &dir.0, AlertCue::Work, &new_file)
            .is_err());
        assert_eq!(sounds.info().presets.work_alert, AudioPreset::Custom);
        let reloaded = AlertSounds::load(&conn, &dir.0);
        assert_eq!(reloaded.info().work_alert.as_deref(), Some("bell.wav"));
        let original_path = &reloaded.0[AlertCue::Work as usize]
            .custom
            .as_ref()
            .unwrap()
            .path;
        assert_eq!(std::fs::read(original_path).unwrap(), SOFT_BELL);

        conn.execute("DROP TRIGGER fail_file_update", []).unwrap();
        sounds
            .import(&mut conn, &dir.0, AlertCue::Work, &new_file)
            .unwrap();
        assert!(!original_path.exists());
        assert_eq!(
            AlertSounds::load(&conn, &dir.0)
                .info()
                .work_alert
                .as_deref(),
            Some("droplet.wav")
        );
        conn.execute("DELETE FROM settings", []).unwrap();
        AlertSounds::save_classic_presets(&conn).unwrap();
        sounds.reset(&dir.0);
        assert_eq!(sounds.info().presets.work_alert, AudioPreset::Classic);
        assert!(AlertSounds::load(&conn, &dir.0).info().work_alert.is_none());
    }

    #[test]
    fn invalid_cues_and_presets_are_rejected() {
        assert!("tick".parse::<AlertCue>().is_err());
        assert!("work_alert".parse::<AlertCue>().is_ok());
        assert!("unknown".parse::<AudioPreset>().is_err());
        assert_eq!(
            "soft-bell".parse::<AudioPreset>().unwrap(),
            AudioPreset::SoftBell
        );
    }
}
