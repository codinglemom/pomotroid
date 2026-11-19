<script lang="ts">
  import type { WalletIncomeHistory } from '$lib/types';
  import { money } from '$lib/utils/money';
  import { getLocale } from '$paraglide/runtime.js';
  import * as m from '$paraglide/messages.js';

  let {
    history,
    days,
    loading,
    error,
    onDaysChange,
    onRetry,
  }: {
    history: WalletIncomeHistory | null;
    days: 7 | 30;
    loading: boolean;
    error: string;
    onDaysChange: (days: 7 | 30) => void;
    onRetry: () => void;
  } = $props();
  let selectedDate = $state<string | null>(null);
  let previousAsOf: string | null = null;

  $effect(() => {
    if (!history) return;
    if (
      !selectedDate ||
      selectedDate === previousAsOf ||
      !history.days.some((day) => day.date === selectedDate)
    ) {
      selectedDate = history.as_of_local_date;
    }
    previousAsOf = history.as_of_local_date;
  });

  const selectedIndex = $derived(history?.days.findIndex((day) => day.date === selectedDate) ?? -1);
  const selected = $derived(
    selectedIndex >= 0 ? history?.days[selectedIndex] : history?.days.at(-1)
  );
  const previous = $derived(
    !history
      ? 0
      : selectedIndex > 0
        ? history.days[selectedIndex - 1].income_minor
        : selectedIndex === 0
          ? history.previous_income_minor
          : (history.days.at(-2)?.income_minor ?? history.previous_income_minor)
  );
  const difference = $derived((selected?.income_minor ?? 0) - previous);
  const total = $derived(history?.days.reduce((sum, day) => sum + day.income_minor, 0) ?? 0);
  const maximum = $derived(Math.max(100, ...(history?.days.map((day) => day.income_minor) ?? [])));
  const today = $derived(selected?.date === history?.as_of_local_date);

  function dateLabel(date: string, full = false): string {
    const [year, month, day] = date.split('-').map(Number);
    return new Date(year, month - 1, day, 12).toLocaleDateString(getLocale(), {
      year: full ? 'numeric' : undefined,
      month: 'numeric',
      day: 'numeric',
    });
  }
  function chartMoney(minor: number): string {
    return minor < 100000
      ? money(minor)
      : `¥${new Intl.NumberFormat(getLocale(), { notation: 'compact', maximumFractionDigits: 1 }).format(minor / 100)}`;
  }
  function percent(): string {
    return new Intl.NumberFormat(getLocale(), {
      style: 'percent',
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    }).format(Math.abs(difference) / previous);
  }
</script>

<section class="income-history" aria-label={m.wallet_daily_income()} aria-busy={loading}>
  <div class="range" role="group" aria-label={m.wallet_range_label()}>
    {#each [7, 30] as count}
      <button
        class:active={days === count}
        aria-pressed={days === count}
        onclick={() => onDaysChange(count as 7 | 30)}>{m.wallet_last_days({ days: count })}</button
      >
    {/each}
  </div>
  {#if error}
    <div class="state-message" role="alert">
      <p>{error}</p>
      <button class="ui-button" onclick={onRetry}>{m.ui_retry()}</button>
    </div>
  {:else if !history || !selected}
    <div class="state-message" role="status">{m.wallet_loading()}</div>
  {:else}
    <div class="overview" aria-live="polite" aria-atomic="true">
      <div class="selected-income">
        <span
          >{today ? m.wallet_today() : m.wallet_income_on({ date: dateLabel(selected.date) })}</span
        ><strong>{money(selected.income_minor)}</strong>
      </div>
      <div class="change">
        <strong class:income={difference > 0} class:expense={difference < 0}>
          {previous === 0
            ? selected.income_minor === 0
              ? m.wallet_unchanged()
              : m.wallet_new_income()
            : difference === 0
              ? m.wallet_unchanged()
              : `${difference > 0 ? '↑' : '↓'} ${percent()}`}
        </strong>
        <span
          >{difference > 0
            ? m.wallet_income_more({ amount: money(difference) })
            : difference < 0
              ? m.wallet_income_less({ amount: money(-difference) })
              : m.wallet_unchanged()} · {m.wallet_previous_income({
            amount: money(previous),
          })}</span
        >
      </div>
      <div class="period">
        <span>{m.wallet_period_total({ days: history.days.length })}</span><strong
          >{money(total)}</strong
        >
      </div>
    </div>
    <div class="chart" aria-label={m.wallet_chart_label()}>
      <div class="grid" aria-hidden="true">
        <span>{chartMoney(maximum)}</span><span>{chartMoney(Math.round(maximum / 2))}</span><span
          >¥0</span
        >
      </div>
      <div class="bars" class:dense={history.days.length > 7}>
        {#each history.days as day, index (day.date)}
          {@const height = (day.income_minor / maximum) * 100}
          {@const isToday = day.date === history.as_of_local_date}
          {@const isSelected = day.date === selected.date}
          {@const showDate =
            history.days.length <= 7 ||
            (index % 7 === 0 && index < history.days.length - 3) ||
            index === history.days.length - 1}
          <button
            aria-label={m.wallet_income_day_label({
              date: dateLabel(day.date, true),
              amount: money(day.income_minor),
            })}
            aria-pressed={isSelected}
            title={m.wallet_income_day_label({
              date: dateLabel(day.date, true),
              amount: money(day.income_minor),
            })}
            onclick={() => (selectedDate = day.date)}
          >
            <span class="plot" style:--bar-height="{height}%">
              <span class="bar"></span>
              {#if history.days.length <= 7 || isSelected}
                <span class="bar-value">{chartMoney(day.income_minor)}</span>
              {/if}
            </span>
            <span class="date"
              >{showDate ? (isToday ? m.stats_tab_today() : dateLabel(day.date)) : ''}</span
            >
          </button>
        {/each}
      </div>
    </div>
    <div class="chart-footer">
      <span>{today ? m.wallet_today_comparison() : m.wallet_day_comparison()}</span><span
        >{loading
          ? m.wallet_loading()
          : total === 0
            ? m.wallet_income_empty()
            : m.wallet_select_day()}</span
      >
    </div>
  {/if}
</section>

<style>
  .income-history {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 16px;
    padding: 12px;
    border: 1px solid var(--color-separator);
    border-radius: var(--radius-panel);
    background: var(--surface-panel);
  }
  .range {
    grid-column: 2;
    grid-row: 1;
    justify-self: end;
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 4px;
  }
  .range button {
    border: 1px solid var(--color-separator);
    border-radius: 6px;
    padding: 6px 8px;
    min-height: 32px;
    background: transparent;
    color: var(--color-foreground-darker);
    font-size: 12px;
    cursor: pointer;
  }
  .range button.active {
    color: var(--color-foreground);
    background: var(--color-hover);
    font-weight: 550;
  }
  .range button:hover {
    color: var(--color-foreground);
    background: var(--color-hover);
  }
  .overview {
    grid-column: 1;
    grid-row: 1;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.9fr) minmax(0, 1fr);
    align-items: center;
    gap: 8px 12px;
  }
  .overview > div {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .overview span {
    font-size: 12px;
    color: var(--color-foreground-darker);
    overflow-wrap: anywhere;
  }
  .overview strong {
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }
  .selected-income strong {
    font-size: 24px;
    line-height: 28px;
    color: var(--color-income);
  }
  .change {
    border-inline-start: 1px solid var(--color-separator);
    padding-inline-start: 12px;
  }
  .change strong {
    font-size: 14px;
  }
  .income {
    color: var(--color-income);
  }
  .expense {
    color: var(--color-expense);
  }
  .period {
    text-align: right;
  }
  .period strong {
    color: var(--color-income);
    font-size: 18px;
    line-height: 28px;
  }
  .chart {
    --axis-width: 60px;
    grid-column: 1 / -1;
    height: 148px;
    margin-top: 8px;
    padding-inline-start: var(--axis-width);
    position: relative;
  }
  .grid {
    position: absolute;
    inset: 16px 0 20px;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    color: var(--color-foreground-darker);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .grid span {
    height: 1px;
    position: relative;
    line-height: 1px;
  }
  .grid span::after {
    content: '';
    position: absolute;
    inset-block-start: 0;
    inset-inline: var(--axis-width) 0;
    height: 1px;
    background: var(--color-separator);
  }
  .bars {
    display: flex;
    gap: 12px;
    height: 100%;
    position: relative;
  }
  .bars button {
    flex: 1;
    min-width: 0;
    position: relative;
    height: 100%;
    padding: 0;
    background: transparent;
    border: 0;
    border-radius: 4px;
    color: var(--color-foreground-darker);
    font-size: 11px;
    cursor: pointer;
    font-variant-numeric: tabular-nums;
  }
  .bars button:hover {
    background: color-mix(in srgb, var(--color-income) 6%, transparent);
  }
  .plot {
    position: absolute;
    inset: 16px 0 20px;
  }
  .bar {
    position: absolute;
    bottom: 0;
    left: 22%;
    width: 56%;
    height: var(--bar-height);
    border-radius: 4px 4px 0 0;
    background: color-mix(in srgb, var(--color-income) 52%, transparent);
  }
  .bars button[aria-pressed='true'] .bar {
    background: var(--color-income);
  }
  .bars button[aria-pressed='true'] .date {
    color: var(--color-foreground);
    font-weight: 600;
  }
  .bar-value {
    position: absolute;
    left: 0;
    right: 0;
    bottom: calc(var(--bar-height) + 3px);
    text-align: center;
    white-space: nowrap;
    line-height: 16px;
    color: var(--color-foreground);
  }
  .date {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    text-align: center;
    line-height: 18px;
    white-space: nowrap;
  }
  .dense {
    gap: 5px;
  }
  .dense .bar {
    left: 12%;
    width: 76%;
    border-radius: 2px 2px 0 0;
  }
  .dense .date,
  .dense .bar-value {
    left: 50%;
    right: auto;
    transform: translateX(-50%);
  }
  .chart-footer {
    grid-column: 1 / -1;
    display: flex;
    justify-content: space-between;
    gap: 12px;
    margin-top: 6px;
    font-size: 11px;
    color: var(--color-foreground-darker);
  }
  .chart-footer > span {
    min-width: 0;
  }
  .state-message {
    grid-column: 1 / -1;
    min-height: 168px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--color-foreground-darker);
  }
  @media (max-width: 680px) {
    .income-history {
      grid-template-columns: minmax(0, 1fr);
      row-gap: 8px;
    }
    .range {
      grid-column: 1;
    }
    .overview {
      grid-row: 2;
    }
    .bars {
      gap: 8px;
    }
    .dense {
      gap: 3px;
    }
    .chart-footer {
      flex-wrap: wrap;
    }
  }
  @media (max-width: 420px) {
    .overview {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .change {
      grid-column: 1 / -1;
      grid-row: 2;
      border-inline-start: 0;
      padding-inline-start: 0;
    }
    .bars:not(.dense) .bar-value {
      white-space: normal;
      overflow-wrap: anywhere;
      line-height: 12px;
    }
  }
</style>
