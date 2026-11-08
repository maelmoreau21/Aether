<script lang="ts">
  import WorkTools from './WorkTools.svelte';
  import { bestVersion } from '../player/version-choice';
  import { preloadProgress, isResumable } from '../player/progress';
  import { serverPresentation } from '../config/presentation';
  import { history } from './watch-state';
  import type { Work } from './api';
  import PersonalFeedback from './PersonalFeedback.svelte';
  import EditorialCredits from './EditorialCredits.svelte';
  import SimilarWorks from './SimilarWorks.svelte';
  import DetailHero from './DetailHero.svelte';
  import DetailTabs from './DetailTabs.svelte';
  import WorkDetails from './WorkDetails.svelte';
  import { creditPeople } from './credits';
  import type { Media } from '../player/types';
  import { sourceLabel } from './api';
  import { videoResolutionBadge, colorProfileLabel, inspectedAudioTracks, audioChannelBadge } from '../player/track-labels';
  import WatchStatus from './WatchStatus.svelte';
  import Icon from '../ui/Icon.svelte';
  import LanguageLabel from '../ui/LanguageLabel.svelte';
  import ChapterImages from './ChapterImages.svelte';
  import MediaSegments from './MediaSegments.svelte';
  import { session } from '../auth/session';
  import { t } from '../i18n';

  let { work, playableMedia = [], allowedIds = [], selected = $bindable(''), mediaError = '', similarCandidates = [], onPlay }: {
    work: Work; playableMedia: Media[]; allowedIds: string[]; selected: string; mediaError?: string; similarCandidates?: Work[]; onPlay: () => Promise<void>;
  } = $props();
  const currentSelectedMedia = $derived(playableMedia.find(item => item.id === selected) ?? playableMedia[0]);
  const continuing = $derived($history.some(item => item.workId === work.id && isResumable(item, $serverPresentation)));
  const currentVideoTrack = $derived(currentSelectedMedia?.sources[0]?.tracks?.find(track => track.codec_type === 'video'));
  const currentAudioTracks = $derived(inspectedAudioTracks(currentSelectedMedia));
  const resolutionBadge = $derived(videoResolutionBadge(currentVideoTrack?.width, currentVideoTrack?.height));
  const colorBadge = $derived(colorProfileLabel(currentVideoTrack?.pix_fmt));

  const hasCast = $derived(creditPeople(work).length > 0);
  const tabs = $derived([
    { id: 'details', label: $t('detail_tab_details') },
    ...(hasCast ? [{ id: 'cast', label: $t('cast') }] : []),
    // Administrators check and correct the detected intro and credits of the selected version.
    ...($session?.user.role === 'administrator' && !work.synthetic && currentSelectedMedia ? [{ id: 'segments', label: $t('segments_tab') }] : [])
  ]);
  let tab = $state('details');
  $effect(() => { void work.id; tab = 'details'; });
</script>

{#snippet mediaFacts()}
  {#if currentSelectedMedia}
    <div class="fact">
      <dt>{$t('detail_quality')}</dt>
      <dd class="pills">
        <span class="quality-pill highlight">{resolutionBadge ?? (currentVideoTrack?.height ? `${currentVideoTrack.height}p` : 'HD')}</span>
        {#if colorBadge && colorBadge !== '8 bits standard'}<span class="quality-pill">{colorBadge}</span>{/if}
      </dd>
    </div>
    <div class="fact">
      <dt>{$t('audio')}</dt>
      <dd class="pills">
        {#each currentAudioTracks as track, index (index)}
          {@const ch = audioChannelBadge(track)}
          <span class="lang"><LanguageLabel language={track.language} />{#if ch}<strong class="channel-tag">{ch}</strong>{/if}</span>
        {:else}
          {$t('detail_original_audio')}
        {/each}
      </dd>
    </div>
    <div class="fact">
      <dt>{$t('subtitles')}</dt>
      <dd class="pills">
        {#each currentSelectedMedia.subtitles as subtitle (subtitle.id)}
          <span class="lang"><LanguageLabel language={subtitle.language} />{#if subtitle.forced}<small class="sub-tag">{$t('detail_forced')}</small>{/if}{#if subtitle.sdh}<small class="sub-tag">SDH</small>{/if}</span>
        {:else}
          {$t('detail_no_subtitles')}
        {/each}
      </dd>
    </div>
  {/if}
{/snippet}

<div class="film-detail-view">
  <DetailHero {work} duration={currentSelectedMedia?.sources[0]?.durationSeconds} facts={currentSelectedMedia ? mediaFacts : undefined}>
    <div class="play-action-row">
      <button class="hero-cta" onpointerenter={() => { if (!work.synthetic) preloadProgress(work.id); }} onfocus={() => { if (!work.synthetic) preloadProgress(work.id); }} disabled={work.available === false || (!playableMedia.length && !allowedIds.length)} onclick={onPlay}>
        <Icon name="play" size={20} /><span>{work.available === false ? $t('temporarily_unavailable') : continuing ? $t('resume_the_film') : $t('play_the_film')}</span>
      </button>

      {#if !work.synthetic}
        <div class="tools-inline personal-actions" aria-label={$t('my_tracking')}>
          <WatchStatus workId={work.id} duration={currentSelectedMedia?.sources[0]?.durationSeconds} compact />
          <PersonalFeedback workId={work.id} compact />
        </div>
        <span class="action-divider" aria-hidden="true"></span>
        <!-- Offline downloads are offered by the native clients only. -->
        <div class="tools-inline">
          <WorkTools {work} selectedMedia={currentSelectedMedia} />
        </div>
      {/if}
    </div>

    {#if playableMedia.length > 1}
      <div class="versions-selector" role="group" aria-label={$t('detail_choose_quality')}>
        <span class="exp-label">{$t('detail_version')}</span>
        <button type="button" class="version-chip" onclick={() => { const probe = document.createElement('video'); selected = bestVersion(playableMedia, mime => probe.canPlayType(mime))?.id ?? selected; }}>{$t('detail_auto_best')}</button>
        {#each playableMedia as item (item.id)}
          {@const vTrack = item.sources[0]?.tracks?.find(t => t.codec_type === 'video')}
          {@const res = videoResolutionBadge(vTrack?.width, vTrack?.height)}
          <button type="button" class="version-chip" aria-pressed={selected === item.id} aria-label={$t('select_version', { value: sourceLabel(item) })} onclick={() => selected = item.id}>
            {res ? `${res} (${sourceLabel(item)})` : sourceLabel(item)}
          </button>
        {/each}
      </div>
    {/if}

    {#if !playableMedia.length && !allowedIds.length}<p class="unavailable">{$t('detail_no_playable_version')}</p>{/if}
    {#if mediaError}<p class="error-banner" role="alert">{mediaError}</p>{/if}
  </DetailHero>

  <section class="detail-sections" aria-label={$t('detail_sections')}>
    {#if tabs.length > 1}
      <DetailTabs {tabs} bind:active={tab} label={$t('detail_sections')} id="film-detail" />
    {/if}
    <div
      class="panel"
      role={tabs.length > 1 ? 'tabpanel' : undefined}
      id="film-detail-panel-{tab}"
      aria-labelledby={tabs.length > 1 ? `film-detail-tab-${tab}` : undefined}
    >
      {#if tab === 'cast' && hasCast}
        <EditorialCredits {work} />
      {:else if tab === 'segments' && currentSelectedMedia && $session?.user.role === 'administrator'}
        {#key currentSelectedMedia.id}<MediaSegments assetId={currentSelectedMedia.id} title={work.title} />{/key}
      {:else}
        <WorkDetails {work}>
          {#if currentSelectedMedia && !work.synthetic}{#key currentSelectedMedia.id}<ChapterImages assetId={currentSelectedMedia.id} />{/key}{/if}
        </WorkDetails>
      {/if}
    </div>
  </section>

  <SimilarWorks {work} candidates={similarCandidates} />
</div>

<style>
  .film-detail-view { padding-bottom: 40px; }

  .detail-sections {
    display: flex;
    flex-direction: column;
    gap: 24px;
    margin-top: 28px;
  }

  .panel { min-width: 0; }

  .exp-label {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    opacity: 0.8;
  }

  .pills {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: center;
    gap: 6px 10px;
  }

  .lang {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    white-space: nowrap;
  }

  .quality-pill {
    display: inline-flex;
    align-items: center;
    padding: 2px 8px;
    border-radius: var(--radius-xs);
    font-size: 12px;
    font-weight: 750;
    background: var(--color-raised);
    border: 1px solid var(--color-line-strong);
    color: var(--color-text-bright);
  }

  .quality-pill.highlight {
    background: var(--color-accent-soft);
    border-color: var(--ac);
    color: var(--color-accent);
  }

  .channel-tag {
    display: inline-block;
    padding: 0 5px;
    border-radius: 4px;
    background: var(--ac-soft);
    color: var(--color-accent);
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.02em;
  }

  .sub-tag {
    padding: 0 5px;
    border-radius: 4px;
    background: var(--pn-2);
    color: var(--color-muted);
    font-size: 10.5px;
    font-weight: 600;
  }

  .versions-selector {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .version-chip {
    min-height: 36px;
    padding: 6px 14px;
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-pill);
    background: var(--color-glass-raised);
    color: var(--color-text);
    font-size: 12.5px;
    font-weight: 600;
  }

  .version-chip:hover {
    border-color: var(--color-text);
    color: var(--color-text-bright);
  }

  .version-chip[aria-pressed='true'] {
    border-color: var(--color-accent);
    background: var(--color-accent-soft);
    color: var(--color-text-bright);
  }

  .play-action-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    width: 100%;
    margin-top: 4px;
  }

  .tools-inline {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }

  .action-divider { width: 1px; height: 28px; background: var(--line-2); }

  .hero-cta {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    height: 56px;
    max-width: 100%;
    padding: 0 26px;
    border: 0;
    border-radius: 18px;
    background: var(--btn);
    color: var(--btn-tx);
    font-weight: 700;
    font-size: 15px;
    transition: transform var(--t-fast) var(--ease), opacity var(--t-fast) ease;
  }

  .hero-cta:hover:not(:disabled) { opacity: 0.9; }
  .hero-cta:active { transform: scale(0.98); }

  .unavailable { margin: 0; font-size: 13px; opacity: 0.8; }
  .error-banner { color: var(--color-danger); margin: 0; line-height: 1.6; }

  @media (max-width: 1100px) {
    .pills { justify-content: flex-start; }
  }

  @media (max-width: 640px) {
    .hero-cta { width: 100%; }
    .action-divider { display: none; }
    .detail-sections { margin-top: 20px; gap: 18px; }
  }
</style>
