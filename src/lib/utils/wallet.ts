import { onSettingsChanged, onWalletChanged } from '$lib/ipc';
import type { UnlistenFn } from '@tauri-apps/api/event';

// Only lives in mounted views. Refresh at local midnight, on focus and on changes.
export function watchWallet(refresh: () => Promise<void>): () => void {
  let disposed = false;
  let midnight: ReturnType<typeof setTimeout>;
  const cleanups: UnlistenFn[] = [];
  const update = () => {
    if (!disposed) void refresh();
  };
  function scheduleMidnight() {
    const now = new Date();
    const next = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
    midnight = setTimeout(
      () => {
        update();
        scheduleMidnight();
      },
      next.getTime() - now.getTime() + 50
    );
  }
  window.addEventListener('focus', update);
  scheduleMidnight();
  void (async () => {
    try {
      for (const subscribe of [onWalletChanged, onSettingsChanged]) {
        const unlisten = await subscribe(update);
        if (disposed) unlisten();
        else cleanups.push(unlisten);
      }
    } finally {
      update();
    }
  })().catch(() => {});
  return () => {
    disposed = true;
    clearTimeout(midnight);
    window.removeEventListener('focus', update);
    for (const cleanup of cleanups) cleanup();
  };
}
