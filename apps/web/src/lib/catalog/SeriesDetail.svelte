<script lang="ts">
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import WorkTools from './WorkTools.svelte';
  import ChapterImages from './ChapterImages.svelte';
  import { imagePreviewUrl } from './image-url';
  import { request, type Work, localizedTitle, localizedPlot } from './api';
  import { locale } from '../i18n';
  import PersonalFeedback from './PersonalFeedback.svelte';
  import EditorialCredits from './EditorialCredits.svelte';
  import SimilarWorks from './SimilarWorks.svelte';
  import type { Media } from '../player/types';
  import type ImmersivePlayer from '../player/ImmersivePlayer.svelte';
  import DetailHero from './DetailHero.svelte';
  import { history } from './watch-state';
  import { isResumable } from '../player/progress';
  import { serverPresentation } from '../config/presentation';
  import WatchStatus from './WatchStatus.svelte';
  import EpisodePlayer from './EpisodePlayer.svelte';
  import Icon from '../ui/Icon.svelte';

  let {
    work,
    season = $bindable(1),
    media = [],
    mediaError = '',
    similarCandidates = [],
    immersive,
    launchingEpisodeId = null,
    onLaunchEpisode,
    onSelectEpisode
  }: {
    work: Work;
    season: number;
    media: Media[];
    mediaError?: string;
    similarCandidates?: Work[];
    immersive?: ImmersivePlayer;
    launchingEpisodeId?: string | null;
    onLaunchEpisode: (episode: Work, versionId: string) => Promise<void>;
    onSelectEpisode: (episode: Work) => void;
  } = $props();

  const artwork = $derived(selectArtwork(work, $session));
  let failedImages = $state<string[]>([]);
  const episodePageSize = 24;
  let episodeOffset = $state(0);
  $effect(() => { void work.id; void season; episodeOffset = 0; });
  const activeSeason = $derived(work.seasons?.find(item => item.number === season) ?? work.seasons?.[0]);
  let pageEpisodes = $state<Work[]>([]);
  let episodesLoading = $state(false);
  let episodesError = $state('');
  const episodeCount = $derived(activeSeason?.episodeCount ?? activeSeason?.episodes.length ?? 0);
  let analyzingSeason = $state(false);
  let analyzeFeedback = $state('');

  async function triggerAnalyzeSeason() {
    if (!activeSeason || analyzingSeason) return;
    const account = $session;
    if (!account || account.user.role !== 'administrator') return;
    analyzingSeason = true;
    analyzeFeedback = '';
    try {
      const res = await fetch(`/api/v1/admin/seasons/${encodeURIComponent(activeSeason.id)}/analyze-markers`, {
        method: 'POST',
        headers: { 'x-csrf-token': account.csrfToken }
      });
      if (res.ok) {
        const body = await res.json().catch(() => ({}));
        analyzeFeedback = body.message || 'Analyse des marqueurs lancée.';
      } else {
        analyzeFeedback = `Erreur (${res.status})`;
      }
    } catch {
      analyzeFeedback = 'Analyse indisponible.';
    } finally {
      analyzingSeason = false;
    }
  }
  function seasonLabel(item?: { number: number; title: string }): string {
    if (!item) return '';
    return item.title || (item.number === 0 ? 'Hors-série' : `Saison ${item.number}`);
  }
  const visibleEpisodes = $derived(work.episodesPage ? pageEpisodes : (activeSeason?.episodes ?? []).slice(episodeOffset, episodeOffset + episodePageSize));
  $effect(() => {
    const currentWork = work;
    const selectedSeason = season;
    const offset = episodeOffset;
    if (!currentWork.episodesPage) return;
    const controller = new AbortController();
    episodesError = '';
    if (currentWork.episodesPage.season === selectedSeason && currentWork.episodesPage.offset === offset) {
      pageEpisodes = currentWork.seasons?.find(item => item.number === selectedSeason)?.episodes ?? [];
      episodesLoading = false;
    } else {
      pageEpisodes = [];
      episodesLoading = true;
      void request<Work>(`/api/v1/catalog/${encodeURIComponent(currentWork.id)}?` + new URLSearchParams({ season: String(selectedSeason), episodeOffset: String(offset), episodeLimit: String(episodePageSize) }), controller.signal)
        .then(result => { if (!controller.signal.aborted) pageEpisodes = result.seasons?.find(item => item.number === selectedSeason)?.episodes ?? []; })
        .catch(cause => { if (!controller.signal.aborted) episodesError = cause instanceof Error ? cause.message : 'Épisodes indisponibles.'; })
        .finally(() => { if (!controller.signal.aborted) episodesLoading = false; });
    }
    return () => controller.abort();
  });
</script>
<div class="series-detail-view">
  <DetailHero {work}>
    <div class="series-hero-actions">
      <button type="button" class="hero-cta" onclick={() => {
        const heading = document.getElementById('episodes-shelf-title');
        heading?.focus({ preventScroll: true });
        heading?.scrollIntoView({ behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth', block: 'start' });
      }}><Icon name="play" size={20} />Voir les épisodes</button>
      {#if !work.synthetic}
        <WorkTools {work} />
      {/if}
    </div>
    {#if mediaError}<p class="error-banner" role="alert">{mediaError}</p>{/if}
  </DetailHero>

  <section class="series-shelf" aria-labelledby="episodes-shelf-title">
    <div class="section-heading">
      <h2 id="episodes-shelf-title" tabindex="-1">Épisodes</h2>
      {#if work.seasons?.length}
        <div class="season-heading">
          <span class="episode-count">{seasonLabel(activeSeason)} · {episodeCount} {episodeCount > 1 ? 'épisodes' : 'épisode'}</span>
          {#if $session?.user.role === 'administrator' && activeSeason}
            <button type="button" class="analyze-markers-btn" disabled={analyzingSeason} onclick={triggerAnalyzeSeason} title="Analyser les intros et génériques de cette saison">
              <Icon name="search" size={15} />
              <span>{analyzingSeason ? 'Analyse...' : 'Analyser intros & génériques'}</span>
            </button>
            {#if analyzeFeedback}<span class="analyze-feedback" role="status">{analyzeFeedback}</span>{/if}
          {/if}
        </div>
      {/if}
    </div>
    {#if work.seasons?.length}
      <!-- seasonNN-poster.jpg : chaque saison est choisie par son affiche (repli : affiche de la série) -->
      <div class="season-cards" role="tablist" aria-label="Saisons">
        {#each work.seasons as item (item.id)}
          {@const seasonImage = imagePreviewUrl(artwork[`season${item.number}`] || artwork.poster, 320)}
          {@const count = item.episodeCount ?? item.episodes.length}
          <button type="button" role="tab" class="season-card" aria-selected={item.number === activeSeason?.number} onclick={() => season = item.number}>
            <span class="season-art">
              {#if seasonImage && !failedImages.includes(seasonImage)}
                <img src={seasonImage} alt="" width="320" height="480" loading="lazy" decoding="async" onerror={() => failedImages = [...failedImages, seasonImage]} />
              {:else}
                <span class="season-fallback" aria-hidden="true">{item.number === 0 ? 'HS' : item.number}</span>
              {/if}
            </span>
            <span class="season-name">{seasonLabel(item)}</span>
            {#if count}<span class="season-sub">{count} {count > 1 ? 'épisodes' : 'épisode'}</span>{/if}
          </button>
        {/each}
      </div>
    {/if}
    {#if episodesLoading}<p role="status">Chargement des épisodes…</p>{/if}
    {#if episodesError}<p class="error-banner" role="alert">{episodesError}</p><button class="retry" onclick={() => { episodeOffset = 0; season = work.seasons?.[0]?.number ?? 1; }}>Revenir à la première saison</button>{/if}
    <div class="episodes-list" aria-busy={episodesLoading}>
      {#each visibleEpisodes as episode (episode.id)}
        {@const epTitle = localizedTitle(episode, $locale)}
        {@const epDesc = localizedPlot(episode, $locale)}
        {@const episodeArtwork = selectArtwork(episode, $session)}
        {@const thumbnail = imagePreviewUrl(episodeArtwork.thumb || episodeArtwork.landscape || episodeArtwork.backdrop, 640)}
        {@const progress = $history.find(item => item.workId === episode.id)}
        {@const descriptor = media.find(item => item.workId === episode.id)}
        {@const duration = descriptor?.sources[0]?.durationSeconds}
        <article class="episode" aria-label={`Épisode ${episode.number ?? ''} — ${epTitle}`}>
          <div class="episode-artwork">
            {#if thumbnail && !failedImages.includes(thumbnail)}<img class="episode-thumb" src={thumbnail} alt="" width="640" height="360" loading="lazy" decoding="async" onerror={() => failedImages = [...failedImages, thumbnail]} />{:else}<Icon name="tv" size={32} />{/if}
            {#if progress && isResumable(progress, $serverPresentation) && progress.duration > 0}<progress value={progress.position} max={progress.duration} aria-label={`Progression de ${epTitle}`}></progress>{/if}
          </div>
          <div class="episode-content">
            <p class="episode-meta">Épisode {episode.number ?? '—'}{duration && duration > 0 ? ` · ${Math.ceil(duration / 60)} min` : ''}{progress?.watched ? ' · Vu' : progress && isResumable(progress, $serverPresentation) ? ' · En cours' : ''}</p>
            <h3>{epTitle}</h3>
            {#if epDesc}<p class="episode-desc">{epDesc}</p>{/if}
            <div class="episode-actions-row">
              <EpisodePlayer {episode} {media} {immersive} isLaunching={launchingEpisodeId === episode.id} onlaunch={onLaunchEpisode} onselect={() => onSelectEpisode(episode)} />
              {#if !episode.synthetic}<WatchStatus workId={episode.id} {duration} />{/if}
              {#if !episode.synthetic && $session?.user.role === 'administrator'}<WorkTools work={episode} selectedMedia={descriptor} />{/if}
              {#if descriptor && !episode.synthetic}<ChapterImages assetId={descriptor.id} />{/if}
            </div>
          </div>
        </article>
      {:else}{#if !episodesLoading && !episodesError}<p class="empty-episodes-box">Aucun épisode disponible pour cette saison.</p>{/if}{/each}
    </div>
    {#if episodeCount > episodePageSize}
      <nav class="episode-pagination" aria-label="Pagination des épisodes">
        <button disabled={episodesLoading || episodeOffset === 0} onclick={() => { episodeOffset = Math.max(0, episodeOffset - episodePageSize); document.getElementById('episodes-shelf-title')?.scrollIntoView({ block: 'start' }); }}>Précédents</button>
        <span role="status">{episodeOffset + 1}–{Math.min(episodeOffset + episodePageSize, episodeCount)} sur {episodeCount}</span>
        <button disabled={episodesLoading || episodeOffset + episodePageSize >= episodeCount} onclick={() => { episodeOffset += episodePageSize; document.getElementById('episodes-shelf-title')?.scrollIntoView({ block: 'start' }); }}>Suivants</button>
      </nav>
    {/if}
  </section>

  {#if !work.synthetic}
    <details class="personal-panel">
      <summary>Mes préférences pour cette série</summary>
      <WatchStatus workId={work.id} />
      <PersonalFeedback workId={work.id} />
    </details>
  {/if}

  <EditorialCredits {work} />
  <SimilarWorks {work} candidates={similarCandidates} />
</div>

<style>
  .series-detail-view { padding-bottom: 40px; }
  .series-hero-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; width: 100%; }
  .hero-cta { display: inline-flex; align-items: center; justify-content: center; gap: 10px; height: 56px; padding: 0 26px; border: 0; border-radius: 18px; background: var(--btn); color: var(--btn-tx); font-size: 15px; font-weight: 700; transition: transform var(--t-fast) var(--ease), opacity var(--t-fast) ease; }
  .hero-cta:hover:not(:disabled) { opacity: 0.9; }
  .hero-cta:active { transform: scale(0.98); }
  .series-shelf { margin-top: 40px; }
  .section-heading { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 16px; margin-bottom: 16px; }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; color: var(--tx); }
  #episodes-shelf-title { scroll-margin-top: 96px; }
  .season-heading { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; min-width: 0; }
  .episode-count { font-size: 14px; font-weight: 600; color: var(--mu); white-space: nowrap; }
  .season-cards { display: grid; grid-auto-flow: column; grid-auto-columns: 150px; gap: 14px; overflow-x: auto; scrollbar-width: none; padding: 4px 3px 12px; margin-bottom: 8px; }
  .season-card { display: flex; flex-direction: column; gap: 4px; padding: 0; border: 0; background: none; color: var(--tx); text-align: left; }
  .season-art { position: relative; display: grid; place-items: center; aspect-ratio: 2 / 3; overflow: hidden; border-radius: var(--r-md); background: var(--pn-2); margin-bottom: 6px; transition: transform var(--t-med) var(--ease), box-shadow var(--t-med) ease; }
  .season-art img { width: 100%; height: 100%; object-fit: cover; }
  .season-fallback { font-size: 40px; font-weight: 800; color: var(--mu); }
  .season-card:hover .season-art { transform: translateY(-3px); }
  .season-card[aria-selected='true'] .season-art { box-shadow: 0 0 0 3px var(--ac); }
  .season-name { font-size: 14px; font-weight: 800; }
  .season-sub { font-size: 12px; color: var(--mu); }
  .analyze-markers-btn { display: inline-flex; align-items: center; gap: 6px; height: 36px; padding: 0 14px; font-size: 13px; font-weight: 700; border-radius: var(--r-sm); background: var(--pn-2); color: var(--tx); border: 0; cursor: pointer; transition: background-color var(--t-fast) ease; }
  .analyze-markers-btn:hover:not(:disabled) { background: var(--pn-3); }
  .analyze-markers-btn:disabled { opacity: 0.6; cursor: wait; }
  .analyze-feedback { font-size: 13px; color: var(--ac); font-weight: 600; }
  .episodes-list { display: grid; gap: 4px; }
  .episode { display: grid; grid-template-columns: minmax(180px, 240px) minmax(0, 1fr); align-items: start; gap: 20px; padding: 14px 12px; border-radius: var(--r-lg); transition: background-color var(--t-fast) ease; }
  .episode:hover { background: var(--pn); }
  .episode-artwork { aspect-ratio: 16 / 9; position: relative; display: grid; place-items: center; overflow: hidden; border-radius: var(--r-sm); background: var(--pn-2); color: var(--mu); }
  .episode-thumb { display: block; width: 100%; height: 100%; object-fit: cover; }
  .episode-artwork progress { position: absolute; left: 8px; right: 8px; bottom: 8px; width: calc(100% - 16px); height: 4px; border: 0; border-radius: 4px; accent-color: var(--ac); }
  .episode-content { min-width: 0; display: flex; flex-direction: column; align-items: start; gap: 8px; overflow-wrap: anywhere; }
  .episode-meta { margin: 0; font-size: 13px; font-weight: 600; color: var(--mu); }
  h3 { font-size: 16px; line-height: 1.35; margin: 0; font-weight: 800; color: var(--tx); }
  .episode-desc { margin: 0; font-size: 14px; line-height: 1.6; color: var(--mu); max-width: 90ch; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
  .episode-actions-row { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; margin-top: 2px; }
  .episode-pagination { display: flex; align-items: center; justify-content: center; flex-wrap: wrap; gap: 16px; margin-top: 24px; font-size: 14px; }
  .episode-pagination button, .retry { min-height: 44px; padding: 0 16px; background: var(--pn-2); color: var(--tx); border: 0; border-radius: var(--r-sm); font-weight: 700; }
  .episode-pagination button:hover:not(:disabled), .retry:hover:not(:disabled) { background: var(--pn-3); }
  .error-banner { color: var(--err); line-height: 1.6; }
  .empty-episodes-box { padding: 36px 24px; border-radius: var(--r-lg); background: var(--bg-2); color: var(--mu); text-align: center; }
  @media (max-width: 760px) { .episode { grid-template-columns: 128px minmax(0, 1fr); gap: 12px; padding: 10px 4px; } .episode-actions-row { grid-column: 1 / -1; } .season-cards { grid-auto-columns: 112px; } .hero-cta { width: 100%; height: 50px; } }

  .personal-panel { border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); margin-top: 24px; padding: 4px 20px; }
  .personal-panel summary { min-height: 52px; padding-block: 14px; cursor: pointer; font-weight: 700; color: var(--tx); }
</style>
