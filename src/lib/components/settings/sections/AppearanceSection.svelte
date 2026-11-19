<script lang="ts">
  import { onMount } from 'svelte';
  import type { Theme, TimerStyle } from '$lib/types';
  import { settings } from '$lib/stores/settings';
  import { applyTheme } from '$lib/stores/theme';
  import { getThemes, setSetting, onThemesChanged } from '$lib/ipc';
  import { resolveThemeName } from '$lib/utils/theme';
  import { isWindows } from '$lib/utils/platform';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import * as m from '$paraglide/messages.js';
  import DeskClock from '../../DeskClock.svelte';

  const timerStyles = [
    ['ring', m.appearance_timer_style_ring],
    ['flip', m.appearance_timer_style_flip],
    ['nixie', m.appearance_timer_style_nixie],
    ['hourglass', m.appearance_timer_style_hourglass],
  ] as const;

  let themes = $state<Theme[]>([]);
  let preview = $state<number | null>(null);
  let saving = $state(false);
  let saveFailed = $state(false);
  let transparency = $derived(preview ?? $settings.acrylic_transparency);
  let savingTimerStyle = $state(false);
  let timerStyleSaveFailed = $state(false);

  $effect(() => {
    if (preview === null) return;
    const style = document.documentElement.style;
    style.setProperty('--glass-preview', `${100 - preview}%`);
    return () => style.removeProperty('--glass-preview');
  });

  async function saveTransparency(value: number) {
    saving = true;
    saveFailed = false;
    try {
      settings.set(await setSetting('acrylic_transparency', String(value)));
    } catch {
      saveFailed = true;
    } finally {
      preview = null;
      saving = false;
    }
  }
  let osDark = $state(window.matchMedia('(prefers-color-scheme: dark)').matches);
  // Accordion: at most one picker open at a time.
  let openPicker = $state<'light' | 'dark' | null>(null);

  // Light picker is active when mode='light', or mode='auto' and OS is light.
  let lightIsActive = $derived(
    $settings.theme_mode === 'light' || ($settings.theme_mode === 'auto' && !osDark)
  );
  // Dark picker is active when mode='dark', or mode='auto' and OS is dark.
  let darkIsActive = $derived(
    $settings.theme_mode === 'dark' || ($settings.theme_mode === 'auto' && osDark)
  );

  let selectedLightTheme = $derived(themes.find((t) => t.name === $settings.theme_light));
  let selectedDarkTheme = $derived(themes.find((t) => t.name === $settings.theme_dark));

  function themeLabel(name: string) {
    if (name === 'Pomotroid') return `小番茄 · ${m.appearance_mode_dark()}`;
    if (name === 'Pomotroid Light') return `小番茄 · ${m.appearance_mode_light()}`;
    return name;
  }

  function togglePicker(picker: 'light' | 'dark') {
    openPicker = openPicker === picker ? null : picker;
  }

  onMount(() => {
    const cleanups: UnlistenFn[] = [];

    // Track live OS color scheme changes to update picker highlights.
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const mqListener = (e: MediaQueryListEvent) => {
      osDark = e.matches;
    };
    mq.addEventListener('change', mqListener);
    cleanups.push(() => mq.removeEventListener('change', mqListener));

    (async () => {
      themes = await getThemes();
      cleanups.push(
        await onThemesChanged((updated) => {
          themes = updated;
        })
      );
    })();

    return () => {
      for (const fn of cleanups) fn();
    };
  });

  // Mode selector: save + immediately apply the resolved theme.
  async function setMode(mode: string) {
    const updated = await setSetting('theme_mode', mode);
    settings.set(updated);
    const resolved = resolveThemeName(updated, osDark);
    const t = themes.find((th) => th.name === resolved);
    if (t) await applyTheme(t);
  }

  async function saveTimerStyle(style: TimerStyle) {
    if (savingTimerStyle || style === $settings.timer_style) return;
    savingTimerStyle = true;
    timerStyleSaveFailed = false;
    try {
      settings.set(await setSetting('timer_style', style));
    } catch {
      timerStyleSaveFailed = true;
    } finally {
      savingTimerStyle = false;
    }
  }

  // Light picker: save + apply only if light picker is currently active.
  async function selectLight(theme: Theme) {
    settings.set(await setSetting('theme_light', theme.name));
    if (lightIsActive) await applyTheme(theme);
  }

  // Dark picker: save + apply only if dark picker is currently active.
  async function selectDark(theme: Theme) {
    settings.set(await setSetting('theme_dark', theme.name));
    if (darkIsActive) await applyTheme(theme);
  }
</script>

<div class="section">
  <h2 class="page-title">{m.nav_appearance()}</h2>
  <!-- Mode selector -->
  <div class="group-label">{m.appearance_group_mode()}</div>
  <div class="mode-selector">
    {#each [['auto', m.appearance_mode_auto()], ['light', m.appearance_mode_light()], ['dark', m.appearance_mode_dark()]] as [value, label] (value)}
      <button
        class="mode-btn"
        class:active={$settings.theme_mode === value}
        aria-pressed={$settings.theme_mode === value}
        onclick={() => setMode(value)}
      >
        <span
          class="mode-preview"
          class:dark={value === 'dark'}
          class:auto={value === 'auto'}
          aria-hidden="true"><i></i></span
        ><span>{label}</span>
      </button>
    {/each}
  </div>

  <div class="group-label" id="timer-style-label">{m.appearance_timer_style()}</div>
  <div class="mode-selector timer-style-selector" role="group" aria-labelledby="timer-style-label">
    {#each timerStyles as [style, label] (style)}
      <button
        type="button"
        class="mode-btn timer-style-btn"
        class:active={$settings.timer_style === style}
        aria-pressed={$settings.timer_style === style}
        aria-disabled={savingTimerStyle}
        onkeydown={(event) => {
          if (event.key === ' ' || event.key === 'Enter') event.stopPropagation();
        }}
        onclick={() => saveTimerStyle(style)}
      >
        <span>{label()}</span>
        <div class="face-preview" aria-hidden="true">
          {#if style === 'ring'}
            <svg viewBox="0 0 64 64" width="50" height="50">
              <circle
                cx="32"
                cy="32"
                r="29"
                fill="none"
                stroke="var(--color-separator)"
                stroke-width="2.5"
              />
              <circle
                cx="32"
                cy="32"
                r="29"
                fill="none"
                stroke="var(--color-focus-round)"
                stroke-width="2.5"
                pathLength="100"
                stroke-dasharray="56 100"
                stroke-linecap="round"
                transform="rotate(-90 32 32)"
              />
              <text
                x="32"
                y="37"
                text-anchor="middle"
                fill="var(--color-foreground)"
                font-family="Mona Sans Mono, monospace"
                font-size="14">14:00</text
              >
            </svg>
          {:else}
            <div class="mini-face">
              <DeskClock styleName={style} display="14:00" remainingFraction={0.56} />
            </div>
          {/if}
        </div>
      </button>
    {/each}
  </div>
  {#if timerStyleSaveFailed}
    <div class="timer-style-error" role="alert">{m.appearance_timer_style_save_failed()}</div>
  {/if}

  {#if isWindows}
    <div class="glass-setting">
      <label for="glass-transparency">{m.appearance_transparency()}</label>
      <output for="glass-transparency">{transparency}%</output>
      <input
        id="glass-transparency"
        type="range"
        min="0"
        max="100"
        step="1"
        value={transparency}
        aria-valuetext={`${transparency}%`}
        disabled={saving}
        oninput={(event) => (preview = event.currentTarget.valueAsNumber)}
        onchange={(event) => void saveTransparency(event.currentTarget.valueAsNumber)}
      />
      {#if saveFailed}<span role="alert">{m.wallet_save_failed()}</span>{/if}
    </div>
  {/if}

  <!-- Light theme picker -->
  <div class="picker-group">
    <button
      class="picker-trigger"
      class:open={openPicker === 'light'}
      onclick={() => togglePicker('light')}
    >
      <span class="trigger-label">
        {m.appearance_group_light_theme()}
        {#if lightIsActive}<span class="active-badge">{m.appearance_badge_active()}</span>{/if}
      </span>
      <span class="trigger-preview">
        <span class="preview-name">{themeLabel($settings.theme_light)}</span>
        {#if selectedLightTheme}
          {@const bg = selectedLightTheme.colors['--color-background'] ?? '#2f384b'}
          {@const focusRound = selectedLightTheme.colors['--color-focus-round'] ?? '#e25d60'}
          {@const shortRound = selectedLightTheme.colors['--color-short-round'] ?? '#3baf82'}
          {@const longRound = selectedLightTheme.colors['--color-long-round'] ?? '#3d85c8'}
          <span class="preview-swatches" style="background:{bg}">
            <span class="swatch" style="background:{focusRound}"></span>
            <span class="swatch" style="background:{shortRound}"></span>
            <span class="swatch" style="background:{longRound}"></span>
          </span>
        {/if}
      </span>
      <svg
        class="chevron"
        class:rotated={openPicker === 'light'}
        width="14"
        height="14"
        viewBox="0 0 24 24"
        fill="currentColor"
      >
        <path d="M7 10l5 5 5-5z" />
      </svg>
    </button>
    {#if openPicker === 'light'}
      <div class="theme-list">
        {#each themes as theme (theme.name)}
          {@const bg = theme.colors['--color-background'] ?? '#2f384b'}
          {@const fg = theme.colors['--color-foreground'] ?? '#d7e1f4'}
          {@const accent = theme.colors['--color-accent'] ?? '#e25d60'}
          {@const focusRound = theme.colors['--color-focus-round'] ?? '#e25d60'}
          {@const shortRound = theme.colors['--color-short-round'] ?? '#3baf82'}
          {@const longRound = theme.colors['--color-long-round'] ?? '#3d85c8'}
          {@const isSelected = theme.name === $settings.theme_light}
          <button
            class="card"
            class:selected={isSelected}
            class:highlighted={isSelected && lightIsActive}
            style="--card-bg:{bg}; --card-fg:{fg}; --card-accent:{accent};"
            onclick={() => selectLight(theme)}
          >
            <span class="swatches">
              <span class="swatch" style="background:{focusRound}"></span>
              <span class="swatch" style="background:{shortRound}"></span>
              <span class="swatch" style="background:{longRound}"></span>
            </span>
            <span class="card-name" style="color:{fg}">{themeLabel(theme.name)}</span>
            <span class="card-right">
              {#if theme.is_custom}
                <span class="badge" style="color:{accent}">{m.appearance_badge_custom()}</span>
              {/if}
              {#if isSelected}
                <svg
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  style="fill:{accent}; flex-shrink:0;"
                >
                  <path d="M9 16.2L4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4L9 16.2z" />
                </svg>
              {/if}
            </span>
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Dark theme picker -->
  <div class="picker-group">
    <button
      class="picker-trigger"
      class:open={openPicker === 'dark'}
      onclick={() => togglePicker('dark')}
    >
      <span class="trigger-label">
        {m.appearance_group_dark_theme()}
        {#if darkIsActive}<span class="active-badge">{m.appearance_badge_active()}</span>{/if}
      </span>
      <span class="trigger-preview">
        <span class="preview-name">{themeLabel($settings.theme_dark)}</span>
        {#if selectedDarkTheme}
          {@const bg = selectedDarkTheme.colors['--color-background'] ?? '#2f384b'}
          {@const focusRound = selectedDarkTheme.colors['--color-focus-round'] ?? '#e25d60'}
          {@const shortRound = selectedDarkTheme.colors['--color-short-round'] ?? '#3baf82'}
          {@const longRound = selectedDarkTheme.colors['--color-long-round'] ?? '#3d85c8'}
          <span class="preview-swatches" style="background:{bg}">
            <span class="swatch" style="background:{focusRound}"></span>
            <span class="swatch" style="background:{shortRound}"></span>
            <span class="swatch" style="background:{longRound}"></span>
          </span>
        {/if}
      </span>
      <svg
        class="chevron"
        class:rotated={openPicker === 'dark'}
        width="14"
        height="14"
        viewBox="0 0 24 24"
        fill="currentColor"
      >
        <path d="M7 10l5 5 5-5z" />
      </svg>
    </button>
    {#if openPicker === 'dark'}
      <div class="theme-list">
        {#each themes as theme (theme.name)}
          {@const bg = theme.colors['--color-background'] ?? '#2f384b'}
          {@const fg = theme.colors['--color-foreground'] ?? '#d7e1f4'}
          {@const accent = theme.colors['--color-accent'] ?? '#e25d60'}
          {@const focusRound = theme.colors['--color-focus-round'] ?? '#e25d60'}
          {@const shortRound = theme.colors['--color-short-round'] ?? '#3baf82'}
          {@const longRound = theme.colors['--color-long-round'] ?? '#3d85c8'}
          {@const isSelected = theme.name === $settings.theme_dark}
          <button
            class="card"
            class:selected={isSelected}
            class:highlighted={isSelected && darkIsActive}
            style="--card-bg:{bg}; --card-fg:{fg}; --card-accent:{accent};"
            onclick={() => selectDark(theme)}
          >
            <span class="swatches">
              <span class="swatch" style="background:{focusRound}"></span>
              <span class="swatch" style="background:{shortRound}"></span>
              <span class="swatch" style="background:{longRound}"></span>
            </span>
            <span class="card-name" style="color:{fg}">{themeLabel(theme.name)}</span>
            <span class="card-right">
              {#if theme.is_custom}
                <span class="badge" style="color:{accent}">{m.appearance_badge_custom()}</span>
              {/if}
              {#if isSelected}
                <svg
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  style="fill:{accent}; flex-shrink:0;"
                >
                  <path d="M9 16.2L4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4L9 16.2z" />
                </svg>
              {/if}
            </span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .glass-setting {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 8px;
    margin: 12px 20px;
  }
  .glass-setting input,
  .glass-setting [role='alert'] {
    grid-column: 1 / -1;
  }
  .glass-setting input {
    width: 100%;
    accent-color: var(--color-accent);
  }
  .glass-setting output {
    font-variant-numeric: tabular-nums;
  }
  .section {
    display: flex;
    flex-direction: column;
  }

  .group-label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.68rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--color-foreground-darker, var(--color-foreground));
    opacity: 0.6;
    margin: 0;
    padding: 16px 20px 6px;
  }

  .active-badge {
    font-size: 0.6rem;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    padding: 1px 5px;
    border-radius: 3px;
    background: color-mix(in oklch, var(--color-accent) 20%, transparent);
    color: var(--color-accent);
    opacity: 1;
  }

  /* Mode selector */
  .mode-selector {
    display: flex;
    margin: 0 20px 4px;
    border: 1px solid var(--color-separator);
    border-radius: 6px;
    overflow: hidden;
  }

  .mode-btn {
    flex: 1;
    padding: 6px 0;
    background: none;
    border: none;
    border-right: 1px solid var(--color-separator);
    font-size: 0.8rem;
    letter-spacing: 0.03em;
    color: var(--color-foreground-darker, var(--color-foreground));
    cursor: pointer;
    transition:
      background 0.12s,
      color 0.12s;
  }

  .mode-btn:last-child {
    border-right: none;
  }

  .mode-btn:hover {
    background: color-mix(in oklch, var(--color-background) 88%, var(--color-foreground) 12%);
  }

  .mode-btn.active {
    background: color-mix(in oklch, var(--color-accent) 18%, transparent);
    color: var(--color-accent);
    font-weight: 600;
  }

  /* Collapsible picker groups */
  .picker-group {
    margin: 8px 20px 0;
    border: 1px solid var(--color-separator);
    border-radius: 6px;
    overflow: hidden;
  }

  .picker-group:last-child {
    margin-bottom: 12px;
  }

  .picker-trigger {
    display: flex;
    align-items: center;
    width: 100%;
    padding: 9px 12px;
    background: none;
    border: none;
    cursor: pointer;
    gap: 8px;
    transition: background 0.12s;
  }

  .picker-trigger:hover {
    background: color-mix(in oklch, var(--color-background) 88%, var(--color-foreground) 12%);
  }

  .picker-trigger.open {
    border-bottom: 1px solid var(--color-separator);
  }

  .trigger-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--color-foreground-darker, var(--color-foreground));
    opacity: 0.7;
    flex-shrink: 0;
  }

  .trigger-preview {
    display: flex;
    align-items: center;
    gap: 7px;
    flex: 1;
    justify-content: flex-end;
    min-width: 0;
  }

  .preview-swatches {
    display: flex;
    gap: 3px;
    flex-shrink: 0;
    padding: 4px 6px;
    border-radius: 4px;
  }

  .preview-name {
    font-size: 0.8rem;
    color: var(--color-foreground-darker, var(--color-foreground));
    opacity: 0.8;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .chevron {
    flex-shrink: 0;
    color: var(--color-foreground-darker, var(--color-foreground));
    opacity: 0.5;
    transition: transform 0.15s;
  }

  .chevron.rotated {
    transform: rotate(180deg);
  }

  /* Theme list */
  .theme-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px 10px;
  }

  .card {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-radius: 6px;
    border: 1px solid transparent;
    background: var(--card-bg);
    cursor: pointer;
    width: 100%;
    text-align: left;
    transition:
      border-color 0.15s,
      background 0.15s;
  }

  .card:hover {
    background: color-mix(in srgb, var(--card-bg) 88%, white 12%);
  }

  /* selected but NOT the active picker: dimmed border */
  .card.selected {
    border-color: color-mix(in oklch, var(--card-accent) 40%, transparent);
  }

  /* selected AND the active picker: full accent border + checkmark shown */
  .card.highlighted {
    border-color: var(--card-accent);
  }

  .swatches {
    display: flex;
    gap: 3px;
    flex-shrink: 0;
  }

  .swatch {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 1px solid color-mix(in oklch, var(--color-foreground) 14%, transparent);
  }

  .card-name {
    font-size: 0.85rem;
    letter-spacing: 0.03em;
    flex: 1;
  }

  .card-right {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .badge {
    font-size: 0.65rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    opacity: 0.8;
  }

  .page-title {
    margin: 24px 24px 4px;
    font-size: 20px;
    line-height: 28px;
    font-weight: 600;
  }
  .group-label {
    padding: 16px 24px 10px;
    font-size: 12px;
    opacity: 1;
    letter-spacing: 0;
    text-transform: none;
  }
  .mode-selector {
    margin: 0 24px 8px;
    gap: 12px;
    border: 0;
    overflow: visible;
  }
  .mode-btn {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    padding: 8px;
    border: 1px solid var(--color-separator);
    border-radius: 8px;
    background: var(--surface-panel);
    text-align: left;
    font-size: 12px;
    letter-spacing: 0;
  }
  .mode-btn:last-child {
    border-right: 1px solid var(--color-separator);
  }
  .mode-btn.active {
    border: 2px solid var(--color-accent);
    padding: 7px;
    background: var(--surface-panel);
    color: var(--color-foreground);
  }
  .timer-style-btn {
    align-items: center;
    justify-content: center;
    min-height: 36px;
    text-align: center;
  }
  .timer-style-selector {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
  }
  .face-preview {
    display: grid;
    place-items: center;
    width: 100%;
    height: 54px;
    pointer-events: none;
  }
  .mini-face {
    width: 250px;
    zoom: 0.32;
  }
  .timer-style-error {
    margin: 0 24px 8px;
    font-size: 12px;
  }
  .mode-preview {
    height: 62px;
    width: 100%;
    border: 1px solid #d9ded6;
    border-radius: 4px;
    background: #f5f5f2;
    display: grid;
    place-items: center;
  }
  .mode-preview.dark {
    background: #202321;
    border-color: #414a42;
  }
  .mode-preview.auto {
    background: linear-gradient(90deg, #f5f5f2 50%, #202321 50%);
  }
  .mode-preview i {
    width: 30px;
    height: 30px;
    border: 2px solid #b44f40;
    border-radius: 50%;
  }
  .mode-preview.dark i {
    border-color: #f0a08e;
  }
  .picker-group {
    margin: 12px 24px 0;
    border-radius: 10px;
    background: var(--surface-panel);
  }
  .picker-trigger {
    min-height: 52px;
    padding: 12px 16px;
    flex-wrap: wrap;
  }
  .trigger-label {
    opacity: 1;
    font-size: 12px;
    letter-spacing: 0;
    text-transform: none;
  }
  .preview-name {
    opacity: 1;
    font-size: 12px;
  }
  .active-badge {
    font-size: 11px;
    letter-spacing: 0;
    text-transform: none;
  }
</style>
