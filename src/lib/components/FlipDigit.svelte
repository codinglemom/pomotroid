<script lang="ts">
  let { digit }: { digit: string } = $props();

  let previous: string | undefined;
  let flip = $state<{ from: string; to: string } | null>(null);

  $effect.pre(() => {
    if (previous !== undefined && previous !== digit) {
      flip = { from: previous, to: digit };
    }
    previous = digit;
  });
</script>

<div class="digit">
  <span class="half top"><span>{digit}</span></span>
  <span class="half bottom"><span>{digit}</span></span>
  {#if flip}
    {#key flip}
      <span class="half bottom previous-bottom"><span>{flip.from}</span></span>
      <span class="half top folding-top"><span>{flip.from}</span></span>
      <span class="half bottom folding-bottom" onanimationend={() => (flip = null)}>
        <span>{flip.to}</span>
      </span>
    {/key}
  {/if}
</div>

<style>
  .digit {
    --fold-time: 115ms;
    --unfold-time: 155ms;
    position: relative;
    flex: 1 1 0;
    min-width: 0;
    height: 100%;
    overflow: clip;
    border-radius: 4px;
    background: #100f0f;
    color: #f2e9d8;
    font-family: 'Mona Sans Mono', monospace;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    text-shadow: 0 1px #181715;
    container-type: inline-size;
    perspective: 160px;
  }

  .half {
    position: absolute;
    inset-inline: 0;
    height: calc(50% - 0.5px);
    overflow: hidden;
    backface-visibility: hidden;
  }

  .half > span {
    position: absolute;
    display: grid;
    place-items: center;
    width: 100%;
    height: calc(200% + 1px);
    font-size: min(65px, 135cqi);
    line-height: 1;
  }

  .top {
    top: 0;
    border-radius: 4px 4px 1px 1px;
    background: linear-gradient(180deg, #3c3937, #343130);
    box-shadow: inset 0 1px 0 #4c4741;
    transform-origin: 50% calc(100% + 0.5px);
  }

  .top > span {
    top: 0;
  }

  .bottom {
    bottom: 0;
    border-radius: 1px 1px 4px 4px;
    background: linear-gradient(180deg, #252424, #2d2a28 18%, #292725);
    box-shadow: inset 0 -1px 0 #494440;
    transform-origin: 50% -0.5px;
  }

  .bottom > span {
    bottom: 0;
  }

  .digit::before,
  .digit::after {
    content: '';
    position: absolute;
    top: calc(50% - 2px);
    z-index: 2;
    width: 3px;
    height: 4px;
    border-radius: 1px;
    background: #100f0f;
    box-shadow: 0 1px 0 #55504a;
  }

  .digit::before {
    left: 0;
  }

  .digit::after {
    right: 0;
  }

  .folding-top {
    z-index: 1;
    animation: fold-top var(--fold-time) ease-in both;
  }

  .folding-bottom {
    z-index: 1;
    animation: unfold-bottom var(--unfold-time) var(--fold-time) ease-out both;
  }

  @keyframes fold-top {
    from {
      transform: rotateX(0deg);
    }
    to {
      transform: rotateX(-90deg);
      filter: brightness(0.65);
    }
  }

  @keyframes unfold-bottom {
    from {
      transform: rotateX(90deg);
      filter: brightness(0.72);
    }
    to {
      transform: rotateX(0deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .previous-bottom,
    .folding-top,
    .folding-bottom {
      visibility: hidden;
      animation-duration: 0ms;
      animation-delay: 0ms;
    }
  }
</style>
