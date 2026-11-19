<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { error as logError } from '@tauri-apps/plugin-log';
  import { desktopWidgetReady, restoreMainWindow } from '$lib/ipc';
  import type { TimerState, TimerStyle } from '$lib/types';
  import * as m from '$paraglide/messages.js';
  import TimerDisplay from './TimerDisplay.svelte';

  let {
    snapshot,
    round,
    styleName,
    countdown,
    ready,
  }: {
    snapshot: TimerState;
    round: string;
    styleName: TimerStyle;
    countdown: boolean;
    ready: boolean;
  } = $props();

  let visibleStyle = $state<'flip' | 'nixie'>();
  let announcing = false;
  let restoring = $state(false);
  let restoreFailed = $state(false);

  $effect(() => {
    if (!ready) return;
    if (styleName !== 'flip' && styleName !== 'nixie') {
      untrack(() => void restore());
      return;
    }
    visibleStyle = styleName;
    if (!announcing) {
      announcing = true;
      void announceReady();
    }
  });

  async function announceReady() {
    try {
      await tick();
      await document.fonts.ready;
      if (styleName !== 'flip' && styleName !== 'nixie') {
        announcing = false;
        return;
      }
      await desktopWidgetReady();
    } catch (error) {
      void logError(`[widget] could not show desktop timer: ${error}`).catch(() => {});
      await restore();
    }
  }

  async function restore() {
    if (restoring) return;
    restoring = true;
    restoreFailed = false;
    try {
      await restoreMainWindow();
    } catch (error) {
      restoreFailed = true;
      void logError(`[widget] could not restore the main window: ${error}`).catch(() => {});
    } finally {
      restoring = false;
    }
  }

  function startDragging(event: MouseEvent) {
    if (event.button !== 0 || event.target !== event.currentTarget) return;
    void getCurrentWebviewWindow()
      .startDragging()
      .catch((error) => {
        void logError(`[widget] could not move desktop timer: ${error}`).catch(() => {});
      });
  }
</script>

<div
  class="desktop-widget"
  style="--round-color:var(--color-{round}); --round-ink:var(--color-on-{round})"
  role="presentation"
  onmousedown={startDragging}
>
  {#if visibleStyle}
    <TimerDisplay state={snapshot} styleName={visibleStyle} {countdown} widget />
  {/if}
  <button
    class="restore"
    aria-label={m.widget_restore()}
    title={m.widget_restore()}
    aria-disabled={restoring}
    onclick={restore}
  >
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      stroke-width="1.4"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M9 2h5v5m0-5L8 8" />
      <path d="M6 3H3a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1v-3" />
    </svg>
  </button>
  {#if restoreFailed}<span class="error" role="status">{m.widget_restore_failed()}</span>{/if}
</div>

<style>
  .desktop-widget {
    position: relative;
    width: 100%;
    height: 100%;
    display: grid;
    grid-template-columns: 1fr 32px;
    grid-template-rows: minmax(0, 1fr) 32px;
    column-gap: 8px;
    align-items: center;
    padding: 4px 9px;
    cursor: grab;
  }
  .desktop-widget:active {
    cursor: grabbing;
  }
  .restore {
    grid-column: 2;
    grid-row: 2;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--color-background);
    color: var(--color-foreground);
    box-shadow: 0 0 0 1px #201f1e80;
    cursor: pointer;
  }
  .restore:hover,
  .restore:focus-visible {
    background: var(--color-foreground);
    color: var(--color-background);
  }
  .restore[aria-disabled='true'] {
    cursor: wait;
  }
  .error {
    position: absolute;
    inset-inline: 9px;
    top: 9px;
    padding: 5px 8px;
    border-radius: var(--radius-control);
    background: var(--color-background);
    color: var(--color-error);
    font-size: 12px;
    pointer-events: none;
  }
</style>
