// Theme store.
// Applies theme colors to CSS custom properties on :root.

import { get } from 'svelte/store';
import { warn } from '@tauri-apps/plugin-log';
import { applyWindowMaterial } from '$lib/ipc';
import { settings } from './settings';
import { isWindows } from '$lib/utils/platform';
import type { Theme } from '$lib/types';

let revision = 0;
let materialUpdate = Promise.resolve();
let latestTheme: Theme | null = null;
let transparencyPreference: MediaQueryList | null = null;
let colorContext: CanvasRenderingContext2D | null | undefined;
let nativeMaterial: { mode: string; enabled: boolean; acrylic: boolean } | null = null;

function luminance(color: string): number {
  let channels: number[];
  if (/^#[\da-f]{6}$/i.test(color)) {
    // Bundled themes use hex; avoid a synchronous graphics readback for each color.
    channels = [1, 3, 5].map((offset) => parseInt(color.slice(offset, offset + 2), 16));
  } else {
    if (colorContext === undefined) {
      const canvas = document.createElement('canvas');
      canvas.width = canvas.height = 1;
      colorContext = canvas.getContext('2d', { willReadFrequently: true });
    }
    if (!colorContext) return 0;
    // Keep browser conversion for modern CSS colors and custom theme syntax.
    colorContext.clearRect(0, 0, 1, 1);
    colorContext.fillStyle = '#202321';
    colorContext.fillStyle = color;
    colorContext.fillRect(0, 0, 1, 1);
    channels = Array.from(colorContext.getImageData(0, 0, 1, 1).data.slice(0, 3));
  }
  return channels.reduce((sum, channel, index) => {
    const value = channel / 255;
    const linear = value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
    return sum + linear * [0.2126, 0.7152, 0.0722][index];
  }, 0);
}

function onColor(color: string): string {
  const value = luminance(color);
  return 1.05 / (value + 0.05) >= (value + 0.05) / 0.067 ? '#ffffff' : '#27221f';
}

/** Apply a theme's colors to the document root CSS custom properties.
 *  Theme keys already include the `--` prefix (e.g. "--color-background"). */
export function applyTheme(theme: Theme, { syncMaterial = true } = {}): Promise<void> {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(theme.colors)) {
    root.style.setProperty(key, value);
  }
  const dark = luminance(theme.colors['--color-background'] ?? '#202321') < 0.3;
  root.dataset.colorMode = dark ? 'dark' : 'light';
  root.style.colorScheme = dark ? 'dark' : 'light';
  root.style.setProperty('--color-income', dark ? '#9bc9ab' : '#356d4b');
  root.style.setProperty('--color-expense', dark ? '#f3a1ac' : '#b13f4b');
  root.style.setProperty('--color-error', dark ? '#ffa6ab' : '#a82e3d');
  for (const role of ['accent', 'focus-round', 'short-round', 'long-round']) {
    root.style.setProperty(
      `--color-on-${role}`,
      onColor(theme.colors[`--color-${role}`] ?? '#b44f40')
    );
  }
  if (!syncMaterial) return Promise.resolve();

  latestTheme = theme;
  if (!transparencyPreference) {
    transparencyPreference = matchMedia('(prefers-reduced-transparency: reduce)');
    transparencyPreference.addEventListener('change', () => {
      if (latestTheme) void applyTheme(latestTheme);
    });
  }
  const request = ++revision;
  const currentMode = get(settings).theme_mode;
  const mode = currentMode === 'light' || currentMode === 'dark' ? currentMode : 'auto';
  const enabled = isWindows && !transparencyPreference.matches;
  materialUpdate = materialUpdate.then(async () => {
    if (request !== revision) return;
    try {
      if (nativeMaterial?.mode !== mode || nativeMaterial.enabled !== enabled) {
        const acrylic = await applyWindowMaterial(mode, enabled);
        nativeMaterial = { mode, enabled, acrylic };
      }
      const acrylic = nativeMaterial.acrylic;
      if (request === revision) root.dataset.material = acrylic ? 'acrylic' : 'solid';
    } catch (error) {
      nativeMaterial = null;
      if (request === revision) root.dataset.material = 'solid';
      void warn(`[theme] native material unavailable: ${String(error)}`).catch(() => {});
    }
  });
  return materialUpdate;
}
