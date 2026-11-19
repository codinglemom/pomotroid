<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { openSettings, openStats } from '$lib/utils/windows';
  import { openDesktopWidget, setSetting, setWindowVisibility } from '$lib/ipc';
  import { error as logError } from '@tauri-apps/plugin-log';
  import { settings } from '$lib/stores/settings';
  import { isMac } from '$lib/utils/platform';
  import Tooltip from './Tooltip.svelte';
  import * as m from '$paraglide/messages.js';

  let maximized = $state(false);
  let suppressTitlebarHover = $state(false);
  let savingPin = $state(false);
  let pinError = $state(false);
  let openingWidget = $state(false);
  let widgetOpenFailed = $state(false);
  const supportsWidget = $derived($settings.timer_style !== 'hourglass');

  async function openWidget() {
    if (openingWidget) return;
    openingWidget = true;
    widgetOpenFailed = false;
    try {
      await openDesktopWidget();
    } catch (error) {
      widgetOpenFailed = true;
      void logError(`[titlebar] could not open the desktop timer: ${error}`).catch(() => {});
    } finally {
      openingWidget = false;
    }
  }

  async function togglePin() {
    if (savingPin) return;
    savingPin = true;
    pinError = false;
    try {
      const updated = await setSetting('always_on_top', $settings.always_on_top ? 'false' : 'true');
      settings.set(updated);
    } catch (error) {
      pinError = true;
      void logError(`[titlebar] failed to change always-on-top: ${error}`).catch(() => {});
    } finally {
      savingPin = false;
    }
  }

  function blurTitlebarControl() {
    const active = document.activeElement;
    if (active instanceof HTMLElement && active.closest('.titlebar')) active.blur();
  }

  function suppressRestoredTitlebarState() {
    suppressTitlebarHover = true;
    blurTitlebarControl();
  }

  onMount(() => {
    const win = getCurrentWebviewWindow();
    win.isMaximized().then((v) => {
      maximized = v;
    });
    const unlisten = win.onResized(async () => {
      maximized = await win.isMaximized();
    });
    const clearRestoredTitlebarFocus = () => {
      if (suppressTitlebarHover) requestAnimationFrame(blurTitlebarControl);
    };
    const clearSuppressedTitlebarHover = () => {
      suppressTitlebarHover = false;
    };
    window.addEventListener('focus', clearRestoredTitlebarFocus);
    document.addEventListener('pointermove', clearSuppressedTitlebarHover);
    return () => {
      unlisten.then((fn) => fn());
      window.removeEventListener('focus', clearRestoredTitlebarFocus);
      document.removeEventListener('pointermove', clearSuppressedTitlebarHover);
    };
  });

  async function minimize() {
    suppressRestoredTitlebarState();
    if ($settings.min_to_tray) {
      await setWindowVisibility(false);
    } else {
      await getCurrentWebviewWindow().minimize();
    }
  }

  function toggleMaximize() {
    getCurrentWebviewWindow().toggleMaximize();
  }

  async function close() {
    suppressRestoredTitlebarState();
    await getCurrentWebviewWindow().close();
  }
</script>

{#snippet settingsBtn()}
  <Tooltip text={m.tooltip_settings()}>
    <button class="btn-icon" onclick={() => openSettings()} aria-label={m.tooltip_settings()}>
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
        <line
          x1="2"
          y1="4"
          x2="14"
          y2="4"
          stroke="currentColor"
          stroke-width="1.3"
          stroke-linecap="round"
        />
        <circle
          cx="5"
          cy="4"
          r="1.8"
          fill="var(--color-background)"
          stroke="currentColor"
          stroke-width="1.3"
        />
        <line
          x1="2"
          y1="8"
          x2="14"
          y2="8"
          stroke="currentColor"
          stroke-width="1.3"
          stroke-linecap="round"
        />
        <circle
          cx="11"
          cy="8"
          r="1.8"
          fill="var(--color-background)"
          stroke="currentColor"
          stroke-width="1.3"
        />
        <line
          x1="2"
          y1="12"
          x2="14"
          y2="12"
          stroke="currentColor"
          stroke-width="1.3"
          stroke-linecap="round"
        />
        <circle
          cx="7"
          cy="12"
          r="1.8"
          fill="var(--color-background)"
          stroke="currentColor"
          stroke-width="1.3"
        />
      </svg>
    </button>
  </Tooltip>
{/snippet}

{#snippet statsBtn()}
  <Tooltip text={m.tooltip_statistics()}>
    <button class="btn-icon" onclick={() => openStats()} aria-label={m.tooltip_statistics()}>
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
        <rect x="2" y="9" width="3" height="5" rx="0.5" fill="currentColor" opacity="0.6" />
        <rect x="6.5" y="5" width="3" height="9" rx="0.5" fill="currentColor" opacity="0.8" />
        <rect x="11" y="2" width="3" height="12" rx="0.5" fill="currentColor" />
      </svg>
    </button>
  </Tooltip>
{/snippet}

{#snippet pinBtn()}
  <Tooltip
    text={pinError
      ? m.tooltip_pin_failed()
      : $settings.always_on_top
        ? m.tooltip_pin_disable()
        : m.tooltip_pin_enable()}
  >
    <button
      class="btn-icon pin"
      class:active={$settings.always_on_top}
      onclick={togglePin}
      aria-label={m.system_toggle_aot()}
      aria-pressed={$settings.always_on_top}
      disabled={savingPin}
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
        <path
          d="M5 2h6M6 2v4.5L4 9v1h8V9l-2-2.5V2M8 10v4"
          stroke="currentColor"
          stroke-width="1.3"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>
  </Tooltip>
{/snippet}

{#snippet widgetBtn()}
  {#if supportsWidget}
    <Tooltip text={m.widget_open()}>
      <button
        class="btn-icon widget"
        onclick={openWidget}
        aria-label={m.widget_open()}
        aria-disabled={openingWidget}
      >
        <svg
          width="16"
          height="16"
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          stroke-width="1.3"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M7 12H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v3" />
          <rect x="9" y="9" width="5" height="4" rx="1" />
        </svg>
      </button>
    </Tooltip>
    {#if widgetOpenFailed}<span class="widget-error" role="status">
        {m.widget_open_failed()}
      </span>{/if}
  {/if}
{/snippet}

<nav class="titlebar" class:suppress-hover={suppressTitlebarHover} data-tauri-drag-region>
  <span class="app-name" class:macos={isMac} data-tauri-drag-region>小番茄</span>
  <!-- Left: settings + stats buttons on Linux/Windows. On macOS the traffic
       lights live here; the action buttons move to the right side instead. -->
  {#if !isMac}
    {@render settingsBtn()}
    {@render statsBtn()}
    {@render pinBtn()}
  {/if}

  <!-- Right: settings + stats buttons on macOS, window controls on Linux/Windows. -->
  <div class="controls">
    {#if isMac}
      {@render pinBtn()}
      {@render statsBtn()}
      {@render settingsBtn()}
      {@render widgetBtn()}
    {:else}
      {@render widgetBtn()}
      <button class="btn-icon" onclick={minimize} aria-label="Minimize">
        <svg width="12" height="12" viewBox="0 0 12 12">
          <line
            x1="1"
            y1="6"
            x2="11"
            y2="6"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
          />
        </svg>
      </button>
      <button
        class="btn-icon"
        onclick={toggleMaximize}
        aria-label={maximized ? 'Restore' : 'Maximize'}
      >
        {#if maximized}
          <svg width="12" height="12" viewBox="0 0 12 12">
            <rect
              x="3"
              y="1"
              width="8"
              height="8"
              rx="1"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
            />
            <path
              d="M1 4 L1 11 L8 11"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
        {:else}
          <svg width="12" height="12" viewBox="0 0 12 12">
            <rect
              x="1"
              y="1"
              width="10"
              height="10"
              rx="1"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
            />
          </svg>
        {/if}
      </button>
      <button class="btn-icon close" onclick={close} aria-label="Close">
        <svg width="12" height="12" viewBox="0 0 12 12">
          <line
            x1="1"
            y1="1"
            x2="11"
            y2="11"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
          />
          <line
            x1="11"
            y1="1"
            x2="1"
            y2="11"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
          />
        </svg>
      </button>
    {/if}
  </div>
</nav>

<style>
  .titlebar {
    height: 40px;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 8px;
    position: relative;
    flex-shrink: 0;
  }

  .controls {
    display: flex;
    gap: 4px;
    margin-left: auto;
  }

  .btn-icon {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--color-foreground-darker, var(--color-foreground));
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 4px;
    transition:
      color 0.15s,
      background 0.15s;
  }

  .btn-icon:focus {
    outline: none;
  }

  .btn-icon:focus-visible {
    outline: 2px solid color-mix(in oklch, var(--color-foreground) 45%, transparent);
    outline-offset: 2px;
  }

  .titlebar:not(.suppress-hover) .btn-icon:hover {
    color: var(--color-foreground);
    background: var(--color-hover);
  }

  .titlebar:not(.suppress-hover) .btn-icon.close:hover {
    color: var(--color-background);
    background: var(--color-focus-round);
  }

  .btn-icon.pin.active,
  .titlebar:not(.suppress-hover) .btn-icon.pin.active:hover {
    color: #059669;
    background: color-mix(in srgb, #059669 10%, transparent);
  }

  .btn-icon.pin:disabled {
    cursor: wait;
    opacity: 0.65;
  }
  .btn-icon.widget[aria-disabled='true'] {
    cursor: wait;
  }
  .widget-error {
    position: absolute;
    z-index: 1;
    top: 100%;
    right: 8px;
    left: 8px;
    padding: 5px 8px;
    border-radius: var(--radius-control);
    background: var(--color-background);
    color: var(--color-error);
    font-size: 12px;
  }

  .app-name {
    font-size: 12px;
    font-weight: 500;
    color: var(--color-foreground);
    margin-right: auto;
    margin-left: 6px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .app-name.macos {
    margin-left: 76px;
  }
  .controls {
    gap: 0;
    margin-left: 8px;
  }
  .btn-icon {
    width: 30px;
    height: 32px;
    border-radius: 6px;
  }
  .btn-icon.pin.active,
  .titlebar:not(.suppress-hover) .btn-icon.pin.active:hover {
    color: var(--color-income);
    background: color-mix(in srgb, var(--color-income) 10%, transparent);
  }
  @media (max-width: 300px) {
    .app-name {
      display: none;
    }
    .controls {
      margin-left: auto;
    }
  }
</style>
