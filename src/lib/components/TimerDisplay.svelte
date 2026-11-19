<script lang="ts">
  import type { TimerState, TimerStyle } from '$lib/types';
  import DeskClock from './DeskClock.svelte';
  import * as m from '$paraglide/messages.js';
  let {
    state,
    compact = false,
    styleName,
    widget = false,
    countdown = false,
  }: {
    state: TimerState;
    compact?: boolean;
    styleName: TimerStyle;
    widget?: boolean;
    countdown?: boolean;
  } = $props();
  const remaining = $derived(Math.max(0, state.total_secs - state.elapsed_secs));
  const remainingFraction = $derived(state.total_secs > 0 ? remaining / state.total_secs : 0);
  const display = $derived(
    `${String(Math.floor(remaining / 60)).padStart(2, '0')}:${String(remaining % 60).padStart(2, '0')}`
  );
  const phase = $derived(
    state.round_type === 'work'
      ? m.round_label_work()
      : state.round_type === 'short-break'
        ? m.round_label_short_break()
        : m.round_label_long_break()
  );
</script>

<div class="display" class:compact class:widget>
  {#if !widget}<span class="phase">{phase}</span>{/if}
  {#if styleName === 'ring'}
    <span class="time">{display}</span>
  {:else}
    <div class="desk-face">
      <span class="sr-only" role="timer">{display}</span>
      <DeskClock {styleName} {display} running={state.is_running} {remainingFraction} />
    </div>
  {/if}
  {#if widget}
    <progress
      aria-label={phase}
      max="1"
      value={countdown ? remainingFraction : 1 - remainingFraction}
    ></progress>
  {:else if !compact}<span class="status"
      >{state.is_running ? m.ui_running() : state.is_paused ? m.ui_paused() : m.ui_ready()}</span
    >{/if}
</div>

<style>
  .display {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    pointer-events: none;
  }
  .phase {
    flex-shrink: 0;
    color: var(--round-color);
    font-size: 12px;
    font-weight: 500;
  }
  .time {
    font-family: 'Mona Sans Mono', monospace;
    font-size: 52px;
    line-height: 60px;
    font-weight: 450;
    letter-spacing: -2px;
    font-variant-numeric: tabular-nums;
    color: var(--color-foreground);
    margin: 8px 0 4px;
  }
  .status {
    color: var(--color-foreground-darker);
    font-size: 12px;
  }
  .desk-face {
    width: 100%;
    margin: 12px 0;
  }
  .compact .phase {
    font-size: calc(11px / var(--timer-scale, 1));
  }
  .compact .time {
    font-size: calc(36px / var(--timer-scale, 1));
    line-height: 1.2;
    letter-spacing: -1px;
  }
  .widget {
    position: static;
    display: contents;
  }
  .widget .desk-face {
    grid-column: 1 / -1;
    grid-row: 1;
    justify-self: center;
    max-width: 208px;
    margin: 0;
    zoom: 0.74;
    pointer-events: none;
  }
  progress {
    grid-column: 1;
    grid-row: 2;
    appearance: none;
    display: block;
    width: 100%;
    height: 7px;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--round-ink) 70%, var(--round-color));
    border-radius: 4px;
    background: var(--round-ink);
    box-shadow: 0 0 0 1px #201f1e80;
    pointer-events: none;
  }
  progress::-webkit-progress-bar {
    background: var(--round-ink);
  }
  progress::-webkit-progress-value {
    background: var(--round-color);
  }
  progress::-moz-progress-bar {
    background: var(--round-color);
  }
</style>
