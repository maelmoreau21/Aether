<script lang="ts">
  import type { Work } from './api';
  import Icon from '../ui/Icon.svelte';
  import { history } from './watch-state';
  import { pickSurprise, surpriseCandidates, surpriseGenres, type SurpriseFilters } from './surprise';
  import { t } from '../i18n';

  let { works = [], onPlay, detailUrl }: { works: Work[]; onPlay: (work: Work) => void; detailUrl: (work: Work) => string } = $props();
  let open = $state(false);
  let filters = $state<SurpriseFilters>({ kind: 'all', genre: '', unseenOnly: false });
  let picked = $state<Work | null>(null);
  const genres = $derived(surpriseGenres(works));
  const candidates = $derived(surpriseCandidates(works, filters, $history));

  function draw() { picked = pickSurprise(candidates, picked?.id); }
  function toggle() { open = !open; if (open) draw(); }
</script>

<section class="surprise">
  <button type="button" class="surprise-toggle" aria-expanded={open} aria-controls="surprise-panel" onclick={toggle}>
    <Icon name="play" size={16} />{$t('surprise_me')}
  </button>
  {#if open}
    <div id="surprise-panel" class="panel">
      <div class="filters">
        <label>{$t('surprise_kind')}
          <select bind:value={filters.kind} onchange={draw}>
            <option value="all">{$t('surprise_kind_all')}</option>
            <option value="film">{$t('surprise_kind_film')}</option>
            <option value="series">{$t('surprise_kind_series')}</option>
          </select>
        </label>
        {#if genres.length}
          <label>{$t('surprise_genre')}
            <select bind:value={filters.genre} onchange={draw}>
              <option value="">{$t('surprise_genre_any')}</option>
              {#each genres as genre (genre)}<option value={genre}>{genre}</option>{/each}
            </select>
          </label>
        {/if}
        <label class="check"><input type="checkbox" bind:checked={filters.unseenOnly} onchange={draw} />{$t('surprise_unseen')}</label>
      </div>
      <div class="result" aria-live="polite">
        {#if picked}
          <!-- detailUrl() already returns a resolved catalogue path. -->
          <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
          <a class="title" href={detailUrl(picked)}>{picked.title}{#if picked.year} <span>({picked.year})</span>{/if}</a>
          <div class="actions">
            <button type="button" class="primary" onclick={() => picked && onPlay(picked)}><Icon name="play" size={14} />{$t('surprise_play')}</button>
            <button type="button" onclick={draw} disabled={candidates.length < 2}>{$t('surprise_again')}</button>
          </div>
        {:else}
          <p>{$t('surprise_none')}</p>
        {/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .surprise { display: flex; flex-direction: column; align-items: flex-start; gap: 12px; }
  button, select { font: inherit; }
  .surprise-toggle, .actions button { display: inline-flex; align-items: center; gap: 8px; height: 40px; padding: 0 16px; border: 1px solid var(--line); border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font-weight: 700; cursor: pointer; }
  .surprise-toggle:hover, .actions button:hover:not(:disabled) { background: var(--pn-2); }
  .actions .primary { border-color: transparent; background: var(--ac); color: var(--on-ac, #fff); }
  .actions button:disabled { opacity: .5; cursor: not-allowed; }
  button:focus-visible, select:focus-visible, input:focus-visible, .title:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }
  .panel { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 16px 24px; width: 100%; padding: 16px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  .filters { display: flex; flex-wrap: wrap; gap: 12px 16px; align-items: flex-end; }
  label { display: flex; flex-direction: column; gap: 6px; color: var(--mu); font-size: 13px; font-weight: 600; }
  label.check { flex-direction: row; align-items: center; min-height: 40px; color: var(--tx); }
  select { height: 40px; padding: 0 10px; border: 1px solid var(--line); border-radius: var(--r-sm); background: var(--pn); color: var(--tx); }
  .result { display: flex; flex-wrap: wrap; align-items: center; gap: 12px 16px; margin-left: auto; }
  .title { color: var(--tx); font-size: 17px; font-weight: 800; text-decoration: none; }
  .title span { color: var(--mu); font-weight: 600; }
  .actions { display: flex; gap: 8px; }
  .result p { margin: 0; color: var(--mu); }
  @media (max-width: 640px) { .result { margin-left: 0; } }
</style>
