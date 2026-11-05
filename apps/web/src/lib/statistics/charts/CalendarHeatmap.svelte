<script lang="ts">
  // Activity calendar: one column per week, Monday at the top.
  import { calendarWeeks, monthName, weekdayName } from '../insights';
  let { days, from, to, label, locale, describe, less, more }: {
    days: { date: string; plays: number; seconds: number }[]; from: string; to: string; label: string; locale: string;
    describe: (date: string, seconds: number, plays: number) => string; less: string; more: string;
  } = $props();
  const weeks = $derived(calendarWeeks(days, from, to));
  // Month label above the first week that contains the 1st day of that month.
  const months = $derived(weeks.map((week, index) => {
    const first = week.find(cell => cell.date.endsWith('-01') && cell.inRange) ?? (index === 0 ? week.find(cell => cell.inRange) : undefined);
    return first ? monthName(Number(first.date.slice(5, 7)), locale, 'short') : '';
  }));
</script>

<div class="calendar-scroll">
  <div class="calendar" role="img" aria-label={label} style:--weeks={weeks.length}>
    <span></span>
    {#each months as month, index (index)}<small class="month">{month}</small>{/each}
    {#each [0, 1, 2, 3, 4, 5, 6] as day (day)}
      <small class="weekday" class:hidden={day % 2 === 1}>{weekdayName(day, locale, 'short')}</small>
      {#each weeks as week (week[0].date)}
        {@const cell = week[day]}
        <span class={`cell l${cell.level}`} class:outside={!cell.inRange} title={cell.inRange ? describe(cell.date, cell.seconds, cell.plays) : ''}></span>
      {/each}
    {/each}
  </div>
</div>
<div class="legend" aria-hidden="true"><small>{less}</small>{#each [0, 1, 2, 3, 4] as value (value)}<span class={`cell l${value}`}></span>{/each}<small>{more}</small></div>

<style>
  .calendar-scroll { overflow-x: auto; padding-bottom: 4px; }
  .calendar { display: grid; grid-template-columns: minmax(30px, auto) repeat(var(--weeks), minmax(11px, 1fr)); grid-template-rows: auto repeat(7, auto); grid-auto-flow: row; gap: 3px; min-width: calc(var(--weeks) * 14px + 34px); }
  .cell { display: block; aspect-ratio: 1; border-radius: 3px; background: var(--pn); }
  .cell.outside { visibility: hidden; }
  .l1 { background: var(--ac-soft); }
  .l2, .l3, .l4 { background: var(--ac); }
  .l2 { opacity: 0.45; }
  .l3 { opacity: 0.72; }
  small { font-size: 10.5px; color: var(--mu); white-space: nowrap; text-transform: capitalize; }
  .month { overflow: visible; }
  .weekday { padding-right: 6px; align-self: center; }
  .hidden { visibility: hidden; }
  .legend { display: flex; align-items: center; justify-content: flex-end; gap: 4px; margin-top: 10px; }
  .legend .cell { width: 11px; }
</style>
