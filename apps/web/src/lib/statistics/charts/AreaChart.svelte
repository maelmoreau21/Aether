<script lang="ts">
  // Time series as a smooth area with a y grid, a hover guide and a tooltip.
  // The geometry is SVG (stretched); labels stay HTML so text never distorts.
  interface Point { key: string; label: string; value: number; hint?: string }
  let { items, label, format = (value: number) => String(value), height = 200, everyLabel = 1, max = undefined, tone = 'accent', duration = false }: {
    items: Point[]; label: string; format?: (value: number) => string; height?: number; everyLabel?: number;
    /** Fixed top of the scale (e.g. 1 for shares); otherwise a rounded maximum. */
    max?: number; tone?: 'accent' | 'info' | 'ok';
    /** Values are seconds: grid lines fall on whole minutes or hours. */
    duration?: boolean;
  } = $props();

  const W = 1000, H = 300;
  /** Round step (1, 2, 5 × 10ⁿ, in hours or minutes for durations) splitting `value` in about four. */
  function step(value: number): number {
    const unit = !duration ? 1 : value >= 2 * 3600 ? 3600 : 60;
    const raw = Math.max(value / unit / 4, duration ? 1 : Number.EPSILON);
    const power = 10 ** Math.floor(Math.log10(raw));
    return ([1, 2, 5, 10].find(candidate => candidate * power >= raw) ?? 10) * power * unit;
  }
  const peak = $derived(Math.max(0, ...items.map(item => item.value)));
  const tick = $derived(max !== undefined ? max / 4 : peak > 0 ? step(peak) : 1);
  const top = $derived(max ?? (peak > 0 ? Math.ceil(peak / tick) * tick : 4));
  const x = (index: number) => items.length <= 1 ? W / 2 : index / (items.length - 1) * W;
  const y = (value: number) => H - Math.min(1, Math.max(0, value / top)) * H;
  const line = $derived.by(() => {
    if (!items.length) return '';
    const points = items.map((item, index) => [x(index), y(item.value)] as const);
    if (points.length === 1) return `M0 ${points[0][1]} L${W} ${points[0][1]}`;
    // Monotone-ish smoothing: horizontal tangents keep the curve inside its bounds.
    let path = `M${points[0][0]} ${points[0][1]}`;
    for (let index = 1; index < points.length; index++) {
      const [x0, y0] = points[index - 1], [x1, y1] = points[index], mid = (x0 + x1) / 2;
      path += ` C${mid} ${y0} ${mid} ${y1} ${x1} ${y1}`;
    }
    return path;
  });
  const area = $derived(line ? `${line} L${items.length <= 1 ? W : x(items.length - 1)} ${H} L0 ${H} Z` : '');
  const grid = $derived(Array.from({ length: Math.round(top / tick) + 1 }, (_, index) => index * tick).reverse().map(value => ({ ratio: value / top, text: format(value) })));
  const uid = $props.id();
  const gradient = `area-${uid}`;

  let hover = $state<number | null>(null);
  let plot: HTMLDivElement;
  function move(event: PointerEvent) {
    if (!items.length) return;
    const box = plot.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (event.clientX - box.left) / box.width));
    hover = items.length <= 1 ? 0 : Math.round(ratio * (items.length - 1));
  }
  const active = $derived(hover === null ? null : items[hover]);
</script>

<figure class={`area tone-${tone}`} style:--area-height={`${height}px`}>
  <div class="axis" aria-hidden="true">{#each grid as row (row.ratio)}<span style:--r={row.ratio}>{row.text}</span>{/each}</div>
  <div class="plot" bind:this={plot} role="img" aria-label={label} onpointermove={move} onpointerleave={() => hover = null}>
    {#each grid as row (row.ratio)}<i class="rule" class:base={row.ratio === 0} style:--r={row.ratio} aria-hidden="true"></i>{/each}
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden="true">
      <defs><linearGradient id={gradient} x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="currentColor" stop-opacity="0.32" /><stop offset="1" stop-color="currentColor" stop-opacity="0" /></linearGradient></defs>
      {#if area}<path class="fill" d={area} fill={`url(#${gradient})`} />{/if}
      {#if line}<path class="stroke" d={line} vector-effect="non-scaling-stroke" />{/if}
    </svg>
    {#if active && hover !== null}
      <i class="guide" style:--x={items.length <= 1 ? 0.5 : hover / (items.length - 1)} aria-hidden="true"><b style:--y={1 - Math.min(1, active.value / top)}></b></i>
      <div class="tip" class:flip={items.length > 1 && hover / (items.length - 1) > 0.7} style:--x={items.length <= 1 ? 0.5 : hover / (items.length - 1)} aria-hidden="true"><small>{active.hint ?? active.label}</small><strong>{format(active.value)}</strong></div>
    {/if}
  </div>
  <div class="labels" aria-hidden="true">{#each items as item, index (item.key)}{#if index % everyLabel === 0}<small style:--x={items.length <= 1 ? 0.5 : index / (items.length - 1)}>{item.label}</small>{/if}{/each}</div>
  <ul class="h-sr">{#each items as item (item.key)}<li>{item.hint ?? item.label} : {format(item.value)}</li>{/each}</ul>
</figure>

<style>
  .area { container-type: inline-size; margin: 0; display: grid; grid-template-columns: auto minmax(0, 1fr); grid-template-rows: var(--area-height) auto; column-gap: 12px; row-gap: 8px; min-width: 0; }
  .tone-accent { color: var(--ac); } .tone-info { color: var(--info); } .tone-ok { color: var(--ok); }
  .axis { position: relative; min-width: 34px; }
  .axis span { position: absolute; right: 0; top: calc((1 - var(--r)) * 100%); transform: translateY(-50%); font-size: 11px; color: var(--mu-2); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .plot { position: relative; min-width: 0; cursor: crosshair; touch-action: pan-y; }
  .rule { position: absolute; left: 0; right: 0; top: calc((1 - var(--r)) * 100%); border-top: 1px dashed var(--line); }
  .rule.base { border-top-style: solid; border-color: var(--line-2); }
  svg { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; }
  .fill { animation: rise var(--t-slow) var(--ease) both; }
  .stroke { fill: none; stroke: currentColor; stroke-width: 2.2; stroke-linejoin: round; stroke-linecap: round; }
  .guide { position: absolute; top: 0; bottom: 0; left: calc(var(--x) * 100%); border-left: 1px solid var(--line-2); pointer-events: none; }
  .guide b { position: absolute; left: -5px; top: calc(var(--y) * 100% - 5px); width: 10px; height: 10px; border-radius: 50%; background: currentColor; box-shadow: 0 0 0 3px var(--bg-2); }
  .tip { position: absolute; top: 4px; left: calc(var(--x) * 100%); transform: translateX(12px); display: grid; gap: 2px; padding: 8px 11px; border-radius: var(--r-sm); background: var(--bg-3); border: 1px solid var(--line-2); box-shadow: var(--shadow); pointer-events: none; white-space: nowrap; z-index: 1; }
  .tip.flip { transform: translateX(calc(-100% - 12px)); }
  .tip small { color: var(--mu); font-size: 11.5px; }
  .tip strong { color: var(--tx); font-size: 14px; font-variant-numeric: tabular-nums; }
  .labels { grid-column: 2; position: relative; height: 16px; }
  .labels small { position: absolute; left: calc(var(--x) * 100%); transform: translateX(-50%); font-size: 11px; color: var(--mu); white-space: nowrap; font-variant-numeric: tabular-nums; }
  .labels small:first-child { transform: none; }
  .labels small:last-child:not(:first-child) { transform: translateX(-100%); }
  @container (max-width: 520px) { .labels small:nth-child(even) { display: none; } }
  @keyframes rise { from { opacity: 0; } to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .fill { animation: none; } }
</style>
