<script lang="ts">
  import { onMount } from 'svelte';
  import { walletGetSummary, walletSetRate } from '$lib/ipc';
  import type { WalletSummary } from '$lib/types';
  import { decimal, money, parseMoney } from '$lib/utils/money';
  import { watchWallet } from '$lib/utils/wallet';
  import { openStats } from '$lib/utils/windows';
  import * as m from '$paraglide/messages.js';

  let summary = $state<WalletSummary | null>(null);
  let rate = $state('');
  let edited = false;
  let saving = $state(false);
  let error = $state('');
  let status = $state('');
  let disposed = false;
  let generation = 0;

  async function refresh() {
    const request = ++generation;
    try {
      const next = await walletGetSummary();
      if (disposed || request !== generation) return;
      summary = next;
      if (!edited)
        rate = next.rate_minor_per_hour === null ? '' : decimal(next.rate_minor_per_hour);
    } catch {
      if (!disposed && request === generation) error = m.wallet_load_failed();
    }
  }

  async function saveRate(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    error = '';
    status = '';
    const minor = parseMoney(rate);
    if (minor === null) {
      error = m.wallet_invalid_amount();
      return;
    }
    saving = true;
    try {
      await walletSetRate(minor);
      edited = false;
      status = m.wallet_rate_saved();
      await refresh();
    } catch (e) {
      error =
        String(e) === 'wallet_invalid_amount' ? m.wallet_invalid_amount() : m.wallet_save_failed();
    } finally {
      saving = false;
    }
  }

  onMount(() => {
    const cleanup = watchWallet(refresh);
    return () => {
      disposed = true;
      cleanup();
    };
  });
</script>

<section>
  <h2>{m.wallet_title()}</h2>
  <p>{m.wallet_rate_heading()}</p>
  {#if summary?.rate_minor_per_hour === null}
    <p class="hint">{m.wallet_first_use()}</p>
  {:else if summary}
    <p class="hint">
      {m.wallet_current_rate()}: {money(summary.rate_minor_per_hour)}
      {m.wallet_per_hour()}
    </p>
  {/if}
  <form onsubmit={saveRate}>
    <label for="reward-rate">{m.wallet_rate_label()}</label>
    <div class="rate-input">
      <input
        id="reward-rate"
        inputmode="decimal"
        autocomplete="off"
        placeholder="0.00"
        bind:value={rate}
        oninput={() => {
          edited = true;
          status = '';
        }}
        disabled={saving || !summary}
        required
      />
      <button type="submit" disabled={saving || !summary}
        >{saving ? m.wallet_saving() : m.wallet_save_rate()}</button
      >
    </div>
  </form>
  {#if error}<p class="feedback" role="alert">{error}</p>{/if}
  <p class="hint" role="status" aria-live="polite">{status}</p>
  <details>
    <summary>{m.wallet_rules()}</summary>
    <p class="hint">{m.wallet_rate_rule()}</p>
    <p class="hint">{m.wallet_rounding_rule()}</p>
    <p class="hint">{m.wallet_history_rule()}</p>
  </details>
  <button type="button" class="ledger-link" onclick={() => openStats('wallet')}
    >{m.wallet_open_ledger()}</button
  >
</section>

<style>
  section {
    padding: 20px;
    font-size: 0.85rem;
    color: var(--color-foreground);
  }
  h2 {
    font-size: 1rem;
    margin: 0 0 16px;
  }
  p {
    margin: 10px 0;
    line-height: 1.6;
    overflow-wrap: anywhere;
  }
  .hint {
    font-size: 0.75rem;
    color: var(--color-foreground-darker);
  }
  form {
    margin: 20px 0 12px;
  }
  label {
    display: block;
    margin-bottom: 8px;
  }
  .rate-input {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }
  input {
    width: 180px;
    min-width: 0;
    user-select: text;
  }
  input,
  button {
    padding: 8px 10px;
    border: 1px solid var(--color-separator);
    border-radius: 5px;
    background: var(--color-background-light);
    color: inherit;
    font: inherit;
  }
  button,
  summary {
    cursor: pointer;
  }
  button:hover:enabled {
    background: var(--color-hover);
  }
  button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  button:focus-visible,
  input:focus-visible,
  summary:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  details {
    margin-top: 20px;
    padding-top: 14px;
    border-top: 1px solid var(--color-separator);
  }
  .ledger-link {
    margin-top: 20px;
  }
  .feedback {
    font-size: 0.75rem;
  }
</style>
