<script lang="ts">
  // 7 × 24 heatmap of session starts (local time).
  import { hoursMatrix, level, weekdayName } from '../insights';
  let { cells, label, locale, hourLabel, unit }: {
    cells: { cell: number; plays: number; seconds: number }[]; label: string; locale: string;
    hourLabel: (hour: number) => string; unit: (value: number) => string;
  } = $props();
  const rows = $derived(hoursMatrix(cells));
  const max = $derived(Math.max(0, ...rows.flat()));
</script>

<div class="heatmap" role="img" aria-label={label}>
  <span></span>
  {#each Array.from({ length: 24 }, (_, hour) => hour) as hour (hour)}<small class="hour" class:hidden={hour % 3 !== 0}>{hour}</small>{/each}
  {#each rows as row, day (day)}
    <small class="day">{weekdayName(day, locale, 'short')}</small>
    {#each row as value, hour (hour)}
      <span class={`cell l${level(value, max)}`} title={`${weekdayName(day, locale)} · ${hourLabel(hour)} · ${unit(value)}`}></span>
    {/each}
  {/each}
</div>

<style>
  .heatmap { display: grid; grid-template-columns: minmax(28px, auto) repeat(24, minmax(0, 1fr)); gap: 3px; align-items: center; }
  .cell { aspect-ratio: 1; border-radius: 4px; background: var(--pn); }
  .l1 { background: var(--ac-soft); }
  .l2, .l3, .l4 { background: var(--ac); }
  .l2 { opacity: 0.45; }
  .l3 { opacity: 0.72; }
  small { font-size: 10.5px; color: var(--mu); font-variant-numeric: tabular-nums; }
  .hour { text-align: center; }
  .hidden { visibility: hidden; }
  .day { text-transform: capitalize; padding-right: 4px; }
</style>
