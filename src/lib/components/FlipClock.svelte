<script lang="ts">
  import FlipDigit from './FlipDigit.svelte';

  let { display }: { display: string } = $props();
</script>

<div class="case" aria-hidden="true">
  <div class="face">
    {#each display.split(':') as group, groupIndex (groupIndex)}
      {#if groupIndex > 0}<span class="colon"></span>{/if}
      {#each [...group] as digit, index (group.length - index)}
        <FlipDigit {digit} />
      {/each}
    {/each}
  </div>
</div>

<style>
  .case {
    width: 100%;
    padding: 12px 10px 15px;
    border: 1px solid #181717;
    border-radius: 13px;
    background: linear-gradient(180deg, #373534, #272625 14%, #242323 87%, #1b1a1a);
    box-shadow:
      inset 0 1px 0 #5a5652,
      inset 0 -3px 0 #171616,
      0 4px 8px #07060538;
    container-type: inline-size;
  }

  .face {
    display: flex;
    align-items: center;
    gap: clamp(2px, 1.5cqi, 4px);
    height: clamp(68px, 37cqi, 88px);
    padding: 4px;
    border-radius: 6px;
    background: #151414;
    box-shadow:
      0 1px 0 #4a4642,
      inset 0 2px 5px #090909;
  }

  .colon {
    display: grid;
    flex: 0 0 clamp(7px, 4cqi, 11px);
    justify-items: center;
    gap: 9px;
  }

  .colon::before,
  .colon::after {
    content: '';
    width: 4px;
    height: 4px;
    border-radius: 1px;
    background: #e9ddc7;
    box-shadow: 0 1px 1px #080707;
  }
</style>
