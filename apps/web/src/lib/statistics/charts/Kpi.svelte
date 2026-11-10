<script lang="ts">
  // KPI tile with an optional growth badge against the previous period and a sparkline.
  import { growthInfo } from '../insights';
  let { label, value, growth = undefined, locale, compare = '', none = '', spark = undefined, hint = '' }: {
    label: string; value: string | number; growth?: number | null; locale: string; compare?: string; none?: string;
    /** Trend over the period (oldest first). */
    spark?: number[]; hint?: string;
  } = $props();
  const info = $derived(growthInfo(growth, locale));
  const path = $derived.by(() => {
    if (!spark || spark.length < 2) return '';
    const top = Math.max(...spark), step = 100 / (spark.length - 1);
    return spark.map((point, index) => `${index ? 'L' : 'M'}${(index * step).toFixed(2)},${(28 - (top > 0 ? point / top : 0) * 26).toFixed(2)}`).join('');
  });
  const area = $derived(path ? `${path}L100,30L0,30Z` : '');
</script>

<article class="kpi" class:with-spark={!!path}>
  <span class="label">{label}</span>
  <strong>{value}</strong>
  <div class="foot">
    {#if growth !== undefined}
      <em class={info.direction} title={info.direction === 'none' ? none : compare}>
        {#if info.direction === 'up'}<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 14l5-5 5 5" /></svg>
        {:else if info.direction === 'down'}<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 10l5 5 5-5" /></svg>{/if}
        {info.direction === 'none' ? '—' : info.label}<span class="h-sr"> {info.direction === 'none' ? none : compare}</span>
      </em>
    {/if}
    {#if hint}<small>{hint}</small>{/if}
  </div>
  {#if path}<svg class="spark" viewBox="0 0 100 30" preserveAspectRatio="none" aria-hidden="true"><path d={area} class="area" /><path d={path} class="line" vector-effect="non-scaling-stroke" /></svg>{/if}
</article>

<style>
  .kpi { position: relative; display: grid; gap: 6px; align-content: start; padding: 18px 20px 16px; border-radius: var(--r-md); background: var(--bg-2); border: 1px solid var(--line); min-width: 0; overflow: hidden; isolation: isolate; }
  .label { font-size: 12px; color: var(--mu); font-weight: 600; text-transform: uppercase; letter-spacing: .06em; }
  strong { font-size: clamp(1.35rem, 2.2vw, 1.8rem); font-weight: 800; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .foot { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-height: 20px; }
  .foot small { font-size: 11.5px; color: var(--mu-2); }
  em { display: inline-flex; align-items: center; gap: 2px; width: fit-content; font-style: normal; font-size: 12px; font-weight: 700; padding: 3px 8px; border-radius: var(--r-pill); background: var(--pn); color: var(--mu); font-variant-numeric: tabular-nums; }
  em.up { color: var(--ok); background: var(--ok-soft); }
  em.down { color: var(--err); background: var(--err-soft); }
  em svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 2.2; stroke-linecap: round; stroke-linejoin: round; }
  .kpi.with-spark { padding-bottom: 46px; }
  .spark { position: absolute; left: 0; right: 0; bottom: 0; width: 100%; height: 34px; z-index: -1; color: var(--ac); opacity: .9; }
  .spark .area { fill: currentColor; opacity: .08; }
  .spark .line { fill: none; stroke: currentColor; stroke-width: 1.5; opacity: .55; }
</style>
