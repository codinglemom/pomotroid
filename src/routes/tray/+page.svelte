<script lang="ts">
  import '../../app.css';
  import { onMount, tick } from 'svelte';
  import { error as logError } from '@tauri-apps/plugin-log';
  import { getSettings, getThemes, getTimerState, trayPanelAction, trayPanelReady } from '$lib/ipc';
  import type { TrayPanelAction } from '$lib/ipc';
  import Icon, { type IconName } from '$lib/components/Icon.svelte';
  import TomatoMark from '$lib/components/TomatoMark.svelte';
  import { setLocale } from '$lib/locale.svelte.js';
  import { settings } from '$lib/stores/settings';
  import { applyTheme } from '$lib/stores/theme';
  import type { TimerState } from '$lib/types';
  import { resolveThemeName } from '$lib/utils/theme';
  import * as m from '$paraglide/messages.js';
  import { getLocale } from '$paraglide/runtime.js';

  let panel: HTMLElement;
  let snapshot = $state<TimerState>();
  let active = $derived(snapshot?.is_running || snapshot?.is_paused);
  let remaining = $derived(snapshot ? Math.max(0, snapshot.total_secs - snapshot.elapsed_secs) : 0);
  let display = $derived(
    `${String(Math.floor(remaining / 60)).padStart(2, '0')}:${String(remaining % 60).padStart(2, '0')}`
  );
  let phase = $derived(
    snapshot?.round_type === 'short-break'
      ? m.round_label_short_break()
      : snapshot?.round_type === 'long-break'
        ? m.round_label_long_break()
        : m.round_label_work()
  );
  let toggleLabel = $derived(
    snapshot?.is_running ? m.ui_pause() : snapshot?.is_paused ? m.ui_resume() : m.ui_start()
  );
  let toggleAction: 'start' | 'pause' | 'resume' = $derived(
    snapshot?.is_running ? 'pause' : snapshot?.is_paused ? 'resume' : 'start'
  );
  let actionPending = false;

  onMount(() => {
    let disposed = false;
    let pollTimer: ReturnType<typeof setTimeout>;

    async function refresh() {
      try {
        const next = await getTimerState();
        if (!disposed) snapshot = next;
      } catch {
        // Keep the last actual snapshot until the next refresh.
      } finally {
        if (!disposed) pollTimer = setTimeout(refresh, 1000);
      }
    }

    async function initialize() {
      try {
        const [saved, themes, initialSnapshot] = await Promise.all([
          getSettings(),
          getThemes(),
          getTimerState(),
        ]);
        if (disposed) return;
        settings.set(saved);
        snapshot = initialSnapshot;

        const osDark = matchMedia('(prefers-color-scheme: dark)').matches;
        const theme = themes.find((entry) => entry.name === resolveThemeName(saved, osDark));
        const activeTheme = theme ?? themes[0];
        await Promise.all([
          setLocale(saved.language, { syncNotifications: false }),
          activeTheme && applyTheme(activeTheme),
        ]);
        document.documentElement.lang = getLocale();
        await tick();
        await document.fonts.ready;
        if (disposed) return;
        await trayPanelReady({ height: Math.ceil(panel.getBoundingClientRect().height) });
        if (!disposed) pollTimer = setTimeout(refresh, 1000);
      } catch (error) {
        if (disposed) return;
        void logError(`[tray] menu initialization failed: ${String(error)}`).catch(() => {});
        void trayPanelAction({ action: 'dismiss' }).catch(() => {});
      }
    }

    void initialize();
    return () => {
      disposed = true;
      clearTimeout(pollTimer);
    };
  });

  async function dispatch(action: TrayPanelAction) {
    if (actionPending) return;
    actionPending = true;
    try {
      await trayPanelAction({ action });
    } catch (error) {
      void logError(`[tray] menu action ${action} failed: ${String(error)}`).catch(() => {});
    } finally {
      actionPending = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      void dispatch('dismiss');
      return;
    }
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown' && event.key !== 'Enter') return;

    const buttons = Array.from(panel.querySelectorAll<HTMLButtonElement>('button:enabled'));
    const focused = buttons.findIndex((button) => button === document.activeElement);
    if (event.key === 'Enter') {
      if (focused === -1) {
        event.preventDefault();
        buttons[0]?.click();
      }
      return;
    }
    event.preventDefault();
    const next =
      event.key === 'ArrowDown'
        ? (focused + 1) % buttons.length
        : focused <= 0
          ? buttons.length - 1
          : focused - 1;
    buttons[next]?.focus();
  }
</script>

{#snippet actionButton(
  action: Exclude<TrayPanelAction, 'dismiss'>,
  icon: IconName,
  label: string,
  disabled = false
)}
  <button
    type="button"
    class:primary={action === toggleAction}
    {disabled}
    onclick={() => dispatch(action)}
  >
    <span class="action-icon"><Icon name={icon} size={16} /></span>
    <span>{label}</span>
  </button>
{/snippet}

<svelte:window onkeydown={handleKeydown} />

<main class="panel" bind:this={panel} aria-labelledby="tray-title">
  <header class="summary">
    <TomatoMark size={18} />
    <h1 id="tray-title">小番茄</h1>
    <span class="phase">{phase}</span>
    <span class="time" role="timer">{display}</span>
  </header>

  <hr />
  {@render actionButton(toggleAction, snapshot?.is_running ? 'pause' : 'play', toggleLabel)}
  {@render actionButton('skip', 'skip', m.shortcuts_skip_round(), !active)}
  {@render actionButton('reset-round', 'reset', m.tray_reset_round(), !active)}
  <hr />
  {@render actionButton('show', 'window', m.tray_show())}
  <hr />
  {@render actionButton('exit', 'exit', m.tray_exit())}
</main>

<style>
  .panel {
    padding: 6px;
    border: 1px solid var(--color-separator);
    border-radius: var(--radius-panel);
  }

  .summary,
  button {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 10px;
    padding-inline: 9px;
  }

  .summary {
    padding-block: 5px 6px;
  }

  h1 {
    grid-column: 2 / -1;
    font-size: 14px;
    font-weight: 650;
  }

  .phase {
    grid-column: 2;
    color: var(--color-foreground-darker);
    font-size: 12px;
  }

  .time {
    font-family: 'Mona Sans Mono', monospace;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }

  hr {
    border: 0;
    border-top: 1px solid var(--color-separator);
    margin: 4px 9px;
  }

  button {
    width: 100%;
    min-height: 32px;
    padding-block: 4px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--color-foreground);
    text-align: start;
    font-size: 13px;
    line-height: 1.35;
    cursor: pointer;
  }

  button > span:last-child {
    grid-column: 2 / -1;
  }

  .action-icon {
    display: grid;
    place-items: center;
    color: var(--color-foreground-darker);
  }

  .primary {
    font-weight: 600;
  }

  .primary .action-icon {
    color: var(--color-accent);
  }

  button:hover:enabled {
    background: var(--color-hover);
  }

  button:focus-visible {
    outline-offset: -2px;
  }

  button:disabled {
    color: color-mix(in srgb, var(--color-foreground-darker) 84%, var(--color-background));
    cursor: default;
  }

  button:disabled .action-icon {
    color: inherit;
    opacity: 0.7;
  }
</style>
