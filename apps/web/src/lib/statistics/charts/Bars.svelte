<script lang="ts">
  // Vertical bars drawn with CSS; the highest value takes the accent.
  interface BarItem { key: string; label: string; value: number; hint?: string }
  let { items, label, format = (value: number) => String(value), height = 170, everyLabel = 1 }: {
    items: BarItem[]; label: string; format?: (value: number) => string; height?: number; everyLabel?: number;
  } = $props();
  const max = $derived(Math.max(0, ...items.map(item => item.value)));
</script>

<div class="bars" role="img" aria-label={label} style:--bars-height={`${height}px`} style:--bars-count={items.length}>
  {#each items as item, index (item.key)}
    <div class="bar" title={`${item.hint ?? item.label} · ${format(item.value)}`}>
      <span class="track"><span class="fill" class:peak={max > 0 && item.value === max} style:--h={max > 0 ? item.value / max : 0} style:--i={index}></span></span>
      <small class:hidden={index % everyLabel !== 0}>{item.label}</small>
    </div>
  {/each}
</div>
<ul class="h-sr">{#each items as item (item.key)}<li>{item.hint ?? item.label} : {format(item.value)}</li>{/each}</ul>

<style>
  .bars { display: grid; grid-template-columns: repeat(var(--bars-count), minmax(0, 1fr)); gap: clamp(2px, 0.6vw, 8px); height: var(--bars-height); align-items: stretch; }
  .bar { display: grid; grid-template-rows: 1fr auto; gap: 8px; min-width: 0; }
  .track { position: relative; display: block; border-radius: var(--r-xs); background: var(--pn); overflow: hidden; }
  .fill { position: absolute; inset: auto 0 0 0; height: calc(var(--h) * 100%); border-radius: var(--r-xs); background: var(--mu-2); opacity: 0.7; transform-origin: bottom; animation: grow var(--t-slow) var(--ease) both; animation-delay: calc(var(--i) * 18ms); }
  .fill.peak { background: var(--ac); opacity: 1; }
  small { font-size: 11px; color: var(--mu); text-align: center; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-variant-numeric: tabular-nums; }
  small.hidden { visibility: hidden; }
  @keyframes grow { from { transform: scaleY(0); } to { transform: scaleY(1); } }
  @media (prefers-reduced-motion: reduce) { .fill { animation: none; } }
</style>
