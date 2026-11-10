<script lang="ts">
  // Ranked titles with artwork; each row may link to the title's own page.
  interface Entry { key: string; title: string; sub?: string; image?: string; href?: string; value: number; detail: string }
  let { items, empty, limit = 8 }: { items: Entry[]; empty: string; limit?: number } = $props();
  const shown = $derived(items.slice(0, limit));
  const max = $derived(Math.max(0, ...shown.map(item => item.value)));
  let broken = $state<Record<string, boolean>>({});
</script>

{#if shown.length}
  <ol class="top">
    {#each shown as item, index (item.key)}
      <li>
        <span class="rank" class:first={index === 0}>{index + 1}</span>
        <span class="art" aria-hidden="true">
          {#if item.image && !broken[item.key]}<img src={item.image} alt="" loading="lazy" decoding="async" onerror={() => broken = { ...broken, [item.key]: true }} />{:else}<span>{item.title.slice(0, 1).toUpperCase()}</span>{/if}
        </span>
        <span class="body">
          <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- callers pass resolved hrefs. -->
          {#if item.href}<a class="title" href={item.href}>{item.title}</a>{:else}<span class="title">{item.title}</span>{/if}
          {#if item.sub}<small>{item.sub}</small>{/if}
          <span class="track"><span class="fill" class:first={index === 0} style:--w={max > 0 ? item.value / max : 0} style:--i={index}></span></span>
        </span>
        <span class="value">{item.detail}</span>
      </li>
    {/each}
  </ol>
{:else}<p class="empty">{empty}</p>{/if}

<style>
  .top { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
  li { display: grid; grid-template-columns: 22px 40px minmax(0, 1fr) auto; gap: 12px; align-items: center; padding: 8px; margin: 0 -8px; border-radius: var(--r-sm); transition: background var(--t-fast) var(--ease); }
  li:hover { background: var(--pn); }
  .rank { font-size: 12px; font-weight: 800; color: var(--mu-2); text-align: center; font-variant-numeric: tabular-nums; }
  .rank.first { color: var(--ac); }
  .art { width: 40px; aspect-ratio: 2 / 3; border-radius: var(--r-xs); overflow: hidden; background: var(--pn-2); display: grid; place-items: center; }
  .art img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .art span { color: var(--mu); font-weight: 800; }
  .body { display: grid; gap: 3px; min-width: 0; }
  .title { font-size: 13.5px; font-weight: 600; color: var(--tx); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-decoration: none; }
  a.title:hover { text-decoration: underline; }
  small { color: var(--mu); font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .track { display: block; height: 4px; margin-top: 3px; border-radius: var(--r-pill); background: var(--pn); overflow: hidden; }
  .fill { display: block; height: 100%; width: calc(var(--w) * 100%); background: var(--mu-2); border-radius: var(--r-pill); transform-origin: left; animation: grow var(--t-slow) var(--ease) both; animation-delay: calc(var(--i) * 40ms); }
  .fill.first { background: var(--ac); }
  .value { font-size: 12.5px; color: var(--mu); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .empty { color: var(--mu); margin: 0; }
  @keyframes grow { from { transform: scaleX(0); } to { transform: scaleX(1); } }
  @media (prefers-reduced-motion: reduce) { .fill { animation: none; } }
</style>
