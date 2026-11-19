import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { get } from 'svelte/store';
import { settings } from '$lib/stores/settings';
import * as m from '$paraglide/messages.js';
import { isMac, isWindows } from './platform';

export const WINDOW_NAVIGATE = 'window:navigate';
const openingWindows = new Map<string, string | undefined>();

export const openSettings = (section?: 'rewards') => openWindow('settings', section);
export const openStats = (tab?: 'wallet') => openWindow('stats', tab);

async function openWindow(label: 'settings' | 'stats', selection?: string) {
  if (openingWindows.has(label)) {
    if (selection) openingWindows.set(label, selection);
    return;
  }
  openingWindows.set(label, selection);
  let waitingForFocus = false;
  try {
    const existing = await WebviewWindow.getByLabel(label);
    if (existing) {
      if (selection) await existing.emitTo(label, WINDOW_NAVIGATE, selection);
      await existing.show();
      await existing.setFocus();
      return;
    }
    const query = selection ? `?${label === 'settings' ? 'section' : 'tab'}=${selection}` : '';
    const mode = get(settings).theme_mode;
    const win = new WebviewWindow(label, {
      url: `/${label}${query}`,
      title: `小番茄 — ${label === 'settings' ? m.settings_title() : m.stats_title()}`,
      width: label === 'settings' ? 720 : 840,
      height: 520,
      decorations: isMac,
      transparent: isWindows,
      backgroundColor: isWindows ? '#00000000' : undefined,
      theme: isWindows && (mode === 'light' || mode === 'dark') ? mode : undefined,
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      titleBarStyle: isMac ? ('Overlay' as any) : undefined,
      hiddenTitle: isMac ? true : undefined,
      resizable: false,
      visible: false,
    });
    await win.once('tauri://focus', async () => {
      const requested = openingWindows.get(label);
      openingWindows.delete(label);
      if (requested) await win.emitTo(label, WINDOW_NAVIGATE, requested);
    });
    await win.once('tauri://error', () => openingWindows.delete(label));
    waitingForFocus = true;
  } finally {
    if (!waitingForFocus) openingWindows.delete(label);
  }
}
