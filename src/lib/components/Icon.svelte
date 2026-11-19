<script module lang="ts">
  export type IconName =
    | 'play'
    | 'pause'
    | 'restart'
    | 'skip'
    | 'reset'
    | 'volume'
    | 'muted'
    | 'clock'
    | 'palette'
    | 'bell'
    | 'wallet'
    | 'coin'
    | 'keyboard'
    | 'settings'
    | 'info'
    | 'chart'
    | 'close'
    | 'window'
    | 'exit'
    | 'plus'
    | 'arrow-down-left'
    | 'arrow-up-right'
    | 'arrow';
</script>

<script lang="ts">
  let { name, size = 18 }: { name: IconName; size?: number } = $props();
  const paths: Record<IconName, string> = {
    play: 'M8 4l12 8-12 8Z',
    pause: 'M8 5v14m8-14v14',
    restart: 'M5 4v16M19 5l-11 7 11 7Z',
    skip: 'M19 4v16M5 5l11 7-11 7Z',
    reset: 'M3 10a9 9 0 1 1 1.7 7M3 4v6h6',
    volume: 'M11 4 6 8H3v8h3l5 4ZM15 8a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14',
    muted: 'M11 4 6 8H3v8h3l5 4ZM16 9l5 6m0-6-5 6',
    clock: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18m0 4v5l3 2',
    palette:
      'M12 3a9 9 0 0 0 0 18h1.5a2 2 0 0 0 1-3.7 1.5 1.5 0 0 1 1-2.8H18a4 4 0 0 0 3-4A9 9 0 0 0 12 3ZM7 10h.1M9 7h.1M14 7h.1M18 10h.1',
    bell: 'M5 16h14l-2-3V9a5 5 0 0 0-10 0v4Zm5 4h4',
    wallet: 'M20 8V5H5a2 2 0 0 0-2 2v12h17v-4M5 8h16v7h-6V9h6m-4 3h.1',
    coin: 'M12 2.5a9.5 9.5 0 1 0 0 19 9.5 9.5 0 0 0 0-19ZM8.5 7.5l3.5 4 3.5-4m-3.5 4v5.5M9 12h6m-6 2.5h6',
    keyboard:
      'M4 5h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2ZM6 9h.1M10 9h.1M14 9h.1M18 9h.1M6 12h.1M10 12h.1M14 12h.1M18 12h.1M7 16h10',
    settings:
      'M10 3 9.2 5.4 6.8 6 4.6 5.1 2.6 8.6 4.2 10.5v3l-1.6 1.9 2 3.5 2.2-.9 2.4.6.8 2.4h4l.8-2.4 2.4-.6 2.2.9 2-3.5-1.6-1.9v-3l1.6-1.9-2-3.5-2.2.9-2.4-.6L14 3Zm2 6a3 3 0 1 0 0 6 3 3 0 0 0 0-6',
    info: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18m0 8v6m0-10h.1',
    chart: 'M4 19h16M6 15V9m6 6V4m6 11V8',
    close: 'm6 6 12 12M18 6 6 18',
    window: 'M4 5h16v14H4zM4 9h16',
    exit: 'M11 5H5v14h6m-2-7h11m-4-4 4 4-4 4',
    plus: 'M12 5v14M5 12h14',
    'arrow-down-left': 'M18 6 6 18m0-9v9h9',
    'arrow-up-right': 'M6 18 18 6m-9 0h9v9',
    arrow: 'M5 12h14m-5-5 5 5-5 5',
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  aria-hidden="true"
  fill={name === 'play' ? 'currentColor' : 'none'}
  stroke="currentColor"
  stroke-width={name === 'pause' ? 3 : 1.6}
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d={paths[name]} />
</svg>

<style>
  svg {
    display: block;
    flex-shrink: 0;
  }
</style>
