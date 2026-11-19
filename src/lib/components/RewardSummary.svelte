<script lang="ts">
  import { onMount } from 'svelte';
  import { walletGetSummary } from '$lib/ipc';
  import type { WalletSummary } from '$lib/types';
  import { money } from '$lib/utils/money';
  import { watchWallet } from '$lib/utils/wallet';
  import { openSettings, openStats } from '$lib/utils/windows';
  import * as m from '$paraglide/messages.js';

  let summary = $state<WalletSummary | null>(null);
  let failed = $state(false);

  onMount(() => {
    let disposed = false;
    let generation = 0;
    const cleanup = watchWallet(async () => {
      const request = ++generation;
      try {
        const next = await walletGetSummary();
        if (disposed || request !== generation) return;
        summary = next;
        failed = false;
      } catch {
        if (!disposed && request === generation) failed = true;
      }
    });
    return () => {
      disposed = true;
      cleanup();
    };
  });
</script>

<div class="reward-summary daily">
  <span class="label" title={m.wallet_today_hint()}>{m.wallet_today()}</span>
  {#if failed}
    <button class="muted" title={m.wallet_load_failed()} onclick={() => openStats('wallet')}>
      {m.wallet_title()} · —
    </button>
  {:else if summary?.rate_minor_per_hour === null && summary.income_minor === 0}
    <button onclick={() => openSettings('rewards')}>{m.wallet_setup()}</button>
  {:else if summary}
    <button class="amount" title={m.wallet_today_hint()} onclick={() => openStats('wallet')}>
      {money(summary.today_income_minor)}
    </button>
    {#if summary.rate_minor_per_hour === null}
      <button class="setup" onclick={() => openSettings('rewards')}>{m.wallet_setup()}</button>
    {/if}
  {:else}
    <span class="muted">—</span>
  {/if}
</div>

<style>
  .reward-summary {
    max-width: 320px;
    min-width: 0;
    text-align: center;
    font-size: 0.7rem;
    line-height: 1.4;
    color: var(--color-foreground-darker);
  }
  button {
    border: 0;
    padding: 0;
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
    max-width: 100%;
    overflow-wrap: anywhere;
  }
  button:hover {
    color: var(--color-foreground);
  }
  button:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 3px;
  }
  .muted {
    color: var(--color-foreground-darker);
  }
  .setup {
    display: block;
    margin: 3px auto 0;
    font-size: 0.7rem;
  }
  .daily {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .label {
    font-size: 0.68rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }
  .daily .amount {
    color: var(--color-focus-round);
    font-size: clamp(1rem, 2.7vw, 1.5rem);
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    line-height: 1.25;
  }

  .daily {
    text-align: left;
    align-items: flex-start;
    gap: 8px;
  }
  .label {
    font-size: 12px;
    font-weight: 400;
    letter-spacing: 0;
    text-transform: none;
  }
  .daily .amount {
    color: var(--color-income);
    font-size: 28px;
    line-height: 36px;
    font-weight: 550;
  }
</style>
