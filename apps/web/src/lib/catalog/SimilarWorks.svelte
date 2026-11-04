<script lang="ts">
  import { resolve } from '$app/paths';
  import { request, type Work } from './api';
  import MediaShelf from '../ui/MediaShelf.svelte';
  import Poster from './Poster.svelte';
  import DiscoverFeed from './DiscoverFeed.svelte';

  let { work, candidates = [] }: { work: Work; candidates?: Work[] } = $props();
  let fetchedItems = $state<Work[]>([]);

  $effect(() => {
    const currentWork = work;
    if (currentWork.synthetic) {
      fetchedItems = [];
      return;
    }
    const controller = new AbortController();
    const kind = currentWork.kind === 'series' ? 'series' : 'film';
    // Candidats : les plus récents du même type, plus ceux qui partagent les genres principaux.
    const queries = [
      { kind, limit: '60', count: 'false', sort: 'added-desc' },
      ...(currentWork.genres ?? []).slice(0, 3).map(genre => ({ kind, genre, limit: '40', count: 'false', sort: 'year-desc' }))
    ];
    fetchedItems = [];
    void Promise.allSettled(queries.map(params => request<{ items: Work[] }>(`/api/v1/catalog?${new URLSearchParams(params)}`, controller.signal)))
      .then(results => {
        if (controller.signal.aborted) return;
        fetchedItems = results.flatMap(result => result.status === 'fulfilled' && Array.isArray(result.value.items) ? result.value.items : []);
      });
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
      .slice(0, 24)
      .map(entry => entry.candidate);
  });

  function itemHref(item: Work) {
    return item.kind === 'series'
      ? resolve(`series/?work=${encodeURIComponent(item.id)}`)
      : resolve(`films/?work=${encodeURIComponent(item.id)}`);
  }
</script>

{#if !work.synthetic}
  <div class="similar-works-section">
    {#if similarWorks.length > 0}
      <MediaShelf title={work.kind === 'series' ? 'Séries similaires' : 'Films similaires'}>
        {#each similarWorks as item (item.id)}
          <Poster work={item} href={itemHref(item)} />
        {/each}
      </MediaShelf>
    {/if}
    <!-- Puis un fil sans fin du même type, en commençant par les genres de l'œuvre. -->
    {#key work.id}
      <DiscoverFeed
        films={work.kind === 'series' ? [] : [work, ...similarWorks]}
        series={work.kind === 'series' ? [work, ...similarWorks] : []}
        exclude={[work.id, ...similarWorks.map(item => item.id)]}
        kind={work.kind === 'series' ? 'series' : 'film'}
        gridTitle={work.kind === 'series' ? 'Encore plus de séries' : 'Encore plus de films'}
        detailUrl={itemHref}
      />
    {/key}
  </div>
{/if}

<style>
  /* Toujours en bas de la fiche, séparé du reste */
  .similar-works-section {
    margin-top: 48px;
    padding-top: 24px;
    border-top: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: clamp(28px, 4vw, 48px);
  }
</style>
