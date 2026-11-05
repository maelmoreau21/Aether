<script lang="ts">
  // KPI tile with an optional growth badge against the previous period.
  import { growthInfo } from '../insights';
  let { label, value, growth = undefined, locale, compare = '', none = '' }: {
    label: string; value: string | number; growth?: number | null; locale: string; compare?: string; none?: string;
  } = $props();
  const info = $derived(growthInfo(growth, locale));
</script>

<article class="kpi">
  <strong>{value}</strong>
  <span>{label}</span>
  {#if growth !== undefined}
    <em class={info.direction} title={info.direction === 'none' ? none : compare}>
      {#if info.direction === 'up'}<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 14l5-5 5 5" /></svg>
      {:else if info.direction === 'down'}<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 10l5 5 5-5" /></svg>{/if}
      {info.direction === 'none' ? '—' : info.label}<span class="h-sr"> {info.direction === 'none' ? none : compare}</span>
    </em>
  {/if}
</article>

<style>
  .kpi { display: grid; gap: 8px; align-content: start; padding: 20px; border-radius: var(--r-md); background: var(--bg-2); border: 1px solid var(--line); min-width: 0; }
  strong { font-size: clamp(1.35rem, 2.2vw, 1.75rem); font-weight: 800; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  span { font-size: 12.5px; color: var(--mu); }
  em { display: inline-flex; align-items: center; gap: 2px; width: fit-content; font-style: normal; font-size: 12px; font-weight: 700; padding: 3px 8px; border-radius: var(--r-pill); background: var(--pn); color: var(--mu); font-variant-numeric: tabular-nums; }
  em.up { color: var(--ok); background: var(--ok-soft); }
  em.down { color: var(--err); background: var(--err-soft); }
  svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 2.2; stroke-linecap: round; stroke-linejoin: round; }
</style>
