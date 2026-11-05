<script lang="ts">
  // Ranked horizontal meters (genres, resolutions, series…).
  interface MeterItem { key: string; label: string; value: number; detail?: string; sub?: string }
  let { items, empty, format = (value: number) => String(value), limit = 10 }: {
    items: MeterItem[]; empty: string; format?: (value: number) => string; limit?: number;
  } = $props();
  const shown = $derived(items.slice(0, limit));
  const max = $derived(Math.max(0, ...shown.map(item => item.value)));
</script>

{#if shown.length}
  <ol class="meter">
    {#each shown as item, index (item.key)}
      <li>
        <div class="row"><span class="name">{item.label}{#if item.sub}<small>{item.sub}</small>{/if}</span><span class="value">{item.detail ?? format(item.value)}</span></div>
        <span class="track" aria-hidden="true"><span class="fill" class:first={index === 0} style:--w={max > 0 ? item.value / max : 0} style:--i={index}></span></span>
      </li>
    {/each}
  </ol>
{:else}
  <p class="empty">{empty}</p>
{/if}

<style>
  .meter { list-style: none; margin: 0; padding: 0; display: grid; gap: 14px; }
  .row { display: flex; justify-content: space-between; gap: 12px; font-size: 13.5px; margin-bottom: 6px; }
  .name { min-width: 0; overflow-wrap: anywhere; color: var(--tx); }
  .name small { display: block; color: var(--mu); font-size: 12px; margin-top: 2px; }
  .value { color: var(--mu); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .track { display: block; height: 6px; border-radius: var(--r-pill); background: var(--pn); overflow: hidden; }
  .fill { display: block; height: 100%; width: calc(var(--w) * 100%); border-radius: var(--r-pill); background: var(--mu-2); transform-origin: left; animation: grow var(--t-slow) var(--ease) both; animation-delay: calc(var(--i) * 40ms); }
  .fill.first { background: var(--ac); }
  .empty { color: var(--mu); margin: 0; }
  @keyframes grow { from { transform: scaleX(0); } to { transform: scaleX(1); } }
  @media (prefers-reduced-motion: reduce) { .fill { animation: none; } }
</style>
