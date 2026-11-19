<script lang="ts">
  let { display }: { display: string } = $props();
</script>

<div class="clock" aria-hidden="true">
  <div class="tubes">
    {#each display as character}
      {#if character === ':'}
        <span class="colon"></span>
      {:else}
        <div class="tube">
          <div class="glass"><span class="digit">{character}</span></div>
          <div class="pins"></div>
        </div>
      {/if}
    {/each}
  </div>
  <div class="base"></div>
</div>

<style>
  .clock {
    width: 100%;
    container-type: inline-size;
  }

  .tubes {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: flex-end;
    gap: clamp(2px, 1.4cqi, 4px);
    padding-inline: 11px;
    margin-bottom: -3px;
  }

  .tube {
    position: relative;
    flex: 1 1 0;
    min-width: 0;
    container-type: inline-size;
  }

  .glass {
    position: relative;
    display: grid;
    place-items: center;
    isolation: isolate;
    height: clamp(70px, 180cqi, 93px);
    overflow: hidden;
    padding-top: 5px;
    border: 1px solid #5b514b;
    border-radius: 49% 49% 23% 23% / 24% 24% 11% 11%;
    background:
      radial-gradient(ellipse at 50% 70%, #a1431b42, transparent 73%),
      linear-gradient(90deg, #272420, #1b1614 26%, #201817 71%, #37302b);
    box-shadow:
      inset 0 1px 0 #9b8d7e,
      inset 0 -5px 9px #1a1411,
      inset 2px 0 3px #b39a8140,
      0 2px 3px #130d0a66;
  }

  .glass::before {
    content: '';
    position: absolute;
    inset: 12% 13% 10%;
    z-index: -1;
    border: 1px solid #9c7e6552;
    border-radius: 46% 46% 20% 20% / 19% 19% 8% 8%;
    background:
      repeating-linear-gradient(45deg, #c4a38930 0 0.6px, transparent 0.6px 3px),
      repeating-linear-gradient(-45deg, #c4a38930 0 0.6px, transparent 0.6px 3px);
    box-shadow: inset 0 0 7px #08060585;
  }

  .glass::after {
    content: '';
    position: absolute;
    inset: 8% 8% 14%;
    border-radius: inherit;
    background:
      linear-gradient(90deg, #f9e6cb24 1%, transparent 12% 85%, #ffe8cc17 95%, transparent),
      linear-gradient(165deg, #fff2df28, transparent 27%);
  }

  .digit {
    color: #ffb16c;
    font-family: 'Mona Sans Mono', monospace;
    font-size: min(58px, 116cqi);
    font-weight: 255;
    font-variant-numeric: tabular-nums;
    line-height: 1;
    text-shadow:
      0 0 2px #ff9739,
      0 0 7px #ff722984,
      0 0 15px #fb4a2340;
  }

  .pins {
    position: relative;
    width: 54%;
    height: 9px;
    margin: -1px auto 0;
    border-bottom: 3px solid #2f2a25;
    border-radius: 0 0 3px 3px;
    background: repeating-linear-gradient(
      90deg,
      transparent 0 3px,
      #645442 3px 4px,
      #bc9b72 4px 5px,
      #4b3c2c 5px 6px
    );
  }

  .colon {
    display: grid;
    flex: 0 0 clamp(7px, 4cqi, 11px);
    align-self: center;
    justify-items: center;
    gap: 10px;
  }

  .colon::before,
  .colon::after {
    content: '';
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: #ffb16c;
    box-shadow: 0 0 5px #ff752da6;
  }

  .base {
    height: 23px;
    border: 1px solid #211b18;
    border-radius: 7px;
    background: linear-gradient(180deg, #60534a 0, #4c4038 5px, #302823 6px, #211a17);
    box-shadow:
      inset 0 1px 0 #7a6b5f,
      inset 0 -2px 0 #1b1513,
      0 4px 8px #110a073d;
  }
</style>
