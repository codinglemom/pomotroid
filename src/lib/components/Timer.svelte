<script lang="ts">
  // Share the live timer subscription between the main and floating views.
  import { onMount } from 'svelte';
  import {
    timerToggle,
    timerRestartRound,
    timerSkip,
    getTimerState,
    onTimerTick,
    onTimerPaused,
    onTimerResumed,
    onRoundChange,
    onTimerReset,
  } from '$lib/ipc';
  import { timerState } from '$lib/stores/timer';
  import { settings } from '$lib/stores/settings';
  import TimerDial from './TimerDial.svelte';
  import TimerDisplay from './TimerDisplay.svelte';
  import TimerFooter from './TimerFooter.svelte';
  import DesktopWidget from './DesktopWidget.svelte';
  import MiniControls from './MiniControls.svelte';
  import Tooltip from './Tooltip.svelte';
  import Icon from './Icon.svelte';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import { error as logError } from '@tauri-apps/plugin-log';
  import * as m from '$paraglide/messages.js';

  interface Props {
    isCompact?: boolean;
    uiScale?: number;
    widget?: boolean;
    widgetSettingsReady?: boolean;
    onInitError?: () => void;
  }

  let {
    isCompact = false,
    uiScale = 1,
    widget = false,
    widgetSettingsReady = false,
    onInitError,
  }: Props = $props();

  let snapshot = $derived($timerState);
  let hydrated = $state(false);
  const round = $derived(
    snapshot.round_type === 'work'
      ? 'focus-round'
      : snapshot.round_type === 'short-break'
        ? 'short-round'
        : 'long-round'
  );

  onMount(() => {
    const cleanups: UnlistenFn[] = [];
    let disposed = false;

    async function register(listener: Promise<UnlistenFn>) {
      const unlisten = await listener;
      if (disposed) unlisten();
      else cleanups.push(unlisten);
    }

    // Async setup: hydrate state and register event listeners.
    (async () => {
      await Promise.all([
        register(
          onTimerTick(({ elapsed_secs, total_secs }) => {
            timerState.update((s) => ({
              ...s,
              elapsed_secs,
              total_secs,
              is_running: true,
              is_paused: false,
            }));
          })
        ),
        register(
          onTimerPaused(({ elapsed_secs }) => {
            timerState.update((s) => ({
              ...s,
              elapsed_secs,
              is_running: false,
              is_paused: true,
            }));
          })
        ),
        register(
          onTimerResumed(({ elapsed_secs }) => {
            timerState.update((s) => ({
              ...s,
              elapsed_secs,
              is_running: true,
              is_paused: false,
            }));
          })
        ),
        register(onRoundChange(timerState.set)),
        register(onTimerReset(timerState.set)),
      ]);
      // Subscribe before hydration so a reconstructed window does not miss a
      // transition while requesting the current running or paused snapshot.
      const snapshot = await getTimerState();
      if (!disposed) {
        timerState.set(snapshot);
        hydrated = true;
      }
    })().catch((error) => {
      if (disposed) return;
      void logError(`[timer] initialization failed: ${error}`).catch(() => {});
      onInitError?.();
    });

    return () => {
      disposed = true;
      for (const unlisten of cleanups) unlisten();
    };
  });
</script>

{#if widget}
  <DesktopWidget
    {snapshot}
    {round}
    styleName={$settings.timer_style === 'ring' ? 'nixie' : $settings.timer_style}
    countdown={$settings.dial_countdown}
    ready={widgetSettingsReady && hydrated}
  />
{:else}
  <div
    class="timer-outer"
    class:compact={isCompact}
    style="--round-color:var(--color-{round}); --round-ink:var(--color-on-{round}); --timer-scale:{uiScale}"
  >
    <div class="timer" style="zoom:{uiScale}">
      <div class="dial-stack" class:desk={$settings.timer_style !== 'ring'}>
        {#if $settings.timer_style === 'ring'}
          <TimerDial snap={snapshot} countdown={$settings.dial_countdown} />
        {/if}
        <TimerDisplay state={snapshot} compact={isCompact} styleName={$settings.timer_style} />
      </div>
      {#if !isCompact}
        <div
          class="rounds"
          aria-label={$settings.long_breaks_enabled
            ? m.ui_round_progress({
                current: snapshot.work_round_number,
                total: snapshot.work_rounds_total,
              })
            : m.timer_session_round({ n: snapshot.session_work_count })}
        >
          {#if $settings.long_breaks_enabled && snapshot.work_rounds_total <= 12}
            <span class="dots" aria-hidden="true"
              >{#each Array(snapshot.work_rounds_total) as _, index}<i
                  class:done={index < snapshot.work_round_number}
                ></i>{/each}</span
            >
          {/if}
          <span
            >{$settings.long_breaks_enabled
              ? m.ui_round_progress({
                  current: snapshot.work_round_number,
                  total: snapshot.work_rounds_total,
                })
              : m.timer_session_round({ n: snapshot.session_work_count })}</span
          >
        </div>
        <div class="controls-wrapper">
          <Tooltip text={m.tooltip_restart_round()}
            ><button
              class="ui-button ui-icon-button"
              onclick={timerRestartRound}
              aria-label={m.tooltip_restart_round()}><Icon name="restart" /></button
            ></Tooltip
          >
          <button class="play-pause" onclick={timerToggle}
            ><Icon name={snapshot.is_running ? 'pause' : 'play'} />{snapshot.is_running
              ? m.ui_pause()
              : snapshot.is_paused
                ? m.ui_resume()
                : m.ui_start()}</button
          >
          <Tooltip text={m.tooltip_skip()}
            ><button
              class="ui-button ui-icon-button"
              onclick={timerSkip}
              aria-label={m.tooltip_skip()}><Icon name="skip" /></button
            ></Tooltip
          >
        </div>
        <TimerFooter />
      {/if}
    </div>
    {#if isCompact}<MiniControls />{/if}
  </div>
{/if}

<style>
  .timer-outer,
  .timer {
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  .timer-outer {
    gap: 10px;
  }
  .dial-stack {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .desk {
    width: 282px;
    height: 224px;
  }
  .compact .desk {
    width: 218px;
  }
  .rounds {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    margin-top: 12px;
    min-height: 20px;
    font-size: 12px;
    color: var(--color-foreground-darker);
  }
  .dots {
    display: flex;
    gap: 5px;
  }
  .dots i {
    display: block;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--color-separator);
  }
  .dots i.done {
    background: var(--round-color);
  }
  .controls-wrapper {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: 20px;
  }
  .play-pause {
    min-width: 128px;
    height: 44px;
    padding: 0 16px;
    border: 0;
    border-radius: 6px;
    background: var(--round-color);
    color: var(--round-ink);
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: filter var(--transition-snappy);
  }
  .play-pause:hover {
    filter: brightness(0.95);
  }
  .play-pause:active {
    filter: brightness(0.9);
  }
</style>
