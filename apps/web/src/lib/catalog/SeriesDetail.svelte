<script lang="ts">
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import WorkTools from './WorkTools.svelte';
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
        {@const seasonImage = imagePreviewUrl(artwork[`season${activeSeason?.number}`], 320)}
        <div class="season-heading">
          {#if seasonImage && !failedImages.includes(seasonImage)}<img class="season-image" src={seasonImage} alt="" width="48" height="72" loading="lazy" onerror={() => failedImages = [...failedImages, seasonImage]} />{/if}
          <label class="season-selector-label"><span>Saison</span><select aria-label="Saison" bind:value={season}>{#each work.seasons as item (item.id)}<option value={item.number}>{item.title || (item.number === 0 ? 'Hors-série' : `Saison ${item.number}`)}</option>{/each}</select></label>
          <span class="episode-count">{episodeCount} {episodeCount > 1 ? 'épisodes' : 'épisode'}</span>
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
  .series-hero-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; width: 100%; margin: 4px 0 6px; }
  .hero-cta { display: inline-flex; align-items: center; justify-content: center; gap: 10px; min-height: 48px; padding: 12px 28px; border: 0; border-radius: var(--radius-pill); background: var(--color-play); color: var(--color-on-play); font-size: 15px; font-weight: 700; box-shadow: 0 6px 20px rgba(249, 115, 22, 0.36); transition: transform var(--transition-fast), box-shadow var(--transition-fast), background var(--transition-fast); }
  .hero-cta:hover:not(:disabled) { background: var(--color-play-hover, var(--color-play)); transform: translateY(-2px); box-shadow: 0 10px 26px rgba(249, 115, 22, 0.5); }
  .series-shelf { margin-top: 32px; }
  .section-heading { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 16px; margin-bottom: 24px; }
  h2 { margin: 0; font-family: var(--font-display); font-size: 26px; font-weight: 700; letter-spacing: -0.03em; color: var(--color-text-bright); }
  #episodes-shelf-title { scroll-margin-top: 96px; }
  .season-heading { display: flex; align-items: center; gap: 16px; min-width: 0; }
  .season-image { width: 48px; height: 72px; object-fit: cover; border-radius: var(--radius-xs); border: 1px solid var(--color-line); }
  .season-selector-label { display: grid; gap: 6px; min-width: 0; color: var(--color-muted); font-size: 12px; font-weight: 600; }
  .season-selector-label select { min-height: 44px; width: 100%; max-width: 360px; font-size: 15px; }
  .episode-count { font-size: 13px; color: var(--color-muted); white-space: nowrap; padding: 4px 10px; border-radius: var(--radius-pill); background: var(--color-raised); border: 1px solid var(--color-line); }
  .analyze-markers-btn { display: inline-flex; align-items: center; gap: 6px; padding: 6px 12px; font-size: 12px; font-weight: 600; border-radius: var(--radius-pill); background: var(--color-raised); color: var(--color-text); border: 1px solid var(--color-line); cursor: pointer; transition: background var(--transition-fast), border-color var(--transition-fast); }
  .analyze-markers-btn:hover:not(:disabled) { background: var(--color-surface-hover); border-color: var(--color-line-strong); color: var(--color-text-bright); }
  .analyze-markers-btn:disabled { opacity: 0.6; cursor: wait; }
  .analyze-feedback { font-size: 12px; color: var(--color-accent); font-weight: 500; }
  .episodes-list { display: grid; gap: 12px; }
  .episode { display: grid; grid-template-columns: minmax(180px, 260px) minmax(0, 1fr); align-items: start; gap: 24px; padding: 18px; border-radius: var(--radius-lg); background: var(--color-surface); border: 1px solid var(--color-line); transition: border-color var(--transition-fast), background var(--transition-fast), box-shadow var(--transition-fast); }
  .episode:hover { border-color: var(--color-line-strong); background: var(--color-surface-hover); box-shadow: var(--shadow-sm); }
  .episode-artwork { aspect-ratio: 16 / 9; position: relative; display: grid; place-items: center; overflow: hidden; border-radius: var(--radius-md); background: var(--color-raised); border: 1px solid var(--color-line); color: var(--color-muted); }
  .episode-thumb { display: block; width: 100%; height: 100%; object-fit: cover; }
  .episode-artwork progress { position: absolute; bottom: 0; width: 100%; height: 4px; border: 0; accent-color: var(--color-play); }
  .episode-content { min-width: 0; display: flex; flex-direction: column; align-items: start; gap: 10px; overflow-wrap: anywhere; }
  .episode-meta { margin: 0; font-size: 12px; font-weight: 650; color: var(--color-accent); letter-spacing: 0.04em; text-transform: uppercase; }
  h3 { font-size: 18px; line-height: 1.4; margin: 0; font-weight: 650; color: var(--color-text-bright); }
  .episode-desc { margin: 0; font-size: 14px; line-height: 1.65; color: var(--color-text); max-width: 95ch; }
  .episode-actions-row { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; margin-top: 4px; }
  .episode-pagination { display: flex; align-items: center; justify-content: center; flex-wrap: wrap; gap: 16px; margin-top: 24px; font-size: 14px; }
  .episode-pagination button, .retry { min-height: 44px; padding: 10px 16px; background: var(--color-surface); color: var(--color-text); border: 1px solid var(--color-line); border-radius: var(--radius-sm); font-weight: 600; }
  .episode-pagination button:hover:not(:disabled), .retry:hover:not(:disabled) { background: var(--color-raised); border-color: var(--color-line-strong); }
  .error-banner { color: var(--color-danger); line-height: 1.6; }
  .empty-episodes-box { padding: 36px 24px; border-radius: var(--radius-lg); background: var(--color-surface); border: 1px dashed var(--color-line); color: var(--color-muted); text-align: center; }
  @media (max-width: 640px) { .episode { grid-template-columns: minmax(0, 1fr); gap: 16px; padding: 14px; } .season-heading { flex-wrap: wrap; width: 100%; gap: 12px; } .season-selector-label { flex: 1; } .hero-cta { width: 100%; } }

  .personal-panel { border: 1px solid var(--color-line); border-radius: var(--radius-lg); background: var(--color-surface); margin-top: 24px; padding: 4px 18px; }
  .personal-panel summary { min-height: 44px; padding-block: 12px; cursor: pointer; font-weight: 650; color: var(--color-text-bright); }
</style>
