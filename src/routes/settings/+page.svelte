<script lang="ts">
  import '../../app.css';
  import { onMount, tick } from 'svelte';
  import { getSettings, getThemes, watchWindowSettings, onThemesChanged } from '$lib/ipc';
  import { settings } from '$lib/stores/settings';
  import { applyTheme } from '$lib/stores/theme';
  import { resolveThemeName } from '$lib/utils/theme';
  import { setLocale } from '$lib/locale.svelte.js';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import { info, error as logError } from '@tauri-apps/plugin-log';
  import { createLocalShortcutHandler } from '$lib/utils/localShortcuts';

  import SettingsTitlebar from '$lib/components/settings/SettingsTitlebar.svelte';
  import TimerSection from '$lib/components/settings/sections/TimerSection.svelte';
  import AppearanceSection from '$lib/components/settings/sections/AppearanceSection.svelte';
  import NotificationsSection from '$lib/components/settings/sections/NotificationsSection.svelte';
  import RewardsSection from '$lib/components/settings/sections/RewardsSection.svelte';
  import { WINDOW_NAVIGATE } from '$lib/utils/windows';
  import ShortcutsSection from '$lib/components/settings/sections/ShortcutsSection.svelte';
  import SystemSection from '$lib/components/settings/sections/SystemSection.svelte';
  import AboutSection from '$lib/components/settings/sections/AboutSection.svelte';

  import * as m from '$paraglide/messages.js';
  import Icon, { type IconName } from '$lib/components/Icon.svelte';

  type Section =
    | 'timer'
    | 'appearance'
    | 'notifications'
    | 'rewards'
    | 'shortcuts'
    | 'system'
    | 'about';

  const SECTIONS: { id: Section; label: () => string }[] = [
    { id: 'timer', label: m.nav_timer },
    { id: 'appearance', label: m.nav_appearance },
    { id: 'notifications', label: m.nav_notifications },
    { id: 'rewards', label: m.wallet_title },
    { id: 'shortcuts', label: m.nav_shortcuts },
    { id: 'system', label: m.nav_system },
    { id: 'about', label: m.nav_about },
  ];

  let active = $state<Section>('timer');
  const sectionIcons: Record<Section, IconName> = {
    timer: 'clock',
    appearance: 'palette',
    notifications: 'bell',
    rewards: 'wallet',
    shortcuts: 'keyboard',
    system: 'settings',
    about: 'info',
  };

  // Local shortcut state for the settings window.
  let localVolume = $state(1.0);
  let preMuteVolume = $state(0.5);
  let isFullscreen = $state(false);

  onMount(() => {
    const cleanups: UnlistenFn[] = [];
    let disposed = false;
    const section = new URLSearchParams(window.location.search).get('section');
    if (section === 'rewards') active = section;

    // Mount local keyboard shortcut handler.
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

    (async () => {
      const unlisten = await getCurrentWebviewWindow().listen<string>(
        WINDOW_NAVIGATE,
        ({ payload }) => {
          if (payload === 'rewards') active = payload;
        }
      );
      if (disposed) {
        unlisten();
        return;
      }
      cleanups.push(unlisten);
      try {
        const [s, themes] = await Promise.all([getSettings(), getThemes()]);
        settings.set(s);
        localVolume = s.volume;

        // Apply the stored locale on mount.
        setLocale(s.language);
        void info(`[settings] settings loaded, locale=${s.language}`).catch(() => {});

        const osDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
        const activeTheme = themes.find((t) => t.name === resolveThemeName(s, osDark)) ?? themes[0];
        if (activeTheme) await applyTheme(activeTheme);
        void info(`[settings] initialized, theme=${activeTheme?.name ?? 'none'}`).catch(() => {});

        // Flush localized controls and wait for local fonts before revealing the window.
        await tick();
        await document.fonts.ready;
        await getCurrentWebviewWindow().show();
        await getCurrentWebviewWindow().setFocus();
      } catch (e) {
        await logError(`[settings] initialization failed: ${e}`);
        throw e;
      }

      // Live OS color scheme changes — re-resolve only in auto mode.
      const mq = window.matchMedia('(prefers-color-scheme: dark)');
      const mqListener = async (e: MediaQueryListEvent) => {
        if ($settings.theme_mode !== 'auto') return;
        const allThemes = await getThemes();
        const t = allThemes.find((th) => th.name === resolveThemeName($settings, e.matches));
        if (t) applyTheme(t);
      };
      mq.addEventListener('change', mqListener);
      cleanups.push(() => mq.removeEventListener('change', mqListener));

      cleanups.push(
        await watchWindowSettings(async (updated) => {
          const prevMode = $settings.theme_mode;
          const prevLight = $settings.theme_light;
          const prevDark = $settings.theme_dark;
          const prevLanguage = $settings.language;
          settings.set(updated);
          localVolume = updated.volume;
          if (updated.language !== prevLanguage) {
            setLocale(updated.language);
          }
          if (
            updated.theme_mode !== prevMode ||
            updated.theme_light !== prevLight ||
            updated.theme_dark !== prevDark
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
    })();

    return () => {
      disposed = true;
      for (const fn of cleanups) fn();
    };
  });
</script>

<div class="window">
  <SettingsTitlebar />

  <div class="body">
    <!-- Left sidebar navigation -->
    <aside class="sidebar">
      <nav>
        {#each SECTIONS as section}
          <button
            class="nav-item"
            class:active={active === section.id}
            aria-current={active === section.id ? 'page' : undefined}
            onclick={() => {
              active = section.id;
            }}
          >
            <Icon name={sectionIcons[section.id]} size={18} />{section.label()}
          </button>
        {/each}
      </nav>
    </aside>

    <!-- Right content area -->
    <main class="content">
      {#if active === 'timer'}
        <TimerSection />
      {:else if active === 'appearance'}
        <AppearanceSection />
      {:else if active === 'notifications'}
        <NotificationsSection />
      {:else if active === 'rewards'}
        <RewardsSection />
      {:else if active === 'shortcuts'}
        <ShortcutsSection />
      {:else if active === 'system'}
        <SystemSection />
      {:else if active === 'about'}
        <AboutSection />
      {/if}
    </main>
  </div>
</div>

<style>
  .window {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: transparent;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    overflow: hidden;
  }
  .sidebar {
    width: 168px;
    flex-shrink: 0;
    border-right: 1px solid var(--color-separator);
    background: var(--surface-sidebar);
    overflow-y: auto;
    padding: 16px 10px;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 38px;
    padding: 8px 12px;
    border: 0;
    border-radius: 6px;
    text-align: left;
    font-size: 14px;
    color: var(--color-foreground-darker);
    background: transparent;
    cursor: pointer;
    position: relative;
  }
  .nav-item:hover {
    color: var(--color-foreground);
    background: var(--color-hover);
  }
  .nav-item.active {
    color: var(--color-foreground);
    background: var(--surface-panel);
    font-weight: 500;
  }
  .nav-item.active::before {
    content: '';
    position: absolute;
    left: 2px;
    top: 10px;
    bottom: 10px;
    width: 3px;
    border-radius: 3px;
    background: var(--color-accent);
  }
  .content {
    flex: 1;
    overflow-y: auto;
    min-width: 0;
  }
</style>
