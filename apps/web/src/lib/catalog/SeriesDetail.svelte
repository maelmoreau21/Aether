<script lang="ts">
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import WorkTools from './WorkTools.svelte';
  import ChapterImages from './ChapterImages.svelte';
  import { imagePreviewUrl } from './image-url';
  import { request, type Work, localizedTitle, localizedPlot } from './api';
  import { locale, t } from '../i18n';
  import PersonalFeedback from './PersonalFeedback.svelte';
  import EditorialCredits from './EditorialCredits.svelte';
  import SimilarWorks from './SimilarWorks.svelte';
  import type { Media } from '../player/types';
  import type ImmersivePlayer from '../player/ImmersivePlayer.svelte';
  import DetailHero from './DetailHero.svelte';
  import DetailTabs from './DetailTabs.svelte';
  import WorkDetails from './WorkDetails.svelte';
  import { creditPeople } from './credits';
  import { resumeTarget } from './detail-facts';
  import { history } from './watch-state';
  import { isResumable } from '../player/progress';
  import { serverPresentation } from '../config/presentation';
  import WatchStatus from './WatchStatus.svelte';
  import EpisodePlayer from './EpisodePlayer.svelte';
  import Icon from '../ui/Icon.svelte';
  import SegmentEditor from './SegmentEditor.svelte';
  import { fitArtwork } from '../ui/fit-artwork';

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
        analyzeFeedback = body.message || $t('marker_analysis_started');
      } else {
        analyzeFeedback = $t('catalog_error', { status: res.status });
      }
    } catch {
      analyzeFeedback = $t('analysis_unavailable');
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
        .catch(cause => { if (!controller.signal.aborted) episodesError = cause instanceof Error ? cause.message : $t('episodes_unavailable'); })
        .finally(() => { if (!controller.signal.aborted) episodesLoading = false; });
    }
    return () => controller.abort();
  });

  // Onglets sous l'en-tête : Épisodes (par défaut), Détails, Distribution.
  const hasCast = $derived(creditPeople(work).length > 0);
  const tabs = $derived([
    { id: 'episodes', label: $t('episodes') },
    { id: 'details', label: $t('detail_tab_details') },
    ...(hasCast ? [{ id: 'cast', label: $t('cast') }] : []),
    ...($session?.user.role === 'administrator' && !work.synthetic && work.seasons?.length ? [{ id: 'segments', label: $t('segments_tab') }] : [])
  ]);
  let tab = $state('episodes');
  $effect(() => { void work.id; tab = 'episodes'; });

  // « Reprendre S2 · É4 » : l'épisode en cours le plus récent de cette série.
  const resume = $derived(work.synthetic ? null : resumeTarget(work, $history, item => isResumable(item, $serverPresentation), pageEpisodes, season));
  const resumeRatio = $derived(resume && resume.progress.duration > 0 ? Math.min(100, (resume.progress.position / resume.progress.duration) * 100) : 0);

  function scrollToSections() {
    const sections = document.getElementById('series-sections');
    sections?.scrollIntoView({ behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth', block: 'start' });
  }

  function showEpisodes() {
    tab = 'episodes';
    scrollToSections();
    document.getElementById('series-detail-tab-episodes')?.focus({ preventScroll: true });
  }
</script>
<div class="series-detail-view">
  <DetailHero {work}>
    <div class="series-hero-actions">
      {#if resume}
        {@const episodeTitle = localizedTitle(resume.episode, $locale)}
        <button
          type="button"
          class="hero-cta resume"
          disabled={launchingEpisodeId === resume.episode.id}
          aria-label={$t('detail_resume_episode_label', { title: episodeTitle, season: resume.season, episode: resume.episode.number ?? '' })}
          onclick={() => onLaunchEpisode(resume.episode, 'auto')}
        >
          <Icon name="play" size={20} />{$t('detail_resume_episode', { season: resume.season, episode: resume.episode.number ?? '' })}
          <span class="cta-progress" aria-hidden="true" style:width="{resumeRatio}%"></span>
        </button>
        <button type="button" class="secondary-cta" onclick={showEpisodes}>{$t('view_episodes')}</button>
      {:else}
        <button type="button" class="hero-cta" onclick={showEpisodes}><Icon name="play" size={20} />{$t('view_episodes')}</button>
      {/if}
      {#if !work.synthetic}
        <div class="personal-actions" aria-label={$t('my_tracking')}>
          <WatchStatus workId={work.id} compact />
          <PersonalFeedback workId={work.id} compact />
        </div>
        <span class="action-divider" aria-hidden="true"></span>
        <WorkTools {work} />
      {/if}
    </div>
    {#if mediaError}<p class="error-banner" role="alert">{mediaError}</p>{/if}
  </DetailHero>

  <section class="detail-sections" id="series-sections" aria-label={$t('detail_sections')}>
  <DetailTabs {tabs} bind:active={tab} label={$t('detail_sections')} id="series-detail" />
  {#if tab === 'details'}
    <div class="panel" role="tabpanel" id="series-detail-panel-details" aria-labelledby="series-detail-tab-details"><WorkDetails {work} /></div>
  {:else if tab === 'segments' && activeSeason && $session?.user.role === 'administrator'}
    <div class="panel" role="tabpanel" id="series-detail-panel-segments" aria-labelledby="series-detail-tab-segments"><SegmentEditor seasonId={activeSeason.id} seasonTitle={seasonLabel(activeSeason)} /></div>
  {:else if tab === 'cast' && hasCast}
    <div class="panel" role="tabpanel" id="series-detail-panel-cast" aria-labelledby="series-detail-tab-cast"><EditorialCredits {work} /></div>
  {:else}
  <div class="panel series-shelf" role="tabpanel" id="series-detail-panel-episodes" aria-labelledby="series-detail-tab-episodes">
    <div class="section-heading">
      <h2 id="episodes-shelf-title" class="h-sr" tabindex="-1">{$t('request_episodes')}</h2>
      {#if work.seasons?.length}
        <div class="season-heading">
          <span class="episode-count">{seasonLabel(activeSeason)} · {episodeCount} {episodeCount > 1 ? 'épisodes' : 'épisode'}</span>
          {#if $session?.user.role === 'administrator' && activeSeason}
            <button type="button" class="analyze-markers-btn" disabled={analyzingSeason} onclick={triggerAnalyzeSeason} title={$t('analyse_the_intros_and_credits_of')}>
              <Icon name="search" size={15} />
              <span>{analyzingSeason ? 'Analyse...' : $t('analyse_intros_credits')}</span>
            </button>
            {#if analyzeFeedback}<span class="analyze-feedback" role="status">{analyzeFeedback}</span>{/if}
          {/if}
        </div>
      {/if}
    </div>
    {#if work.seasons?.length}
      <!-- seasonNN-poster.jpg : chaque saison est choisie par son affiche (repli : affiche de la série) -->
      <div class="season-cards" role="tablist" aria-label={$t('catalog_seasons')}>
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
    {#if episodesLoading}<p role="status">{$t('loading_episodes')}</p>{/if}
    {#if episodesError}<p class="error-banner" role="alert">{episodesError}</p><button class="retry" onclick={() => { episodeOffset = 0; season = work.seasons?.[0]?.number ?? 1; }}>{$t('back_to_the_first_season')}</button>{/if}
    <div class="episodes-list" aria-busy={episodesLoading}>
      {#each visibleEpisodes as episode (episode.id)}
        {@const epTitle = localizedTitle(episode, $locale)}
        {@const epDesc = localizedPlot(episode, $locale)}
        {@const episodeArtwork = selectArtwork(episode, $session)}
        {@const thumbnail = imagePreviewUrl(episodeArtwork.thumb || episodeArtwork.landscape || episodeArtwork.backdrop || artwork.landscape || artwork.backdrop, 640)}
        {@const progress = $history.find(item => item.workId === episode.id)}
        {@const descriptor = media.find(item => item.workId === episode.id)}
        {@const duration = descriptor?.sources[0]?.durationSeconds}
        <article class="episode" aria-label={$t('episode_aria', { number: episode.number ?? '', title: epTitle })}>
          <div class="episode-artwork">
            {#if thumbnail && !failedImages.includes(thumbnail)}<img class="episode-thumb" src={thumbnail} alt="" width="640" height="360" loading="lazy" decoding="async" use:fitArtwork onerror={() => failedImages = [...failedImages, thumbnail]} />{:else}<Icon name="tv" size={32} />{/if}
            {#if progress && isResumable(progress, $serverPresentation) && progress.duration > 0}<progress value={progress.position} max={progress.duration} aria-label={$t('progress_of_3', { epTitle })}></progress>{/if}
          </div>
          <div class="episode-content">
            <p class="episode-meta">{$t('episode_meta', { number: episode.number ?? '—', duration: duration && duration > 0 ? ` · ${$t('minutes_short', { count: Math.ceil(duration / 60) })}` : '', state: progress?.watched ? ` · ${$t('seen_short')}` : progress && isResumable(progress, $serverPresentation) ? ` · ${$t('stats_kpi_active')}` : '' })}</p>
            <h3>{epTitle}</h3>
            {#if epDesc}<p class="episode-desc">{epDesc}</p>{/if}
            <div class="episode-actions-row">
              <EpisodePlayer {episode} {media} {immersive} isLaunching={launchingEpisodeId === episode.id} onlaunch={onLaunchEpisode} onselect={() => onSelectEpisode(episode)} />
              {#if !episode.synthetic}<WatchStatus workId={episode.id} {duration} compact />{/if}
              {#if !episode.synthetic && $session?.user.role === 'administrator'}<WorkTools work={episode} selectedMedia={descriptor} />{/if}
              {#if descriptor && !episode.synthetic}<ChapterImages assetId={descriptor.id} />{/if}
            </div>
          </div>
        </article>
      {:else}{#if !episodesLoading && !episodesError}<p class="empty-episodes-box">{$t('no_episodes_available_for_this_season')}</p>{/if}{/each}
    </div>
    {#if episodeCount > episodePageSize}
      <nav class="episode-pagination" aria-label={$t('episode_pagination')}>
        <button disabled={episodesLoading || episodeOffset === 0} onclick={() => { episodeOffset = Math.max(0, episodeOffset - episodePageSize); document.getElementById('series-sections')?.scrollIntoView({ block: 'start' }); }}>{$t('previous_page')}</button>
        <span role="status">{$t('range_of_total', { from: episodeOffset + 1, to: Math.min(episodeOffset + episodePageSize, episodeCount), total: episodeCount })}</span>
        <button disabled={episodesLoading || episodeOffset + episodePageSize >= episodeCount} onclick={() => { episodeOffset += episodePageSize; document.getElementById('series-sections')?.scrollIntoView({ block: 'start' }); }}>{$t('next_page')}</button>
      </nav>
    {/if}
  </div>
  {/if}
  </section>

  <SimilarWorks {work} candidates={similarCandidates} />
</div>

<style>
  .series-detail-view { padding-bottom: 40px; }
  .series-hero-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; width: 100%; margin-top: 8px; }
  .personal-actions { display: inline-flex; flex-wrap: wrap; align-items: center; gap: 10px; }
  .action-divider { width: 1px; height: 28px; background: var(--line-2); }
  @media (max-width: 760px) { .action-divider { display: none; } }
  .hero-cta { display: inline-flex; align-items: center; justify-content: center; gap: 10px; height: 56px; padding: 0 26px; border: 0; border-radius: 18px; background: var(--btn); color: var(--btn-tx); font-size: 15px; font-weight: 700; transition: transform var(--t-fast) var(--ease), opacity var(--t-fast) ease; }
  .hero-cta:hover:not(:disabled) { opacity: 0.9; }
  .hero-cta:active { transform: scale(0.98); }
  .hero-cta:disabled { opacity: 0.7; cursor: wait; }
  .hero-cta.resume { position: relative; overflow: hidden; }
  .cta-progress { position: absolute; left: 0; bottom: 0; height: 4px; background: var(--ac); }
  .secondary-cta { display: inline-flex; align-items: center; height: 48px; padding: 0 18px; border: 1px solid var(--line-2); border-radius: var(--r-pill); background: var(--pn-2); color: inherit; font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease; }
  .secondary-cta:hover { background: var(--pn-3); }
  .detail-sections { display: flex; flex-direction: column; gap: 24px; margin-top: 28px; scroll-margin-top: 88px; }
  .panel { min-width: 0; }
  .section-heading { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 16px; margin-bottom: 16px; }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; color: var(--tx); }
  .season-heading { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; min-width: 0; }
  .episode-count { font-size: 14px; font-weight: 600; color: var(--mu); white-space: nowrap; }
  /* Saisons : pastilles compactes avec leur affiche (seasonNN-poster.jpg). */
  .season-cards { display: flex; gap: 12px; overflow-x: auto; scrollbar-width: none; padding: 4px 3px 12px; margin-bottom: 8px; }
  .season-card { flex: 0 0 auto; display: grid; grid-template-columns: 44px auto; grid-template-rows: auto auto; column-gap: 12px; align-items: center; align-content: center; padding: 6px 16px 6px 6px; border: 1px solid transparent; border-radius: var(--r-md); background: var(--pn); color: var(--tx); text-align: left; cursor: pointer; transition: background-color var(--t-fast) ease, border-color var(--t-fast) ease; }
  .season-card:hover { background: var(--pn-2); }
  .season-card[aria-selected='true'] { border-color: var(--ac); background: var(--ac-soft); }
  .season-art { grid-row: 1 / 3; position: relative; display: grid; place-items: center; width: 44px; aspect-ratio: 2 / 3; overflow: hidden; border-radius: var(--r-xs); background: var(--pn-2); }
  .season-art img { width: 100%; height: 100%; object-fit: cover; }
  .season-fallback { font-size: 16px; font-weight: 800; color: var(--mu); }
  .season-name { font-size: 14px; font-weight: 800; white-space: nowrap; }
  .season-sub { font-size: 12px; color: var(--mu); white-space: nowrap; }
  .analyze-markers-btn { display: inline-flex; align-items: center; gap: 6px; height: 36px; padding: 0 14px; font-size: 13px; font-weight: 700; border-radius: var(--r-sm); background: var(--pn-2); color: var(--tx); border: 0; cursor: pointer; transition: background-color var(--t-fast) ease; }
  .analyze-markers-btn:hover:not(:disabled) { background: var(--pn-3); }
  .analyze-markers-btn:disabled { opacity: 0.6; cursor: wait; }
  .analyze-feedback { font-size: 13px; color: var(--ac); font-weight: 600; }
  .episodes-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 560px), 1fr)); gap: 4px 24px; }
  .episode { display: grid; grid-template-columns: minmax(160px, 220px) minmax(0, 1fr); align-items: start; gap: 20px; padding: 14px 12px; border-radius: var(--r-lg); transition: background-color var(--t-fast) ease; }
  .episode:hover { background: var(--pn); }
  .episode-artwork { aspect-ratio: var(--artwork-ratio, 16 / 9); position: relative; display: grid; place-items: center; overflow: hidden; border-radius: var(--r-sm); background: var(--pn-2); color: var(--mu); }
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
  @media (max-width: 760px) { .episode { grid-template-columns: 128px minmax(0, 1fr); gap: 12px; padding: 10px 4px; } .episode-actions-row { grid-column: 1 / -1; } .hero-cta { width: 100%; height: 50px; } .secondary-cta { width: 100%; justify-content: center; } .detail-sections { margin-top: 20px; gap: 18px; } }

</style>
