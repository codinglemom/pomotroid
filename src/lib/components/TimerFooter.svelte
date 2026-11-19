<script lang="ts">
  // Round counter, reset/skip buttons, and volume slider.
  import { timerReset, setSetting } from '$lib/ipc';
  import { settings } from '$lib/stores/settings';
  import * as m from '$paraglide/messages.js';
  import Tooltip from './Tooltip.svelte';
  import Icon from './Icon.svelte';

  // Local volume for immediate slider feedback — avoids waiting for the
  // async IPC round-trip before the thumb visually moves.
  let localVolume = $state($settings.volume);
  $effect(() => {
    localVolume = $settings.volume;
  });

  // Remembered pre-mute level so the button can restore it on unmute.
  let premuteVolume = $state<number | null>(null);

  function handleVolumeChange(e: Event) {
    const val = (e.target as HTMLInputElement).valueAsNumber;
    localVolume = val;
    setSetting('volume', String(Math.round(val * 100)));
  }

  function toggleMute() {
    if (localVolume === 0) {
      const restore = premuteVolume ?? 1.0;
      premuteVolume = null;
      localVolume = restore;
      setSetting('volume', String(Math.round(restore * 100)));
    } else {
      premuteVolume = localVolume;
      localVolume = 0;
      setSetting('volume', '0');
    }
  }
</script>

<div class="timer-footer">
  <Tooltip text={m.tooltip_reset()}>
    <button class="reset" onclick={timerReset}
      ><Icon name="reset" size={15} />{m.timer_reset()}</button
    >
  </Tooltip>
  <div class="volume">
    <Tooltip text={localVolume === 0 ? m.tooltip_unmute() : m.tooltip_mute()}>
      <button
        class="ui-button ui-icon-button"
        onclick={toggleMute}
        aria-label={localVolume === 0 ? m.tooltip_unmute() : m.tooltip_mute()}
        aria-pressed={localVolume === 0}
        ><Icon name={localVolume === 0 ? 'muted' : 'volume'} size={16} /></button
      >
    </Tooltip>
    <input
      type="range"
      min="0"
      max="1"
      step="0.01"
      value={localVolume}
      oninput={handleVolumeChange}
      aria-label={m.ui_volume()}
    />
  </div>
</div>

<style>
  .timer-footer {
    width: 280px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 16px;
    color: var(--color-foreground-darker);
  }
  .reset {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 32px;
    padding: 4px 6px;
    font-size: 12px;
    border: 0;
    border-radius: 6px;
    color: inherit;
    background: transparent;
    cursor: pointer;
  }
  .reset:hover {
    background: var(--color-hover);
  }
  .volume {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  input {
    width: 78px;
    height: 24px;
    accent-color: var(--color-accent);
    cursor: pointer;
  }
</style>
