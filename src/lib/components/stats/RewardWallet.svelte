<script lang="ts">
  import { onMount } from 'svelte';
  import { walletGetSummary, walletGetLedger, walletGetIncomeHistory, walletSpend } from '$lib/ipc';
  import type { WalletSummary, WalletLedgerEntry, WalletIncomeHistory } from '$lib/types';
  import { money, parseMoney } from '$lib/utils/money';
  import { watchWallet } from '$lib/utils/wallet';
  import { openSettings } from '$lib/utils/windows';
  import { getLocale } from '$paraglide/runtime.js';
  import Icon from '../Icon.svelte';
  import IncomeHistory from './IncomeHistory.svelte';
  import * as m from '$paraglide/messages.js';

  let summary = $state<WalletSummary | null>(null);
  let entries = $state<WalletLedgerEntry[]>([]);
  let history = $state<WalletIncomeHistory | null>(null);
  let days = $state<7 | 30>(7);
  let view = $state<'income' | 'ledger'>('income');
  let hasMore = $state(false);
  let amount = $state('');
  let note = $state('');
  let loading = $state(true);
  let loadingMore = $state(false);
  let spending = $state(false);
  let error = $state('');
  let incomeError = $state('');
  let spendError = $state('');
  let status = $state('');
  let dialog: HTMLDialogElement;
  let spendButton: HTMLButtonElement;
  let ledgerButton: HTMLButtonElement;
  let generation = 0;
  let disposed = false;
  let pendingSpend: { id: string; amount: number; note: string } | null = null;

  function expenseError(e: unknown): string {
    const code = String(e);
    if (code === 'wallet_insufficient_balance') return m.wallet_insufficient_balance();
    if (code === 'wallet_invalid_amount') return m.wallet_invalid_amount();
    if (code === 'wallet_invalid_note') return m.wallet_invalid_note();
    if (code === 'wallet_amount_limit') return m.wallet_amount_limit();
    return m.wallet_save_failed();
  }

  async function refresh() {
    const request = ++generation;
    loading = true;
    const results = await Promise.allSettled([
      walletGetSummary(),
      walletGetLedger(),
      walletGetIncomeHistory(days),
    ]);
    if (disposed || request !== generation) return;
    const [totals, ledger, income] = results;
    error = '';
    if (totals.status === 'fulfilled') summary = totals.value;
    else error = m.wallet_load_failed();
    if (ledger.status === 'fulfilled') {
      entries = ledger.value.entries;
      hasMore = ledger.value.has_more;
    } else error = m.wallet_load_failed();
    if (income.status === 'fulfilled') {
      history = income.value;
      incomeError = '';
    } else incomeError = m.wallet_income_failed();
    loading = false;
  }

  function changeDays(next: 7 | 30) {
    if (days === next) return;
    days = next;
    history = null;
    incomeError = '';
    void refresh();
  }

  async function loadMore() {
    if (loadingMore || loading || !hasMore) return;
    const request = generation;
    loadingMore = true;
    error = '';
    try {
      const page = await walletGetLedger(entries.at(-1)?.id ?? null);
      if (disposed || request !== generation) return;
      entries = [...entries, ...page.entries];
      hasMore = page.has_more;
    } catch {
      if (!disposed && request === generation) error = m.wallet_load_failed();
    } finally {
      if (!disposed) loadingMore = false;
    }
  }

  function openExpense() {
    spendError = '';
    status = '';
    dialog.showModal();
  }

  async function spend(event: SubmitEvent) {
    event.preventDefault();
    if (spending) return;
    spendError = '';
    status = '';
    const minor = parseMoney(amount);
    if (minor === null) {
      spendError = m.wallet_invalid_amount();
      return;
    }
    const trimmedNote = note.trim();
    if (!trimmedNote || [...trimmedNote].length > 500) {
      spendError = m.wallet_invalid_note();
      return;
    }
    spending = true;
    try {
      if (pendingSpend?.amount !== minor || pendingSpend?.note !== trimmedNote) {
        pendingSpend = { id: crypto.randomUUID(), amount: minor, note: trimmedNote };
      }
      await walletSpend(minor, trimmedNote, pendingSpend.id);
      pendingSpend = null;
      amount = '';
      note = '';
      status = m.wallet_expense_saved();
      await refresh();
      view = 'ledger';
      dialog.close();
    } catch (e) {
      spendError = expenseError(e);
    } finally {
      spending = false;
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

<div class="wallet" aria-busy={loading}>
  <div class="summary">
    <div class="balance">
      <dl>
        <dt>{m.wallet_balance()}</dt>
        <dd>{summary ? money(summary.balance_minor) : '—'}</dd>
      </dl>
      <span class="balance-coin" aria-hidden="true">
        <Icon name="coin" size={29} />
      </span>
    </div>
    <div class="summary-side">
      <dl class="totals">
        <div>
          <dt><Icon name="arrow-down-left" size={16} />{m.wallet_income_total()}</dt>
          <dd class:income={summary}>{summary ? money(summary.income_minor) : '—'}</dd>
        </div>
        <div>
          <dt><Icon name="arrow-up-right" size={16} />{m.wallet_expense_total()}</dt>
          <dd class:expense={summary}>{summary ? money(summary.expense_minor) : '—'}</dd>
        </div>
      </dl>
      <button
        bind:this={spendButton}
        class="ui-button primary spend"
        onclick={openExpense}
        disabled={!summary || summary.balance_minor === 0}
      >
        <Icon name="plus" size={16} />{m.wallet_record_expense()}
      </button>
    </div>
  </div>
  {#if error}<div class="feedback" role="alert">
      <span>{error}</span><button
        class="ui-button"
        onclick={() => void refresh()}
        disabled={loading}>{m.ui_retry()}</button
      >
    </div>{/if}
  <nav class="wallet-tabs" aria-label={m.wallet_title()}>
    <button
      class:active={view === 'income'}
      aria-current={view === 'income' ? 'page' : undefined}
      onclick={() => (view = 'income')}>{m.wallet_daily_income()}</button
    >
    <button
      bind:this={ledgerButton}
      class:active={view === 'ledger'}
      aria-current={view === 'ledger' ? 'page' : undefined}
      onclick={() => (view = 'ledger')}>{m.wallet_transactions()}</button
    >
  </nav>

  <div hidden={view !== 'income'}>
    <IncomeHistory
      {history}
      {days}
      {loading}
      error={incomeError}
      onDaysChange={changeDays}
      onRetry={() => void refresh()}
    />
  </div>
  <section class="ledger" hidden={view !== 'ledger'} aria-label={m.wallet_transactions()}>
    <div class="rate-summary">
      <span
        >{m.wallet_current_rate()}: {summary?.rate_minor_per_hour != null
          ? `${money(summary.rate_minor_per_hour)} ${m.wallet_per_hour()}`
          : '—'}</span
      ><button class="rate-link" onclick={() => openSettings('rewards')}
        >{summary?.rate_minor_per_hour != null ? m.wallet_adjust_rate() : m.wallet_setup()}<Icon
          name="arrow"
          size={14}
        /></button
      >
    </div>
    <div class="ledger-heading">
      <h2>{m.wallet_ledger_heading()}</h2>
      <button class="ui-button refresh" onclick={() => void refresh()} disabled={loading}
        ><Icon name="reset" size={14} />{loading ? m.wallet_loading() : m.wallet_refresh()}</button
      >
    </div>
    {#if entries.length > 0}
      <table>
        <caption class="sr-only">{m.wallet_ledger_heading()}</caption>
        <thead
          ><tr
            ><th scope="col">{m.wallet_date()}</th><th scope="col">{m.wallet_detail()}</th><th
              scope="col"
              class="amount">{m.wallet_amount_label()}</th
            ></tr
          ></thead
        >
        <tbody>
          {#each entries as entry (entry.id)}
            <tr>
              <td class="date"
                ><time datetime={new Date(entry.created_at * 1000).toISOString()}
                  >{new Date(entry.created_at * 1000).toLocaleDateString(getLocale())}<small
                    >{new Date(entry.created_at * 1000).toLocaleTimeString(getLocale(), {
                      hour: '2-digit',
                      minute: '2-digit',
                    })}</small
                  ></time
                ></td
              >
              <td>
                <div class="transaction">
                  <span
                    class="direction"
                    class:income={entry.kind === 'income'}
                    class:expense={entry.kind === 'expense'}
                  >
                    <Icon
                      name={entry.kind === 'income' ? 'arrow-down-left' : 'arrow-up-right'}
                      size={16}
                    />
                  </span>
                  <div>
                    {#if entry.kind === 'income'}
                      <span>{m.wallet_focus_income()}</span>
                      <small
                        >{new Intl.NumberFormat(getLocale(), { maximumFractionDigits: 1 }).format(
                          (entry.duration_secs ?? 0) / 60
                        )} min · {money(entry.rate_minor_per_hour ?? 0)}
                        {m.wallet_per_hour()}</small
                      >
                      {#if entry.started_at !== null}
                        <small
                          >{m.wallet_started_at()}: {new Date(
                            entry.started_at * 1000
                          ).toLocaleString(getLocale())}</small
                        >
                      {/if}
                    {:else}
                      <span>{m.wallet_expense()}</span>
                      <small>{entry.note}</small>
                    {/if}
                  </div>
                </div>
              </td>
              <td
                class="amount"
                class:income={entry.kind === 'income'}
                class:expense={entry.kind === 'expense'}
                >{entry.kind === 'income' ? '+' : '−'}{money(entry.amount_minor)}</td
              >
            </tr>
          {/each}
        </tbody>
      </table>
      {#if hasMore}<button
          class="ui-button more"
          onclick={loadMore}
          disabled={loadingMore || loading}
          >{loadingMore ? m.wallet_loading() : m.wallet_load_more()}</button
        >{/if}
    {:else if !loading && summary}<p class="empty">{m.wallet_empty()}</p>{/if}
  </section>
  <p class="status" role="status" aria-live="polite">{status}</p>
</div>

<dialog
  bind:this={dialog}
  aria-labelledby="spend-title"
  onclose={() => (spendButton?.disabled ? ledgerButton : spendButton)?.focus()}
  oncancel={(event) => {
    if (spending) event.preventDefault();
  }}
>
  <form onsubmit={spend} aria-busy={spending}>
    <div class="dialog-heading">
      <h2 id="spend-title">{m.wallet_spend_heading()}</h2>
      <button
        type="button"
        class="ui-button ui-icon-button"
        aria-label={m.ui_close()}
        disabled={spending}
        onclick={() => dialog.close()}><Icon name="close" /></button
      >
    </div>
    <label for="wallet-amount">{m.wallet_amount_label()}</label>
    <input
      id="wallet-amount"
      inputmode="decimal"
      autocomplete="off"
      placeholder="0.00"
      bind:value={amount}
      disabled={spending}
      required
    />
    <label for="wallet-note">{m.wallet_note_label()}</label>
    <input id="wallet-note" maxlength="500" bind:value={note} disabled={spending} required />
    {#if spendError}<p class="spend-error" role="alert">{spendError}</p>{/if}
    <div class="dialog-actions">
      <button type="button" class="ui-button" onclick={() => dialog.close()} disabled={spending}
        >{m.ui_cancel()}</button
      ><button
        class="ui-button primary"
        type="submit"
        disabled={spending || !summary || summary.balance_minor === 0}
        >{spending ? m.wallet_saving() : m.wallet_record_expense()}</button
      >
    </div>
  </form>
</dialog>

<style>
  .wallet {
    min-height: 0;
    overflow-y: auto;
    padding: 12px 24px;
    font-size: 14px;
    flex: 1;
  }
  .summary {
    display: grid;
    grid-template-columns: minmax(0, 5fr) minmax(0, 7fr);
    gap: 12px;
  }
  .summary dt {
    font-size: 12px;
  }
  .summary dd {
    font-variant-numeric: tabular-nums;
    font-weight: 550;
    overflow-wrap: anywhere;
  }
  .balance {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    min-height: 92px;
    padding: 12px 16px;
    border: 1px solid #496457;
    border-radius: var(--radius-panel);
    color: #f4f5ec;
    background:
      repeating-radial-gradient(
        circle at 98% 132%,
        transparent 0 11px,
        rgb(220 211 166 / 10%) 12px,
        transparent 13px 17px
      ),
      linear-gradient(116deg, #152d25 25%, #234337 100%);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 8%),
      0 2px 6px rgb(15 28 20 / 14%);
  }
  .balance dt {
    color: #c1d3c8;
  }
  .balance dd {
    margin-top: 2px;
    font-size: 32px;
    line-height: 40px;
  }
  .balance-coin {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    border: 1px solid rgb(217 185 119 / 38%);
    border-radius: 50%;
    background: rgb(22 45 36 / 72%);
    color: #e4c78b;
    box-shadow:
      inset 0 1px 2px rgb(237 211 159 / 17%),
      0 2px 4px rgb(4 18 11 / 30%);
  }
  .summary-side {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 16px;
    padding: 8px 12px;
    border: 1px solid var(--color-separator);
    border-radius: var(--radius-panel);
    background: var(--surface-panel);
  }
  .totals > div {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    padding: 6px 0;
  }
  .totals > div + div {
    border-top: 1px solid var(--color-separator);
  }
  .totals dt {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--color-foreground-darker);
    overflow-wrap: anywhere;
  }
  .totals dd {
    font-size: 15px;
    text-align: right;
  }
  .spend {
    max-width: 180px;
    overflow-wrap: anywhere;
  }
  .income {
    color: var(--color-income);
  }
  .expense {
    color: var(--color-expense);
  }
  .wallet-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 8px 0;
    border-bottom: 1px solid var(--color-separator);
  }
  .wallet-tabs button {
    border: 1px solid transparent;
    border-bottom-width: 2px;
    border-radius: 6px 6px 0 0;
    padding: 6px 12px;
    min-height: 32px;
    background: transparent;
    color: var(--color-foreground-darker);
    font-size: 12px;
    cursor: pointer;
  }
  .wallet-tabs button:hover {
    color: var(--color-foreground);
    background: var(--color-hover);
  }
  .wallet-tabs button.active {
    color: var(--color-foreground);
    background: var(--surface-panel);
    border-bottom-color: var(--color-accent);
    font-weight: 600;
  }
  .rate-summary {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    color: var(--color-foreground-darker);
    font-size: 12px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--color-separator);
  }
  .rate-link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--color-accent);
    padding: 6px;
    cursor: pointer;
    font-size: 12px;
  }
  .ledger-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin: 12px 0 6px;
  }
  h2 {
    font-size: 14px;
    font-weight: 600;
    margin: 0;
  }
  .refresh {
    font-size: 12px;
    min-height: 30px;
  }
  table {
    width: 100%;
    border: 1px solid var(--color-separator);
    border-radius: var(--radius-panel);
    border-spacing: 0;
    table-layout: fixed;
    font-size: 12px;
    background: var(--surface-panel);
  }
  th {
    text-align: left;
    color: var(--color-foreground-darker);
    font-weight: 500;
    padding: 8px 12px;
    overflow-wrap: anywhere;
  }
  td {
    border-top: 1px solid var(--color-separator);
    padding: 10px 12px;
    vertical-align: middle;
    overflow-wrap: anywhere;
  }
  th:first-child {
    width: 22%;
  }
  th:last-child {
    width: 26%;
  }
  .date {
    color: var(--color-foreground-darker);
    font-variant-numeric: tabular-nums;
  }
  .transaction {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: start;
    gap: 8px;
  }
  .direction {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    background: color-mix(in srgb, currentColor 11%, transparent);
  }
  .transaction > div > span {
    font-weight: 550;
  }
  small {
    display: block;
    font-size: 11px;
    color: var(--color-foreground-darker);
    line-height: 1.5;
    margin-top: 2px;
  }
  .amount {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  td.amount {
    font-size: 14px;
    font-weight: 550;
  }
  .more {
    display: flex;
    margin: 12px auto 0;
    font-size: 12px;
  }
  .status {
    font-size: 12px;
    color: var(--color-income);
    margin-top: 8px;
  }
  .status:empty {
    display: none;
  }
  .feedback {
    display: flex;
    align-items: center;
    gap: 12px;
    color: var(--color-error);
    font-size: 12px;
    margin: 8px 0;
  }
  .empty {
    color: var(--color-foreground-darker);
    font-size: 12px;
    padding: 24px 12px;
    line-height: 1.6;
    text-align: center;
    border: 1px solid var(--color-separator);
    border-radius: var(--radius-panel);
    background: var(--surface-panel);
  }
  dialog {
    width: 380px;
    max-width: calc(100vw - 40px);
    padding: 24px;
    border: 1px solid var(--color-separator);
    border-radius: 12px;
    color: var(--color-foreground);
    background: var(--color-background-light);
    box-shadow: 0 16px 48px rgb(0 0 0 / 24%);
    margin: auto;
  }
  dialog::backdrop {
    background: rgb(16 24 20 / 38%);
  }
  .dialog-heading {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }
  .dialog-heading h2 {
    font-size: 18px;
  }
  label {
    display: block;
    margin: 16px 0 8px;
    color: var(--color-foreground-darker);
    font-size: 12px;
  }
  input {
    width: 100%;
    min-height: 36px;
    padding: 8px 12px;
    color: var(--color-foreground);
    background: var(--color-background);
    border: 1px solid var(--control-border);
    border-radius: 6px;
    user-select: text;
  }
  .spend-error {
    margin-top: 12px;
    font-size: 12px;
    color: var(--color-error);
    line-height: 1.5;
  }
  .dialog-actions {
    margin-top: 20px;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  @media (max-width: 680px) {
    .summary {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  @media (max-width: 400px) {
    .summary-side {
      grid-template-columns: minmax(0, 1fr);
      gap: 8px;
    }
    .spend {
      justify-self: start;
    }
    th,
    td {
      padding-inline: 8px;
    }
    .transaction {
      gap: 4px;
    }
    .direction {
      width: 20px;
      height: 20px;
    }
  }
</style>
