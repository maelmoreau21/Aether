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
  import type { Media } from '../player/types';
  import { sourceLabel } from './api';
  import { videoResolutionBadge, colorProfileLabel, inspectedAudioTracks, audioChannelBadge } from '../player/track-labels';
  import WatchStatus from './WatchStatus.svelte';
  import Icon from '../ui/Icon.svelte';
  import LanguageLabel from '../ui/LanguageLabel.svelte';
  import ChapterImages from './ChapterImages.svelte';
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
</script>

<div class="film-detail-view">
  <DetailHero {work} duration={currentSelectedMedia?.sources[0]?.durationSeconds}>
    {#if currentSelectedMedia}
      <div class="media-experience-strip" aria-label="Qualité d’image, langues audio et sous-titres">
        <div class="experience-group">
          <span class="exp-label">Qualité</span>
          <div class="exp-pills">
            <span class="quality-pill highlight">
              {resolutionBadge ?? (currentVideoTrack?.height ? `${currentVideoTrack.height}p` : 'HD')}
            </span>
            {#if colorBadge && colorBadge !== '8 bits standard'}
              <span class="quality-pill">{colorBadge}</span>
            {/if}
          </div>
        </div>

        <div class="experience-group">
          <span class="exp-label">Audio</span>
          <div class="exp-pills">
            {#each currentAudioTracks as track, index (index)}
              {@const ch = audioChannelBadge(track)}
              <span class="lang-pill">
                <LanguageLabel language={track.language} />
                {#if ch}<strong class="channel-tag">{ch}</strong>{/if}
              </span>
            {:else}
              <span class="lang-pill muted-pill">Audio original</span>
            {/each}
          </div>
        </div>

        <div class="experience-group">
          <span class="exp-label">Sous-titres</span>
          <div class="exp-pills">
            {#if currentSelectedMedia.subtitles.length}
              {#each currentSelectedMedia.subtitles as subtitle (subtitle.id)}
                <span class="lang-pill">
                  <LanguageLabel language={subtitle.language} />
                  {#if subtitle.forced}<small class="sub-tag">Forcés</small>{/if}
                  {#if subtitle.sdh}<small class="sub-tag">SDH</small>{/if}
                </span>
              {/each}
            {:else}
              <span class="lang-pill muted-pill">Aucun sous-titre</span>
            {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if playableMedia.length > 1}
      <div class="versions-selector" aria-label="Choisir la qualité">
        <span class="exp-label">Version :</span>
        <button type="button" class="version-chip" onclick={() => { const probe = document.createElement('video'); selected = bestVersion(playableMedia, mime => probe.canPlayType(mime))?.id ?? selected; }}>Auto (Meilleure)</button>
        {#each playableMedia as item (item.id)}
          {@const vTrack = item.sources[0]?.tracks?.find(t => t.codec_type === 'video')}
          {@const res = videoResolutionBadge(vTrack?.width, vTrack?.height)}
          <button type="button" class="version-chip" aria-pressed={selected === item.id} aria-label={$t('select_version', { value: sourceLabel(item) })} onclick={() => selected = item.id}>
            {res ? `${res} (${sourceLabel(item)})` : sourceLabel(item)}
          </button>
        {/each}
      </div>
    {/if}

    <div class="play-action-row">
      <button class="hero-cta" onpointerenter={() => { if (!work.synthetic) preloadProgress(work.id); }} onfocus={() => { if (!work.synthetic) preloadProgress(work.id); }} disabled={work.available === false || (!playableMedia.length && !allowedIds.length)} onclick={onPlay}>
        <Icon name="play" size={20} /><span>{work.available === false ? $t('temporarily_unavailable') : continuing ? $t('resume_the_film') : $t('play_the_film')}</span>
      </button>

      {#if !work.synthetic}
        <div class="tools-inline personal-actions" aria-label="Mon suivi">
          <WatchStatus workId={work.id} duration={currentSelectedMedia?.sources[0]?.durationSeconds} compact />
          <PersonalFeedback workId={work.id} compact />
        </div>
        <span class="action-divider" aria-hidden="true"></span>
        <!-- Offline downloads are offered by the native clients only. -->
        <div class="tools-inline">
          <WorkTools {work} selectedMedia={currentSelectedMedia} />
        </div>
      {/if}

      {#if !playableMedia.length && !allowedIds.length}<p class="unavailable">Aucune version disponible pour la lecture.</p>{/if}
    </div>
    {#if mediaError}<p class="error-banner" role="alert">{mediaError}</p>{/if}
    {#snippet below()}<EditorialCredits {work} />{/snippet}
  </DetailHero>

  {#if currentSelectedMedia && !work.synthetic}{#key currentSelectedMedia.id}<ChapterImages assetId={currentSelectedMedia.id} />{/key}{/if}
  <SimilarWorks {work} candidates={similarCandidates} />
</div>

<style>
  .film-detail-view { padding-bottom: 40px; }

  .media-experience-strip {
    display: flex;
    flex-wrap: wrap;
    gap: 16px 28px;
    padding: 14px 18px;
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    width: fit-content;
    max-width: 100%;
  }

  .experience-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .exp-label {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--color-muted);
  }

  .exp-pills {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .quality-pill {
    display: inline-flex;
    align-items: center;
    padding: 4px 10px;
    border-radius: var(--radius-xs);
    font-size: 12.5px;
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

  .lang-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: var(--radius-pill);
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    color: var(--color-text-bright);
    font-size: 12.5px;
    font-weight: 550;
  }

  .muted-pill {
    color: var(--color-muted);
    font-weight: 450;
  }

  .channel-tag {
    display: inline-block;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--ac-soft);
    color: var(--color-accent);
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.02em;
  }

  .sub-tag {
    padding: 1px 5px;
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.08);
    color: var(--color-muted);
    font-size: 10.5px;
    font-weight: 600;
  }

  .versions-selector {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
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
    margin-top: 8px;
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

  .unavailable { margin: 0; font-size: 13px; color: var(--color-muted); }
  .error-banner { color: var(--color-danger); margin: 0; line-height: 1.6; }

  @media (max-width: 640px) {
    .hero-cta { width: 100%; }
    .action-divider { display: none; }
    .media-experience-strip { width: 100%; gap: 12px 20px; }
  }
</style>
