<script lang="ts">
  import type { Snippet } from 'svelte';
  import { type Work, localizedPlot, localizedTagline } from './api';
  import { locale, t, fmt } from '../i18n';
  import { meaningfulText } from './detail-facts';

  // Onglet « Détails » : synopsis complet, étiquettes, sources externes et fiche technique.
  let { work, children }: { work: Work; children?: Snippet } = $props();

  const plot = $derived(localizedPlot(work, $locale));
  const tagline = $derived(localizedTagline(work, $locale));
  const placeholders = $derived([$t('indexed_local_media'), $t('local_media')]);
  const synopsis = $derived(meaningfulText(plot, placeholders) ? plot : $t('detail_no_synopsis'));
  const providers = $derived.by(() => {
    const ratings = (work.ratings ?? []).map(r => r.provider.toLowerCase());
    return [
      work.externalIds?.imdb || ratings.includes('imdb') ? 'IMDb' : '',
      work.externalIds?.tmdb || ratings.some(p => p === 'tmdb' || p === 'themoviedb') ? 'TMDB' : '',
      work.externalIds?.tvdb || ratings.includes('tvdb') ? 'TVDB' : ''
    ].filter(Boolean);
  });
</script>

<div class="work-details">
  {#if meaningfulText(tagline, placeholders)}<p class="tagline">« {tagline} »</p>{/if}
  <p class="synopsis">{synopsis}</p>

  {#if work.tags?.length}
    <p class="tags-line">
      <span class="tags-label">{$t('tags')}</span>
      <strong>{work.tags.join(', ')}</strong>
    </p>
  {/if}

  {#if providers.length || work.ratings?.length}
    <div class="provider-badges-row" aria-label={$t('external_sources_and_ratings')}>
      {#each providers as provider (provider)}<span class="provider-logo">{provider}</span>{/each}
      {#each work.ratings ?? [] as r, idx (idx)}
        <span class="provider-score-pill">
          <strong>{r.provider.toUpperCase()}</strong>
          {$fmt.decimal(Number(r.value), 1)} / {r.max}
        </span>
      {/each}
    </div>
  {/if}

  {#if work.directors?.length || work.writers?.length || work.genres?.length || (work.originalTitle && work.originalTitle !== work.title)}
    <dl class="meta-table">
      {#if work.directors?.length}
        <div class="meta-row">
          <dt>{work.directors.length > 1 ? $t('detail_directors') : $t('director')}</dt>
          <dd>{work.directors.join(', ')}</dd>
        </div>
      {/if}
      {#if work.writers?.length}
        <div class="meta-row">
          <dt>{work.writers.length > 1 ? $t('detail_writers') : $t('screenplay')}</dt>
          <dd>{work.writers.join(', ')}</dd>
        </div>
      {/if}
      {#if work.genres?.length}
        <div class="meta-row">
          <dt>{$t('stats_genres_title')}</dt>
          <dd>{work.genres.join(', ')}</dd>
        </div>
      {/if}
      {#if work.originalTitle && work.originalTitle !== work.title}
        <div class="meta-row">
          <dt>{$t('original_title')}</dt>
          <dd>{work.originalTitle}</dd>
        </div>
      {/if}
    </dl>
  {/if}

  {#if children}{@render children()}{/if}
</div>

<style>
  .work-details {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 16px;
    max-width: 860px;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .tagline {
    margin: 0;
    font-size: 15px;
    font-style: italic;
    color: var(--mu);
  }

  .synopsis {
    max-width: 72ch;
    margin: 0;
    font-size: 17px;
    line-height: 1.65;
    color: var(--tx);
  }

  .tags-line {
    margin: 0;
    max-width: 90ch;
    font-size: 14px;
    line-height: 1.6;
    color: var(--mu);
  }

  .tags-label { margin-right: 6px; }

  .tags-line strong {
    color: var(--tx);
    font-weight: 600;
  }

  .provider-badges-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .provider-logo,
  .provider-score-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border-radius: var(--r-xs);
    background: var(--pn-2);
    color: var(--tx);
    font-size: 12px;
    font-weight: 700;
  }

  .provider-score-pill strong { color: var(--mu); }

  .meta-table {
    display: grid;
    gap: 10px;
    margin: 4px 0 0;
    width: 100%;
    max-width: 760px;
    font-size: 14px;
  }

  .meta-row {
    display: grid;
    grid-template-columns: 130px minmax(0, 1fr);
    gap: 16px;
    align-items: baseline;
  }

  .meta-row dt {
    color: var(--mu);
    font-weight: 600;
  }

  .meta-row dd {
    margin: 0;
    color: var(--tx);
    font-weight: 600;
  }

  @media (max-width: 760px) {
    .synopsis { font-size: 15px; }
    .meta-row { grid-template-columns: 100px minmax(0, 1fr); gap: 10px; }
  }
</style>
