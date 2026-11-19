<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import Titlebar from '$lib/components/Titlebar.svelte';
  import Timer from '$lib/components/Timer.svelte';
  import {
    getThemes,
    watchWindowSettings,
    onThemesChanged,
    setWindowVisibility,
    restoreMainWindow,
  } from '$lib/ipc';
  import { settings } from '$lib/stores/settings';
  import { applyTheme } from '$lib/stores/theme';
  import { resolveThemeName } from '$lib/utils/theme';
  import { isMac } from '$lib/utils/platform';
  import { setLocale } from '$lib/locale.svelte.js';
  import { isTauri } from '@tauri-apps/api/core';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import { info, error as logError } from '@tauri-apps/plugin-log';
  import { createLocalShortcutHandler } from '$lib/utils/localShortcuts';
  import type { Theme } from '$lib/types';

  const isWidget = isTauri() && getCurrentWebviewWindow().label === 'widget';
  const windowLabel = isWidget ? 'widget' : 'main';
  let widgetSettingsReady = $state(false);

  // Local shortcut state — volume and fullscreen tracked separately so the
  // handler can read current values without waiting for settings:changed round-trip.
  let localVolume = $state(1.0);
  let preMuteVolume = $state(0.5);
  let isFullscreen = $state(false);

  // Base window dimensions (natural/default size).
  const BASE_W = 360;
  const BASE_H = 478;
  const TITLEBAR_H = 40;

  // Compact mode: when either dimension drops below this threshold,
  // hide non-essential elements (footer, label, play/pause) to show
  // only the timer dial — like an Apple Watch face.
  const COMPACT_THRESHOLD = 300;

  let uiScale = $state(1.0);
  let isCompact = $state(false);

  // Extra bottom padding added to <main> in compact mode.  Shifts the
  // dial upward so the whitespace sits at the bottom rather than being
  // split equally — compensates for the visual weight of the titlebar.
  const COMPACT_BOTTOM_PAD = 48;

  $effect(() => {
    if (isWidget) return;

    function update() {
      const w = window.innerWidth;
      const h = window.innerHeight;
      isCompact = w < COMPACT_THRESHOLD || h < COMPACT_THRESHOLD;
      if (isCompact) {
        // Scale so the dial fills the available space, reserving
        // COMPACT_BOTTOM_PAD px for the intentional bottom whitespace.
        const available = Math.min(w - 16, h - TITLEBAR_H - 16 - COMPACT_BOTTOM_PAD);
        uiScale = Math.max(0.4, Math.min(available / 220, 4));
      } else {
        // Scale proportionally to the base window dimensions.
        uiScale = Math.max(0.5, Math.min(w / BASE_W, (h - TITLEBAR_H) / (BASE_H - TITLEBAR_H), 4));
      }
    }
    update();
    window.addEventListener('resize', update);
    return () => window.removeEventListener('resize', update);
  });

  async function startResize(direction: string) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    await getCurrentWebviewWindow().startResizeDragging(direction as any);
  }

  function restoreAfterWidgetInitError() {
    void restoreMainWindow().catch((error) => {
      void logError(`[widget] initialization recovery failed: ${error}`).catch(() => {});
    });
  }

  onMount(() => {
    const cleanups: UnlistenFn[] = [];
    let disposed = false;

    if (isWidget) {
      document.documentElement.classList.add('desktop-widget-window');
      cleanups.push(() => document.documentElement.classList.remove('desktop-widget-window'));
    } else {
      const shortcutHandler = createLocalShortcutHandler({
        getSettings: () => $settings,
        getVolume: () => localVolume,
        setVolume: (v) => {
          localVolume = v;
        },
        getPreMuteVolume: () => preMuteVolume,
        setPreMuteVolume: (v) => {
          preMuteVolume = v;
        },
        getFullscreen: () => isFullscreen,
        setFullscreen: (v) => {
          isFullscreen = v;
        },
      });
      document.addEventListener('keydown', shortcutHandler);
      cleanups.push(() => document.removeEventListener('keydown', shortcutHandler));
    }

    const colorScheme = window.matchMedia('(prefers-color-scheme: dark)');
    let themes: Theme[] | undefined;
    let localeReady: Promise<void> | undefined;

    function applyActiveTheme() {
      const name = resolveThemeName($settings, colorScheme.matches);
      const active = themes?.find((theme) => theme.name === name) ?? themes?.[0];
      if (active) return applyTheme(active, { syncMaterial: !isWidget });
    }

    const onColorSchemeChange = () => {
      if ($settings.theme_mode === 'auto') void applyActiveTheme();
    };
    colorScheme.addEventListener('change', onColorSchemeChange);
    cleanups.push(() => colorScheme.removeEventListener('change', onColorSchemeChange));

    (async () => {
      const stopSettings = await watchWindowSettings((updated) => {
        if (disposed) return;
        const previous = $settings;
        settings.set(updated);
        if (!isWidget) localVolume = updated.volume;
        if (!localeReady || updated.language !== previous.language) {
          localeReady = setLocale(updated.language, { syncNotifications: !isWidget });
        }
        if (
          updated.theme_mode !== previous.theme_mode ||
          updated.theme_light !== previous.theme_light ||
          updated.theme_dark !== previous.theme_dark
        ) {
          void applyActiveTheme();
        }
      });
      if (disposed) return stopSettings();
      cleanups.push(stopSettings);

      const stopThemes = await onThemesChanged((updated) => {
        if (disposed) return;
        themes = updated;
        void applyActiveTheme();
      });
      if (disposed) return stopThemes();
      cleanups.push(stopThemes);

      const initialThemes = await getThemes();
      if (disposed) return;
      themes ??= initialThemes;
      await applyActiveTheme();
      await localeReady;
      if (disposed) return;
      if (isWidget) {
        widgetSettingsReady = true;
      } else {
        await setWindowVisibility(true);
        void info(`[main] initialized, locale=${$settings.language}`).catch(() => {});
      }
    })().catch((error) => {
      if (disposed) return;
      void logError(`[${windowLabel}] initialization failed: ${error}`).catch(() => {});
      if (isWidget) restoreAfterWidgetInitError();
    });

    return () => {
      disposed = true;
      for (const fn of cleanups) fn();
    };
  });
</script>

{#if isWidget}
  <Timer widget {widgetSettingsReady} onInitError={restoreAfterWidgetInitError} />
{:else}
  <!-- Resize handles — invisible edge/corner strips for decorations-free windows.
       Not needed on macOS where native resizing is provided by decorations:true. -->
  {#if !isMac}
    <!-- N -->
    <div class="rh rh-n" onmousedown={() => startResize('North')} role="none"></div>
    <!-- S -->
    <div class="rh rh-s" onmousedown={() => startResize('South')} role="none"></div>
    <!-- E -->
    <div class="rh rh-e" onmousedown={() => startResize('East')} role="none"></div>
    <!-- W -->
    <div class="rh rh-w" onmousedown={() => startResize('West')} role="none"></div>
    <!-- NE -->
    <div class="rh rh-ne" onmousedown={() => startResize('NorthEast')} role="none"></div>
    <!-- NW -->
    <div class="rh rh-nw" onmousedown={() => startResize('NorthWest')} role="none"></div>
    <!-- SE -->
    <div class="rh rh-se" onmousedown={() => startResize('SouthEast')} role="none"></div>
    <!-- SW -->
    <div class="rh rh-sw" onmousedown={() => startResize('SouthWest')} role="none"></div>
  {/if}

  <div class="app">
    <Titlebar />
    <main class:compact={isCompact}>
      <Timer {isCompact} {uiScale} />
    </main>
  </div>
{/if}

<style>
  :global(html.desktop-widget-window),
  :global(html.desktop-widget-window body) {
    background: transparent;
  }

  .app {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: app-fade-in 0.4s var(--transition-slow) both;
  }

  main {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }

  main.compact {
    /* Bottom padding provides breathing room below the mini controls. */
    padding-bottom: 8px;
  }

  /* ---------------------------------------------------------------------------
     Resize handles — positioned outside/over the window edges so the user can
     grab them to resize a decoration-free window (needed on Linux/Wayland and
     GNOME with undecorated windows).
     --------------------------------------------------------------------------- */
  :global(.rh) {
    position: fixed;
    z-index: 9999;
  }

  /* Edge handles */
  :global(.rh-n) {
    top: 0;
    left: 6px;
    right: 6px;
    height: 5px;
    cursor: n-resize;
  }
  :global(.rh-s) {
    bottom: 0;
    left: 6px;
    right: 6px;
    height: 5px;
    cursor: s-resize;
  }
  :global(.rh-e) {
    right: 0;
    top: 6px;
    bottom: 6px;
    width: 5px;
    cursor: e-resize;
  }
  :global(.rh-w) {
    left: 0;
    top: 6px;
    bottom: 6px;
    width: 5px;
    cursor: w-resize;
  }

  /* Corner handles (larger for easier grabbing) */
  :global(.rh-ne) {
    top: 0;
    right: 0;
    width: 10px;
    height: 10px;
    cursor: ne-resize;
  }
  :global(.rh-nw) {
    top: 0;
    left: 0;
    width: 10px;
    height: 10px;
    cursor: nw-resize;
  }
  :global(.rh-se) {
    bottom: 0;
    right: 0;
    width: 10px;
    height: 10px;
    cursor: se-resize;
  }
  :global(.rh-sw) {
    bottom: 0;
    left: 0;
    width: 10px;
    height: 10px;
    cursor: sw-resize;
  }
</style>
