<script lang="ts">
  // Share of a whole as a ring, with the total in the middle and a legend.
  interface Slice { key: string; label: string; value: number; detail?: string; tone?: string }
  let { items, total = '', caption = '', empty, percent }: {
    items: Slice[]; total?: string; caption?: string; empty: string; percent: (value: number) => string;
  } = $props();
  const tones = ['var(--ac)', 'var(--info)', 'var(--ok)', 'var(--warn)', 'var(--err)', 'var(--mu-2)'];
  const sum = $derived(items.reduce((value, item) => value + Math.max(0, item.value), 0));
  const R = 42, C = 2 * Math.PI * R;
  const arcs = $derived.by(() => {
    let offset = 0;
    return items.filter(item => item.value > 0).map((item, index) => {
      const share = sum > 0 ? item.value / sum : 0;
      const arc = { ...item, share, color: item.tone ?? tones[index % tones.length], dash: `${Math.max(0, share * C - (items.length > 1 ? 1.5 : 0))} ${C}`, offset: -offset * C };
      offset += share;
      return arc;
    });
  });
</script>

{#if sum > 0}
  <div class="donut">
    <svg viewBox="0 0 100 100" role="img" aria-label={caption}>
      <circle class="track" cx="50" cy="50" r={R} />
      {#each arcs as arc (arc.key)}<circle class="arc" cx="50" cy="50" r={R} stroke={arc.color} stroke-dasharray={arc.dash} stroke-dashoffset={arc.offset}><title>{arc.label} · {percent(arc.share)}</title></circle>{/each}
    </svg>
    {#if total}<div class="center" aria-hidden="true"><strong>{total}</strong></div>{/if}
    <ul class="legend">
      {#each arcs as arc (arc.key)}<li><i style:background={arc.color}></i><span>{arc.label}</span><b>{percent(arc.share)}</b>{#if arc.detail}<small>{arc.detail}</small>{/if}</li>{/each}
    </ul>
  </div>
{:else}<p class="empty">{empty}</p>{/if}

<style>
  .donut { display: grid; grid-template-columns: minmax(100px, 132px) minmax(0, 1fr); grid-template-areas: "ring legend"; align-items: center; gap: 22px; position: relative; }
  svg { grid-area: ring; width: 100%; aspect-ratio: 1; transform: rotate(-90deg); }
  .track { fill: none; stroke: var(--pn); stroke-width: 11; }
  .arc { fill: none; stroke-width: 11; transition: stroke-dasharray var(--t-slow) var(--ease); }
  .center { grid-area: ring; display: grid; place-content: center; text-align: center; pointer-events: none; }
  .center strong { font-size: 1.2rem; font-weight: 800; font-variant-numeric: tabular-nums; }
    .legend { grid-area: legend; list-style: none; margin: 0; padding: 0; display: grid; gap: 10px; min-width: 0; }
  .legend li { display: grid; grid-template-columns: 10px minmax(0, 1fr) auto; column-gap: 10px; align-items: center; font-size: 13px; }
  .legend i { width: 10px; height: 10px; border-radius: 3px; }
  .legend span { overflow-wrap: anywhere; }
  .legend b { font-variant-numeric: tabular-nums; font-weight: 700; }
  .legend small { grid-column: 2 / 4; color: var(--mu); font-size: 11.5px; }
  .empty { color: var(--mu); margin: 0; }
  @media (max-width: 420px) { .donut { grid-template-columns: 1fr; grid-template-areas: "ring" "legend"; } svg, .center { max-width: 160px; justify-self: center; } }
</style>
