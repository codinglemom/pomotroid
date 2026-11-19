<script lang="ts">
  import type { TimerStyle } from '$lib/types';
  import FlipClock from './FlipClock.svelte';
  import NixieClock from './NixieClock.svelte';
  import HourglassClock from './HourglassClock.svelte';

  let {
    styleName,
    display,
    remainingFraction,
    running = false,
  }: {
    styleName: Exclude<TimerStyle, 'ring'>;
    display: string;
    remainingFraction: number;
    running?: boolean;
  } = $props();
</script>

{#if styleName === 'flip'}
  <FlipClock {display} />
{:else if styleName === 'nixie'}
  <NixieClock {display} />
{:else}
  <HourglassClock {display} {remainingFraction} {running} />
{/if}
