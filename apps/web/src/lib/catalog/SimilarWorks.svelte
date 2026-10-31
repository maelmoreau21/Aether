<script lang="ts">
  import { resolve } from '$app/paths';
  import { request, type Work } from './api';
  import MediaShelf from '../ui/MediaShelf.svelte';
  import Poster from './Poster.svelte';

  let { work, candidates = [] }: { work: Work; candidates?: Work[] } = $props();
  let fetchedItems = $state<Work[]>([]);

  $effect(() => {
    const currentWork = work;
    if (currentWork.synthetic) {
      fetchedItems = [];
      return;
    }
    const controller = new AbortController();
    const params = new URLSearchParams({
      kind: currentWork.kind === 'series' ? 'series' : 'film',
      limit: '36',
      count: 'false'
    });
    void request<{ items: Work[] }>(`/api/v1/catalog?${params}`, controller.signal)
      .then(result => {
        if (!controller.signal.aborted && Array.isArray(result.items)) {
          fetchedItems = result.items;
        }
      })
      .catch(() => {});
    return () => controller.abort();
  });

  const similarWorks = $derived.by(() => {
    const seen: Record<string, boolean> = {};
    const pool: Work[] = [];
    for (const item of [...candidates, ...fetchedItems]) {
      if (item.id && item.id !== work.id && item.kind === work.kind && !seen[item.id]) {
        seen[item.id] = true;
        pool.push(item);
      }
    }
    if (!pool.length) return [];

    const workGenres = (work.genres ?? []).map(g => g.toLowerCase());
    const workDirectors = (work.directors ?? []).map(d => d.toLowerCase());
    const workActors = (work.actors ?? []).map(a => a.name.toLowerCase());

    return pool
      .map(candidate => {
        let score = 0;
        for (const genre of candidate.genres ?? []) {
          if (workGenres.includes(genre.toLowerCase())) score += 4;
        }
        for (const director of candidate.directors ?? []) {
          if (workDirectors.includes(director.toLowerCase())) score += 6;
        }
        for (const actor of candidate.actors ?? []) {
          if (workActors.includes(actor.name.toLowerCase())) score += 2;
        }
        if (work.libraryId && candidate.libraryId === work.libraryId) {
          score += 2;
        }
        if (work.year && candidate.year && Math.abs(work.year - candidate.year) <= 5) {
          score += 1;
        }
        return { candidate, score };
      })
      .sort((a, b) => b.score - a.score || (b.candidate.rating ?? 0) - (a.candidate.rating ?? 0))
      .slice(0, 16)
      .map(entry => entry.candidate);
  });

  function itemHref(item: Work) {
    return item.kind === 'series'
      ? resolve(`series/?work=${encodeURIComponent(item.id)}`)
      : resolve(`films/?work=${encodeURIComponent(item.id)}`);
  }
</script>

{#if similarWorks.length > 0}
  <div class="similar-works-section">
    <MediaShelf title="Plus de ce genre">
      {#each similarWorks as item (item.id)}
        <Poster work={item} href={itemHref(item)} />
      {/each}
    </MediaShelf>
  </div>
{/if}

<style>
  .similar-works-section {
    margin-top: 36px;
  }
</style>
