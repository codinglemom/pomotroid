<script lang="ts">
  let {
    display,
    remainingFraction,
    running = false,
  }: { display: string; remainingFraction: number; running?: boolean } = $props();
  const id = $props.id();
  // All geometry is in SVG units. The open space above y=55 belongs to the time.
  const outline =
    'M31 7 H149 Q157 7 157 15 C157 49 148 58 120 76 L99 89 Q90 96 99 103 L120 117 C148 135 157 144 157 171 Q157 178 149 178 H31 Q23 178 23 171 C23 144 32 135 60 117 L81 103 Q90 96 81 89 L60 76 C32 58 23 49 23 15 Q23 7 31 7 Z';
  const f = $derived(Math.max(0, Math.min(1, remainingFraction)));
  const surface = $derived(96 - 40 * Math.sqrt(f));
  const deposited = $derived(Math.sqrt(1 - f));
  const peak = $derived(178 - 47 * deposited);
  const spread = $derived(81 * deposited);

  // Independent periods and lateral bends keep the falling grains out of lockstep.
  const grains = [
    { r: 1.45, bend: -6, land: -9, duration: 1.21, phase: -0.76 },
    { r: 1.05, bend: 4, land: 6, duration: 1.47, phase: -0.25 },
    { r: 1.7, bend: 1, land: -3, duration: 1.36, phase: -1.06 },
    { r: 1.22, bend: -3, land: 4, duration: 1.63, phase: -0.68 },
    { r: 1.5, bend: 7, land: 11, duration: 1.53, phase: -1.23 },
    { r: 0.98, bend: -4, land: -6, duration: 1.74, phase: -0.37 },
  ];
</script>

<svg class="clock" viewBox="0 0 180 186" aria-hidden="true">
  <defs><clipPath id="{id}-inside"><path d={outline} /></clipPath></defs>
  <path class="body" d={outline} />
  <g clip-path="url(#{id}-inside)">
    {#if f > 0}
      <path
        class="sand"
        d="M16 {surface} Q44 {surface - 2} 71 {surface + 1} T113 {surface + 1} Q143 {surface -
          1} 164 {surface} V96 H16Z"
      />
    {/if}
    {#if (running || f < 1) && f > 0}
      {#each grains as g}
        {@const land = g.land * deposited}
        {@const distance = peak - 96 + Math.abs(land) * 0.52}
        <circle
          class="grain"
          class:paused={!running}
          cx="90"
          cy="96"
          r={g.r}
          style="--bend:{g.bend}px;--turn:{g.bend * -0.25 +
            land * 0.65}px;--land:{land}px;--first:{distance * 0.24}px;--last:{distance *
            0.63}px;--drop:{distance}px;--period:{g.duration}s;--phase:{g.phase}s"
        />
      {/each}
    {/if}
    {#if f < 1}
      <path
        class="sand"
        d="M{90 - spread} 178 C{90 - spread * 0.56} {178 - 9 * deposited} {90 -
          spread * 0.14} {peak} 90 {peak} C{90 + spread * 0.17} {peak} {90 + spread * 0.52} {178 -
          9 * deposited} {90 + spread} 178 V182 H{90 - spread}Z"
      />
    {/if}
  </g>
  <path class="rim" d={outline} />
  <text x="90" y="44" text-anchor="middle" class="time" font-size={display.length > 5 ? 27 : 32}
    >{display}</text
  >
</svg>

<style>
  .clock {
    display: block;
    margin-inline: auto;
    width: min(100%, 170px);
    height: auto;
    color: var(--round-color, var(--color-focus-round));
  }
  .body {
    fill: color-mix(in srgb, currentColor 5%, transparent);
  }
  .rim {
    fill: none;
    stroke: color-mix(in srgb, currentColor 55%, var(--color-foreground-darker));
    stroke-width: 1.7;
  }
  .sand {
    fill: color-mix(in srgb, currentColor 68%, var(--color-background));
  }
  .time {
    fill: var(--color-foreground);
    font-family: 'Mona Sans Mono', monospace;
    font-weight: 475;
    font-variant-numeric: tabular-nums;
    letter-spacing: -1.4px;
  }
  .grain {
    fill: currentColor;
    animation: drift var(--period) ease-in var(--phase) infinite;
  }
  .grain.paused {
    animation-play-state: paused;
  }
  @keyframes drift {
    0%,
    14% {
      opacity: 0;
      transform: translate(0, -2px);
    }
    21% {
      opacity: 0.94;
      transform: translate(0, 1px);
    }
    47% {
      transform: translate(var(--bend), var(--first));
    }
    71% {
      transform: translate(var(--turn), var(--last));
      opacity: 0.86;
    }
    88% {
      transform: translate(var(--land), var(--drop));
      opacity: 0.8;
    }
    90%,
    100% {
      transform: translate(var(--land), var(--drop));
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .grain {
      animation: none !important;
      display: none;
    }
  }
</style>
