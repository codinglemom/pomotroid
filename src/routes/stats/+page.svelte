<script lang="ts">
  import '../../app.css';
  import { onMount, tick } from 'svelte';
  import {
    getSettings,
    getThemes,
    watchWindowSettings,
    onThemesChanged,
    onRoundChange,
    onSessionsCleared,
    onWalletError,
    statsGetDetailed,
    statsGetHeatmap,
  } from '$lib/ipc';
  import { settings } from '$lib/stores/settings';
  import { applyTheme } from '$lib/stores/theme';
  import { setLocale } from '$lib/locale.svelte.js';
  import { resolveThemeName } from '$lib/utils/theme';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { isMac } from '$lib/utils/platform';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import type { DetailedStats, HeatmapStats } from '$lib/types';
  import * as m from '$paraglide/messages.js';
  import { info, error as logError } from '@tauri-apps/plugin-log';

  import DailyView from '$lib/components/stats/DailyView.svelte';
  import WeeklyView from '$lib/components/stats/WeeklyView.svelte';
  import YearlyView from '$lib/components/stats/YearlyView.svelte';
  import RewardWallet from '$lib/components/stats/RewardWallet.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import { WINDOW_NAVIGATE } from '$lib/utils/windows';

  type Tab = 'today' | 'week' | 'alltime' | 'wallet';

  let activeTab = $state<Tab>('today');
  let detailed = $state<DetailedStats | null>(null);
  let heatmap = $state<HeatmapStats | null>(null);
  let heatmapLoaded = $state(false);
  let walletFailure = $state(false);
  let statsYear = $state(new Date().getFullYear());

  async function switchTab(tab: Tab) {
    activeTab = tab;
    if (tab === 'alltime' && !heatmapLoaded) {
      try {
        heatmap = await statsGetHeatmap();
        heatmapLoaded = true;
      } catch (e) {
        await logError(`[stats] failed to load heatmap: ${e}`);
      }
    }
  }

  function close() {
    getCurrentWebviewWindow().close();
  }

  onMount(() => {
    const cleanups: UnlistenFn[] = [];
    let disposed = false;
    if (new URLSearchParams(window.location.search).get('tab') === 'wallet') activeTab = 'wallet';

    (async () => {
      const unlisten = await getCurrentWebviewWindow().listen<string>(
        WINDOW_NAVIGATE,
        ({ payload }) => {
          if (payload === 'wallet') void switchTab('wallet');
        }
      );
      if (disposed) {
        unlisten();
        return;
      }
      cleanups.push(unlisten);
      try {
        const [s, themes, initialDetailed] = await Promise.all([
          getSettings(),
          getThemes(),
          statsGetDetailed().catch((e) => {
            void logError(`[stats] failed to load initial stats: ${e}`).catch(() => {});
            return null;
          }),
        ]);
        detailed = initialDetailed;
        settings.set(s);
        setLocale(s.language);
        void info(`[stats] settings loaded, locale=${s.language}`).catch(() => {});

        const osDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
        const activeTheme = themes.find((t) => t.name === resolveThemeName(s, osDark)) ?? themes[0];
        if (activeTheme) await applyTheme(activeTheme);

        // Present the populated first view with its final theme and local fonts.
        await tick();
        await document.fonts.ready;
        await getCurrentWebviewWindow().show();
        await getCurrentWebviewWindow().setFocus();

        void info(`[stats] initialized, theme=${activeTheme?.name ?? 'none'}`).catch(() => {});
      } catch (e) {
        await logError(`[stats] initialization failed: ${e}`);
        throw e;
      }

      // Refresh date-sensitive views whenever the window regains focus.
      cleanups.push(
        await getCurrentWebviewWindow().onFocusChanged(async ({ payload: focused }) => {
          if (!focused) return;
          statsYear = new Date().getFullYear();
          try {
            detailed = await statsGetDetailed();
            if (heatmapLoaded) heatmap = await statsGetHeatmap();
          } catch (e) {
            await logError(`[stats] failed to refresh stats on focus: ${e}`);
          }
        }),
        await onWalletError(() => {
          walletFailure = true;
        }),
        await onRoundChange(async () => {
          try {
            detailed = await statsGetDetailed();
            if (heatmapLoaded) heatmap = await statsGetHeatmap();
          } catch (e) {
            await logError(`[stats] failed to refresh stats after round change: ${e}`);
          }
        }),
        await onSessionsCleared(async () => {
          try {
            detailed = await statsGetDetailed();
            if (heatmapLoaded) heatmap = await statsGetHeatmap();
          } catch (e) {
            await logError(`[stats] failed to refresh stats after session clear: ${e}`);
          }
        }),
        await watchWindowSettings(async (updated) => {
          const prev = {
            mode: $settings.theme_mode,
            light: $settings.theme_light,
            dark: $settings.theme_dark,
            language: $settings.language,
          };
          settings.set(updated);
          if (updated.language !== prev.language) {
            setLocale(updated.language);
          }
          if (
            updated.theme_mode !== prev.mode ||
            updated.theme_light !== prev.light ||
            updated.theme_dark !== prev.dark
          ) {
            const allThemes = await getThemes();
            const dark = window.matchMedia('(prefers-color-scheme: dark)').matches;
            const t = allThemes.find((th) => th.name === resolveThemeName(updated, dark));
            if (t) applyTheme(t);
          }
        }),
        await onThemesChanged((updated) => {
          const dark = window.matchMedia('(prefers-color-scheme: dark)').matches;
          const current =
            updated.find((t) => t.name === resolveThemeName($settings, dark)) ?? updated[0];
          if (current) applyTheme(current);
        })
      );

      const mq = window.matchMedia('(prefers-color-scheme: dark)');
      const mqListener = async (e: MediaQueryListEvent) => {
        if ($settings.theme_mode !== 'auto') return;
        const allThemes = await getThemes();
        const t = allThemes.find((th) => th.name === resolveThemeName($settings, e.matches));
        if (t) applyTheme(t);
      };
      mq.addEventListener('change', mqListener);
      cleanups.push(() => mq.removeEventListener('change', mqListener));
    })();

    return () => {
      disposed = true;
      for (const fn of cleanups) fn();
    };
  });
</script>

<div class="window">
  <!-- Titlebar -->
  <nav class="titlebar" class:macos={isMac} data-tauri-drag-region>
    <span class="titlebar-label">小番茄 · {m.stats_title()}</span>
    {#if !isMac}
      <button class="btn-close" onclick={close} aria-label={m.ui_close()}>
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
  </nav>

  <!-- Tab bar -->
  <nav class="tabs" aria-label={m.stats_title()}>
    <button
      class="tab"
      class:active={activeTab === 'today'}
      aria-current={activeTab === 'today' ? 'page' : undefined}
      onclick={() => switchTab('today')}>{m.stats_tab_today()}</button
    >
    <button
      class="tab"
      class:active={activeTab === 'week'}
      aria-current={activeTab === 'week' ? 'page' : undefined}
      onclick={() => switchTab('week')}>{m.stats_tab_week()}</button
    >
    <button
      class="tab"
      class:active={activeTab === 'alltime'}
      aria-current={activeTab === 'alltime' ? 'page' : undefined}
      onclick={() => switchTab('alltime')}>{m.stats_tab_alltime()}</button
    >
    <button
      class="tab wallet-tab"
      class:active={activeTab === 'wallet'}
      aria-current={activeTab === 'wallet' ? 'page' : undefined}
      onclick={() => switchTab('wallet')}><Icon name="coin" size={16} />{m.wallet_title()}</button
    >
  </nav>

  {#if walletFailure}
    <p class="wallet-error" role="alert">{m.wallet_recording_failed()}</p>
  {/if}

  <!-- Content -->
  <div class="content">
    {#if activeTab === 'today'}
      <DailyView today={detailed?.today ?? null} />
    {:else if activeTab === 'week'}
      <WeeklyView week={detailed?.week ?? null} streak={detailed?.streak ?? null} />
    {:else if activeTab === 'alltime'}
      {#key statsYear}
        <YearlyView {heatmap} />
      {/key}
    {:else}
      <RewardWallet />
    {/if}
  </div>
</div>

<style>
  .window {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: transparent;
    color: var(--color-foreground);
    overflow: hidden;
  }
  .titlebar {
    height: 40px;
    display: flex;
    align-items: center;
    position: relative;
    flex-shrink: 0;
    border-bottom: 1px solid var(--color-separator);
    padding: 0 16px;
    background: var(--surface-sidebar);
  }
  .titlebar.macos {
    padding-left: 76px;
  }
  .titlebar-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--color-foreground-darker);
    pointer-events: none;
  }
  .btn-close {
    position: absolute;
    right: 8px;
    border: 0;
    background: transparent;
    cursor: pointer;
    color: var(--color-foreground-darker);
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 6px;
  }
  .btn-close:hover {
    background: var(--color-error);
    color: white;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 10px 24px;
    min-height: 56px;
    border-bottom: 1px solid var(--color-separator);
    flex-shrink: 0;
  }
  .tab {
    border: 0;
    border-radius: 6px;
    padding: 6px 14px;
    min-height: 32px;
    font-size: 12px;
    color: var(--color-foreground-darker);
    background: transparent;
    cursor: pointer;
  }
  .tab:hover {
    color: var(--color-foreground);
    background: var(--color-hover);
  }
  .wallet-tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .tab.active {
    color: var(--color-foreground);
    background: var(--surface-panel);
    font-weight: 600;
    box-shadow: 0 1px 3px rgb(0 0 0 / 7%);
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .wallet-error {
    padding: 8px 24px;
    color: var(--color-error);
    background: var(--surface-panel);
    font-size: 12px;
    flex-shrink: 0;
  }
</style>
