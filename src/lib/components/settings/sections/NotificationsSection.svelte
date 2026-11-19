<script module lang="ts">
  // A page revisited during a save must read audio settings after that save.
  let pendingAudioChange: Promise<unknown> | null = null;
  let pendingVolumeChange = Promise.resolve();
  let volumeRevision = 0;
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { settings } from '$lib/stores/settings';
  import {
    setSetting,
    getCustomAudioInfo,
    setCustomAudio,
    clearCustomAudio,
    openAudioFilePicker,
    setAudioPreset,
    previewAudio,
    notificationTest,
  } from '$lib/ipc';
  import Icon from '$lib/components/Icon.svelte';
  import SettingsToggle from '$lib/components/settings/SettingsToggle.svelte';
  import type { AudioPreset, CueKey, CustomAudioInfo } from '$lib/types';
  import * as m from '$paraglide/messages.js';
  import { warn, error as logError } from '@tauri-apps/plugin-log';

  const isWindows = /Windows/.test(navigator.userAgent);

  const CUE_LIST: { id: CueKey; label: () => string }[] = [
    { id: 'work_alert', label: m.notif_alert_work },
    { id: 'short_break_alert', label: m.notif_alert_short_break },
    { id: 'long_break_alert', label: m.notif_alert_long_break },
  ];
  const BUILT_IN_PRESETS = [
    ['classic', m.notif_audio_classic],
    ['soft-bell', m.notif_audio_soft_bell],
    ['marimba', m.notif_audio_marimba],
    ['droplet', m.notif_audio_droplet],
  ] as const;

  let localVolume = $state($settings.volume);
  let savingVolume = $state(false);
  $effect(() => {
    if (!savingVolume) localVolume = $settings.volume;
  });

  let audioInfo = $state<CustomAudioInfo | null>(null);
  let audioBusy = $state(false);
  let audioLoadFailed = $state(false);
  let audioErrors = $state<Partial<Record<CueKey, () => string>>>({});
  let testing = $state(false);
  let testFeedback = $state('');
  let testFailed = $state(false);

  async function sendTest() {
    if (testing || !$settings.notifications_enabled) return;
    testing = true;
    testFeedback = '';
    testFailed = false;
    try {
      await notificationTest();
      testFeedback = isWindows ? m.notif_test_submitted_windows() : m.notif_test_submitted();
    } catch (e) {
      testFailed = true;
      switch (String(e)) {
        case 'notifications_disabled':
          testFeedback = m.notif_test_enable_first();
          break;
        case 'notifications_windows_unregistered':
          testFeedback = m.notif_test_windows_unregistered();
          break;
        case 'notifications_windows_disabled':
          testFeedback = m.notif_test_windows_disabled();
          break;
        default:
          testFeedback = m.notif_test_failed();
      }
    } finally {
      testing = false;
    }
  }

  async function refreshAudioInfo() {
    await pendingAudioChange?.catch(() => {});
    try {
      audioInfo = await getCustomAudioInfo();
      audioLoadFailed = false;
    } catch (err) {
      audioInfo = null;
      audioLoadFailed = true;
      void warn(`[audio] getCustomAudioInfo failed: ${err}`).catch(() => {});
    }
  }

  onMount(() => {
    void refreshAudioInfo();
  });

  async function toggle(dbKey: string, current: boolean) {
    const updated = await setSetting(dbKey, current ? 'false' : 'true');
    settings.set(updated);
  }

  function handleVolumeInput(e: Event & { currentTarget: HTMLInputElement }) {
    const val = e.currentTarget.valueAsNumber;
    localVolume = val;
    savingVolume = true;
    const revision = ++volumeRevision;
    pendingVolumeChange = pendingVolumeChange.then(async () => {
      if (revision !== volumeRevision) return;
      try {
        const updated = await setSetting('volume', String(Math.round(val * 100)));
        if (revision === volumeRevision) settings.set(updated);
      } catch (err) {
        void logError(`[audio] setting volume failed: ${err}`).catch(() => {});
      } finally {
        if (revision === volumeRevision) savingVolume = false;
      }
    });
  }

  async function updateAudio(id: CueKey, change: () => Promise<unknown>) {
    if (audioBusy || !audioInfo) return;
    audioBusy = true;
    audioErrors[id] = undefined;
    pendingAudioChange = Promise.resolve().then(change);
    try {
      await pendingAudioChange;
    } catch (err) {
      audioErrors[id] = m.notif_audio_save_failed;
      void logError(`[audio] updating ${id} failed: ${err}`).catch(() => {});
    } finally {
      pendingAudioChange = null;
      await refreshAudioInfo();
      audioBusy = false;
    }
  }

  function selectPreset(id: CueKey, select: HTMLSelectElement) {
    const current = audioInfo?.presets[id];
    const selected = select.value as AudioPreset;
    if (!current || selected === current) return;
    // Keep the persisted selection until the backend confirms the change.
    select.value = current;
    return updateAudio(id, () => setAudioPreset(id, selected));
  }

  function pickAudio(id: CueKey) {
    return updateAudio(id, async () => {
      const path = await openAudioFilePicker();
      if (path) await setCustomAudio(id, path);
    });
  }

  async function playAudio(id: CueKey) {
    if (audioBusy || !audioInfo || localVolume === 0) return;
    audioBusy = true;
    audioErrors[id] = undefined;
    try {
      await pendingVolumeChange;
      await previewAudio(id);
    } catch (err) {
      audioErrors[id] = m.notif_audio_preview_failed;
      void logError(`[audio] previewing ${id} failed: ${err}`).catch(() => {});
    } finally {
      audioBusy = false;
    }
  }

  function keepAudioControlKeys(event: KeyboardEvent) {
    event.stopPropagation();
  }
</script>

<div class="section">
  <div class="group-heading">{m.notif_group_desktop()}</div>
  <div class="desktop-row">
    <div class="desktop-toggle">
      <SettingsToggle
        label={m.notif_toggle_desktop()}
        description={m.notif_toggle_desktop_desc()}
        checked={$settings.notifications_enabled}
        onclick={() => {
          testFeedback = '';
          return toggle('notifications', $settings.notifications_enabled);
        }}
      />
    </div>
    <button
      class="btn-test"
      onclick={sendTest}
      disabled={testing || !$settings.notifications_enabled}
    >
      {testing ? m.notif_test_sending() : m.notif_test_button()}
    </button>
  </div>
  {#if !$settings.notifications_enabled}<p class="desktop-hint">
      {m.notif_test_enable_first()}
    </p>{/if}
  {#if testFeedback}<p
      class="desktop-hint"
      role={testFailed ? 'alert' : 'status'}
      aria-live="polite"
    >
      {testFeedback}
    </p>{/if}
  {#if isWindows}<p class="desktop-hint">{m.notif_windows_hint()}</p>{/if}

  <div class="group-heading">{m.notif_group_alert()}</div>

  {#if audioLoadFailed}
    <p class="audio-error" role="alert">{m.notif_audio_load_failed()}</p>
  {/if}
  {#if audioInfo && localVolume === 0}
    <p class="desktop-hint" id="alert-preview-muted">{m.notif_audio_preview_muted()}</p>
  {/if}

  {#each CUE_LIST as { id, label } (id)}
    {@const filename = audioInfo?.[id]}
    <div class="audio-row">
      <label class="label" for={id}>{label()}</label>
      <div class="audio-actions">
        <select
          {id}
          value={audioInfo?.presets[id] ?? ''}
          disabled={!audioInfo}
          aria-disabled={audioBusy}
          title={audioInfo?.presets[id] === 'custom' ? filename : undefined}
          onkeydown={keepAudioControlKeys}
          onchange={(event) => selectPreset(id, event.currentTarget)}
        >
          {#if !audioInfo}<option value=""></option>{/if}
          {#each BUILT_IN_PRESETS as [preset, presetLabel] (preset)}
            <option value={preset}>{presetLabel()}</option>
          {/each}
          {#if filename}
            <option value="custom">{m.notif_audio_custom({ name: filename })}</option>
          {/if}
        </select>
        <button
          type="button"
          class="audio-icon-button"
          title={m.notif_audio_preview()}
          aria-label={m.notif_audio_action({ cue: label(), action: m.notif_audio_preview() })}
          aria-describedby={audioInfo && localVolume === 0 ? 'alert-preview-muted' : undefined}
          disabled={!audioInfo}
          aria-disabled={audioBusy || localVolume === 0}
          onkeydown={keepAudioControlKeys}
          onclick={() => playAudio(id)}
        >
          <Icon name="play" size={16} />
        </button>
        <button
          type="button"
          aria-label={m.notif_audio_action({ cue: label(), action: m.notif_btn_choose() })}
          disabled={!audioInfo}
          aria-disabled={audioBusy}
          onkeydown={keepAudioControlKeys}
          onclick={() => pickAudio(id)}
        >
          {m.notif_btn_choose()}
        </button>
        {#if filename}
          <button
            type="button"
            class="audio-icon-button"
            title={m.notif_btn_restore()}
            aria-label={m.notif_audio_action({ cue: label(), action: m.notif_btn_restore() })}
            aria-disabled={audioBusy}
            onkeydown={keepAudioControlKeys}
            onclick={() => updateAudio(id, () => clearCustomAudio(id))}
          >
            <Icon name="reset" size={16} />
          </button>
        {/if}
      </div>
    </div>
    {#if audioErrors[id]}
      <p class="audio-error" role="alert">{audioErrors[id]?.()}</p>
    {/if}
  {/each}

  <div class="group-heading">{m.notif_group_tick()}</div>

  <SettingsToggle
    label={m.notif_toggle_tick_work()}
    description={m.notif_toggle_tick_work_desc()}
    checked={$settings.tick_sounds_during_work}
    onclick={() => toggle('tick_sounds_work', $settings.tick_sounds_during_work)}
  />
  <SettingsToggle
    label={m.notif_toggle_tick_break()}
    description={m.notif_toggle_tick_break_desc()}
    checked={$settings.tick_sounds_during_break}
    onclick={() => toggle('tick_sounds_break', $settings.tick_sounds_during_break)}
  />

  <div class="group-heading">{m.notif_group_volume()}</div>

  <div class="volume-row">
    <div class="volume-meta">
      <span class="label">{m.notif_label_volume()}</span>
      <span class="value">{Math.round(localVolume * 100)}%</span>
    </div>
    <div class="slider-wrap">
      <input
        type="range"
        min="0"
        max="1"
        step="0.01"
        value={localVolume}
        class="slider"
        oninput={handleVolumeInput}
      />
      <div class="bar" style="width: {localVolume * 100}%"></div>
    </div>
  </div>
</div>

<style>
  .desktop-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-right: 20px;
  }
  .desktop-toggle {
    flex: 1;
    min-width: 0;
  }
  .desktop-hint {
    padding: 4px 20px 8px;
    font-size: 0.73rem;
    line-height: 1.5;
    color: var(--color-foreground-darker);
  }
  .btn-test {
    flex-shrink: 0;
    border: 1px solid var(--color-separator);
    border-radius: 4px;
    padding: 6px 10px;
    background: var(--color-hover);
    color: var(--color-foreground);
    font: inherit;
    font-size: 0.72rem;
    cursor: pointer;
  }
  .btn-test:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn-test:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  .section {
    display: flex;
    flex-direction: column;
  }

  .group-heading {
    font-size: 0.68rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--color-foreground-darker, var(--color-foreground));
    opacity: 0.6;
    margin: 0;
    padding: 16px 20px 6px;
  }

  .volume-row {
    padding: 10px 20px 14px;
    border-bottom: 1px solid var(--color-separator);
  }

  .volume-meta {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 10px;
  }

  .label {
    font-size: 0.85rem;
    color: var(--color-foreground);
    letter-spacing: 0.02em;
  }

  .value {
    font-size: 0.8rem;
    font-family: monospace;
    color: var(--color-foreground-darker, var(--color-foreground));
    background: var(--color-hover);
    padding: 2px 8px;
    border-radius: 3px;
  }

  .slider-wrap {
    position: relative;
    height: 20px;
    display: flex;
    align-items: center;
  }

  .slider {
    position: relative;
    z-index: 2;
    width: 100%;
    -webkit-appearance: none;
    appearance: none;
    height: 4px;
    background: color-mix(in oklch, var(--color-foreground) 14%, transparent);
    border-radius: 2px;
    outline: none;
    cursor: pointer;
  }

  .slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--color-foreground);
    cursor: pointer;
  }

  .slider::-moz-range-thumb {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--color-foreground);
    cursor: pointer;
    border: none;
  }

  .bar {
    position: absolute;
    left: 0;
    height: 4px;
    border-radius: 2px;
    pointer-events: none;
    z-index: 1;
    background: var(--color-accent);
    transition: width 0.05s;
  }

  .audio-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 12px;
    padding: 10px 20px;
    border-bottom: 1px solid var(--color-separator);
  }

  .audio-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    max-width: 100%;
  }

  .audio-actions select,
  .audio-actions button {
    min-height: 36px;
    padding: 4px 10px;
    border-radius: var(--radius-control);
    cursor: pointer;
    border: 1px solid var(--control-border);
    background: var(--color-hover);
    color: var(--color-foreground);
    font: inherit;
    font-size: 0.8rem;
  }

  .audio-actions select {
    width: 142px;
    min-width: 84px;
    text-overflow: ellipsis;
    background: var(--color-background-light);
  }

  .audio-actions select option {
    background: var(--color-background-light);
    color: var(--color-foreground);
  }

  .audio-actions .audio-icon-button {
    display: grid;
    place-items: center;
    width: 36px;
    flex-shrink: 0;
    padding: 0;
  }

  .audio-actions button:not(:disabled, [aria-disabled='true']):hover {
    background: color-mix(in oklch, var(--color-foreground) 17%, transparent);
  }

  .audio-actions :is(select, button):focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }

  .audio-actions :is(:disabled, [aria-disabled='true']) {
    opacity: 0.5;
    cursor: default;
  }

  .audio-error {
    margin: 0;
    padding: 2px 20px 8px;
    font-size: 0.75rem;
    color: var(--color-error);
  }
</style>
