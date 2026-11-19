<script lang="ts">
  import { timerToggle, timerRestartRound, timerSkip } from '$lib/ipc';
  import { timerState } from '$lib/stores/timer';
  import Icon from './Icon.svelte';
  import * as m from '$paraglide/messages.js';
  const state = $derived($timerState);
</script>

<div class="mini-controls">
  <button
    class="ui-button ui-icon-button"
    onclick={timerRestartRound}
    aria-label={m.tooltip_restart_round()}><Icon name="restart" size={15} /></button
  >
  <button
    class="play"
    onclick={timerToggle}
    aria-label={state.is_running ? m.ui_pause() : state.is_paused ? m.ui_resume() : m.ui_start()}
    ><Icon name={state.is_running ? 'pause' : 'play'} size={16} /></button
  >
  <button class="ui-button ui-icon-button" onclick={timerSkip} aria-label={m.tooltip_skip()}
    ><Icon name="skip" size={15} /></button
  >
</div>

<style>
  .mini-controls {
    display: flex;
    align-items: center;
    gap: 18px;
  }
  .play {
    width: 36px;
    height: 36px;
    border: 0;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--round-color);
    color: var(--round-ink);
    cursor: pointer;
  }
</style>
