// Typed wrappers around Tauri invoke() and listen().
// All backend communication goes through this module.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open as dialogOpen } from '@tauri-apps/plugin-dialog';
import type {
  TimerState,
  Settings,
  Theme,
  CueKey,
  AudioPreset,
  CustomAudioInfo,
  DetailedStats,
  HeatmapStats,
  UpdateInfo,
  WalletSummary,
  WalletIncomeHistory,
  WalletLedgerPage,
} from '$lib/types';

// --- Timer commands ---

export const timerToggle = () => invoke<void>('timer_toggle');
export const timerReset = () => invoke<void>('timer_reset');
export const timerRestartRound = () => invoke<void>('timer_restart_round');
export const timerSkip = () => invoke<void>('timer_skip');
export const getTimerState = () => invoke<TimerState>('timer_get_state');

// --- Settings commands ---

export const getSettings = () => invoke<Settings>('settings_get');
/** Save a single setting key/value pair and receive the full updated settings. */
export const setSetting = (key: string, value: string) =>
  invoke<Settings>('settings_set', { key, value });
export const resetSettings = () => invoke<Settings>('settings_reset_defaults');
export const reloadShortcuts = () => invoke<void>('shortcuts_reload');

// --- Theme commands ---

export const getThemes = () => invoke<Theme[]>('themes_list');

// --- Notification commands ---

export const setNotificationLocale = (locale: string) =>
  invoke<void>('notification_set_locale', { locale });
/** Submission only; the OS may suppress a queued notification. */
export const notificationTest = () => invoke<void>('notification_test');

// --- Window commands ---

export const setWindowVisibility = (visible: boolean) =>
  invoke<void>('window_set_visibility', { visible });
export const openDesktopWidget = () => invoke<void>('window_open_widget');
export const desktopWidgetReady = () => invoke<void>('window_widget_ready');
export const restoreMainWindow = () => invoke<void>('window_restore_main');

export type TrayPanelAction =
  | 'start'
  | 'pause'
  | 'resume'
  | 'skip'
  | 'reset-round'
  | 'show'
  | 'exit'
  | 'dismiss';

export const trayPanelReady = ({ height }: { height: number }) =>
  invoke<void>('tray_panel_ready', { height });
export const trayPanelAction = ({ action }: { action: TrayPanelAction }) =>
  invoke<void>('tray_panel_action', { action });

/** True means Windows queued the effect request; rendering depends on the OS. */
export const applyWindowMaterial = (themeMode: 'auto' | 'light' | 'dark', enabled: boolean) =>
  invoke<boolean>('window_apply_material', { themeMode, enabled });

// --- Audio commands ---

export const getCustomAudioInfo = () => invoke<CustomAudioInfo>('audio_get_custom_info');

/** Import and select a custom sound for `cue`; returns the display name. */
export const setCustomAudio = (cue: CueKey, srcPath: string) =>
  invoke<string>('audio_set_custom', { cue, srcPath });

/** Delete the custom file for `cue` and select Classic. */
export const clearCustomAudio = (cue: CueKey) => invoke<void>('audio_clear_custom', { cue });

export const setAudioPreset = (cue: CueKey, preset: AudioPreset) =>
  invoke<void>('audio_set_preset', { cue, preset });
export const previewAudio = (cue: CueKey) => invoke<void>('audio_preview', { cue });

/** Open a native file picker filtered to audio formats. Returns a path or null. */
export const openAudioFilePicker = (): Promise<string | null> =>
  dialogOpen({
    multiple: false,
    filters: [{ name: 'Audio', extensions: ['mp3', 'wav', 'ogg'] }],
  }) as Promise<string | null>;

// --- Diagnostic log commands ---

/** Open the application log directory in the OS file manager. */
export const openLogDir = () => invoke<void>('open_log_dir');

/** Return the resolved log directory path as a string. */
export const getLogDir = () => invoke<string>('get_log_dir');

/** Return the compile-time build version string (e.g. `1.0.0-dev.80+20b2d87`). */
export const appVersion = () => invoke<string>('app_version');

// --- Sessions commands ---

export const clearSessionHistory = () => invoke<void>('sessions_clear');

// --- Stats commands ---

/** Daily + weekly data + streak in one call (Today and This Week tabs). */
export const statsGetDetailed = () => invoke<DetailedStats>('stats_get_detailed');

/** Heatmap entries + lifetime totals (All Time tab). */
export const statsGetHeatmap = () => invoke<HeatmapStats>('stats_get_heatmap');

// --- Reward wallet commands ---

export const walletGetSummary = () => invoke<WalletSummary>('wallet_get_summary');
export const walletGetIncomeHistory = (days: 7 | 30 = 7) =>
  invoke<WalletIncomeHistory>('wallet_get_income_history', { days });
export const walletGetLedger = (beforeId: number | null = null) =>
  invoke<WalletLedgerPage>('wallet_get_ledger', { beforeId });
export const walletSetRate = (rateMinorPerHour: number) =>
  invoke<void>('wallet_set_rate', { rateMinorPerHour });
export const walletSpend = (amountMinor: number, note: string, requestId: string) =>
  invoke<void>('wallet_spend', { amountMinor, note, requestId });
export const onWalletChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen<void>('wallet:changed', () => cb());
export const onWalletError = (cb: () => void): Promise<UnlistenFn> =>
  listen<void>('wallet:error', () => cb());

// --- Platform commands ---

export const accessibilityTrusted = () => invoke<boolean>('accessibility_trusted');

/** Returns true if the system tray is usable on this platform/install.
 *  On Linux this probes for libayatana-appindicator3 / libappindicator3 at
 *  runtime; on macOS and Windows it always returns true. */
export const traySupported = () => invoke<boolean>('tray_supported');

// --- Updater commands ---

/** Check for an available update. Returns update info or null if already up to date. */
export const checkUpdate = () => invoke<UpdateInfo | null>('check_update');

/** Download, install, and immediately relaunch with the pending update. */
export const installUpdate = () => invoke<void>('install_update');

// --- Event listeners ---

export const onTimerTick = (
  cb: (payload: { elapsed_secs: number; total_secs: number }) => void
): Promise<UnlistenFn> =>
  listen<{ elapsed_secs: number; total_secs: number }>('timer:tick', (e) => cb(e.payload));

export const onTimerPaused = (
  cb: (payload: { elapsed_secs: number }) => void
): Promise<UnlistenFn> => listen<{ elapsed_secs: number }>('timer:paused', (e) => cb(e.payload));

export const onTimerResumed = (
  cb: (payload: { elapsed_secs: number }) => void
): Promise<UnlistenFn> => listen<{ elapsed_secs: number }>('timer:resumed', (e) => cb(e.payload));

export const onRoundChange = (cb: (state: TimerState) => void): Promise<UnlistenFn> =>
  listen<TimerState>('timer:round-change', (e) => cb(e.payload));

export const onTimerReset = (cb: (state: TimerState) => void): Promise<UnlistenFn> =>
  listen<TimerState>('timer:reset', (e) => cb(e.payload));

export const onSettingsChanged = (cb: (settings: Settings) => void): Promise<UnlistenFn> =>
  listen<Settings>('settings:changed', (e) => cb(e.payload));

// Catch changes made while the new window was loading its theme and fonts.
export async function watchWindowSettings(cb: (settings: Settings) => void): Promise<UnlistenFn> {
  let received = false;
  const stop = await onSettingsChanged((value) => {
    received = true;
    cb(value);
  });
  try {
    const snapshot = await getSettings();
    if (!received) cb(snapshot);
  } catch (error) {
    stop();
    throw error;
  }
  return stop;
}

export const onThemesChanged = (cb: (themes: Theme[]) => void): Promise<UnlistenFn> =>
  listen<Theme[]>('themes:changed', (e) => cb(e.payload));

export const onSessionsCleared = (cb: () => void): Promise<UnlistenFn> =>
  listen<void>('sessions:cleared', () => cb());
