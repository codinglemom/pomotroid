<script lang="ts">
  import type { DailyStats } from '$lib/types';
  import RewardSummary from '../RewardSummary.svelte';
  import * as m from '$paraglide/messages.js';

  let { today }: { today: DailyStats | null } = $props();

  const CHART_H = 80; // px, max bar height in the hourly chart
  const CHART_W = 744; // px, total SVG width for 24 bars
  const BAR_W = 22; // px per bar
  const BAR_GAP = 9; // px between bars

  function fmtTime(mins: number): string {
    if (mins < 60) return `${mins}m`;
    const h = Math.floor(mins / 60);
    const m = mins % 60;
    return m === 0 ? `${h}h` : `${h}h ${m}m`;
  }

  function fmtRate(rate: number | null): string {
    if (rate === null) return '—';
    return `${Math.round(rate * 100)}%`;
  }

  const byHour = $derived(today?.by_hour ?? Array(24).fill(0));
  const maxHour = $derived(Math.max(1, ...byHour));
  const hasData = $derived(today !== null && today.rounds > 0);

  // Hour labels: show every 6 hours
  const hourLabels = [0, 6, 12, 18];
</script>

<div class="view">
  <!-- Stat cards -->
  <div class="cards">
    <div class="card">
      <span class="card-label">{m.stats_rounds()}</span>
      <span class="card-value">{today?.rounds ?? '—'}</span>
    </div>
    <div class="card-divider"></div>
    <div class="card">
      <span class="card-label">{m.stats_focus_time()}</span>
      <span class="card-value">{today ? fmtTime(today.focus_mins) : '—'}</span>
    </div>
    <div class="card-divider"></div>
    <div class="card">
      <span class="card-label">{m.stats_completion()}</span>
      <span class="card-value">{today ? fmtRate(today.completion_rate) : '—'}</span>
    </div>
    <div class="card-divider"></div>
    <div class="card"><RewardSummary /></div>
  </div>

  <!-- Hourly breakdown -->
  <div class="section">
    <div class="section-header">
      <span class="section-title">{m.stats_sessions_by_hour()}</span>
      {#if !hasData}
        <span class="empty-hint">{m.stats_no_sessions_today()}</span>
      {/if}
    </div>

    <div class="chart-wrap">
      <svg
        width={CHART_W}
        height={CHART_H + 28}
        viewBox="0 0 {CHART_W} {CHART_H + 28}"
        class="chart"
      >
        {#each byHour as count, h}
          {@const barH = Math.max(2, Math.round((count / maxHour) * CHART_H))}
          {@const x = h * (BAR_W + BAR_GAP)}
          {@const y = CHART_H - barH}

          <!-- Bar -->
          <rect
            {x}
            {y}
            width={BAR_W}
            height={barH}
            rx="2"
            class="bar"
            class:bar-empty={count === 0}
          />

          <!-- Hour label (every 6 hours) -->
          {#if hourLabels.includes(h)}
            <text x={x + BAR_W / 2} y={CHART_H + 18} text-anchor="middle" class="hour-label"
              >{h === 0 ? '12a' : h === 12 ? '12p' : h < 12 ? `${h}a` : `${h - 12}p`}</text
            >
          {/if}
        {/each}

        <!-- Baseline -->
        <line x1="0" y1={CHART_H} x2={CHART_W} y2={CHART_H} class="baseline" />
      </svg>
    </div>
  </div>
</div>

<style>
  .view {
    display: flex;
    flex-direction: column;
    gap: 0;
    height: 100%;
    padding: 0;
  }

  /* ── Stat cards ──────────────────────────────────────────── */
  .cards {
    display: flex;
    align-items: stretch;
    border-bottom: 1px solid var(--color-separator);
  }

  .card {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 28px 12px;
  }

  .card-label {
    font-size: 0.68rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--color-foreground-darker);
  }

  .card-value {
    font-size: clamp(1.5rem, 3.8vw, 2.1rem);
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
    color: var(--color-foreground);
    line-height: 1;
  }

  .card-divider {
    width: 1px;
    background: var(--color-separator);
    align-self: stretch;
    margin: 12px 0;
  }

  /* ── Section ─────────────────────────────────────────────── */
  .section {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 20px 24px 16px;
    gap: 12px;
    overflow: hidden;
  }

  .section-header {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }

  .section-title {
    font-size: 0.68rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--color-foreground-darker);
  }

  .empty-hint {
    font-size: 0.72rem;
    color: color-mix(in oklch, var(--color-foreground-darker) 60%, transparent);
    font-style: italic;
  }

  /* ── Hourly chart ────────────────────────────────────────── */
  .chart-wrap {
    overflow-x: auto;
    overflow-y: hidden;
  }

  .chart {
    display: block;
  }

  .bar {
    fill: var(--color-focus-round);
    opacity: 0.85;
  }

  .bar-empty {
    fill: color-mix(in oklch, var(--color-foreground) 8%, transparent);
  }

  .hour-label {
    fill: var(--color-foreground-darker);
    font-size: 9px;
    font-variant-numeric: tabular-nums;
    cursor: default;
  }

  .baseline {
    stroke: var(--color-separator);
    stroke-width: 1;
  }

  .cards {
    padding: 24px;
    gap: 24px;
  }
  .card {
    align-items: flex-start;
    padding: 0;
    gap: 8px;
  }
  .card-divider {
    display: none;
  }
  .card-label,
  .section-title {
    font-size: 12px;
    letter-spacing: 0;
    text-transform: none;
  }
  .card-value {
    font-size: 28px;
    font-weight: 550;
    line-height: 36px;
  }
  .hour-label {
    font-size: 11px;
  }
  .empty-hint {
    color: var(--color-foreground-darker);
    font-size: 12px;
    font-style: normal;
  }
</style>
