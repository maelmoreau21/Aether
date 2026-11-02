<script lang="ts">
  import WorkTools from './WorkTools.svelte';
  import OfflineDownload from './OfflineDownload.svelte';
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
  import { videoResolutionBadge, containerLabel, colorProfileLabel, inspectedAudioTracks, audioChannelBadge } from '../player/track-labels';
  import WatchStatus from './WatchStatus.svelte';
  import Icon from '../ui/Icon.svelte';
  import LanguageLabel from '../ui/LanguageLabel.svelte';

  let { work, playableMedia = [], allowedIds = [], selected = $bindable(''), mediaError = '', similarCandidates = [], onPlay }: {
    work: Work; playableMedia: Media[]; allowedIds: string[]; selected: string; mediaError?: string; similarCandidates?: Work[]; onPlay: () => Promise<void>;
  } = $props();
  const currentSelectedMedia = $derived(playableMedia.find(item => item.id === selected) ?? playableMedia[0]);
  const continuing = $derived($history.some(item => item.workId === work.id && isResumable(item, $serverPresentation)));
  const currentVideoTrack = $derived(currentSelectedMedia?.sources[0]?.tracks?.find(track => track.codec_type === 'video'));
  const currentAudioTracks = $derived(inspectedAudioTracks(currentSelectedMedia));
  const resolutionBadge = $derived(videoResolutionBadge(currentVideoTrack?.width, currentVideoTrack?.height));
  const colorBadge = $derived(colorProfileLabel(currentVideoTrack?.pix_fmt));
  const downloadOptions = $derived(playableMedia.filter(item => item.available !== false).map(item => ({ url: `/api/v1/assets/${encodeURIComponent(item.id)}/content`, label: sourceLabel(item), mime: item.sources.find(source => !source.prepare)?.mime ?? 'application/octet-stream' })));
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
          <button type="button" class="version-chip" aria-pressed={selected === item.id} onclick={() => selected = item.id}>
            {res ? `${res} (${sourceLabel(item)})` : sourceLabel(item)}
          </button>
        {/each}
      </div>
    {/if}

    <div class="play-action-row">
      <button class="hero-cta" onpointerenter={() => { if (!work.synthetic) preloadProgress(work.id); }} onfocus={() => { if (!work.synthetic) preloadProgress(work.id); }} disabled={work.available === false || (!playableMedia.length && !allowedIds.length)} onclick={onPlay}>
        <Icon name="play" size={20} /><span>{work.available === false ? 'Temporairement indisponible' : continuing ? 'Reprendre le film' : 'Lancer le film'}</span>
      </button>

      {#if !work.synthetic}
        <div class="tools-inline">
          {#if currentSelectedMedia}
            <OfflineDownload options={downloadOptions} url={`/api/v1/assets/${encodeURIComponent(currentSelectedMedia.id)}/content`} title={work.title} mime={currentSelectedMedia.sources.find(source => !source.prepare)?.mime ?? 'application/octet-stream'} />
          {/if}
          <WorkTools {work} selectedMedia={currentSelectedMedia} />
        </div>
      {/if}

      {#if !playableMedia.length && !allowedIds.length}<p class="unavailable">Aucune version disponible pour la lecture.</p>{/if}
    </div>
    {#if mediaError}<p class="error-banner" role="alert">{mediaError}</p>{/if}
  </DetailHero>

  {#if playableMedia.length || allowedIds.length}
    <details class="versions-panel">
      <summary>Versions et informations techniques</summary>
      {#if playableMedia.length > 1}
        <div class="versions" aria-label="Versions disponibles">
          <button type="button" onclick={() => { const probe = document.createElement('video'); selected = bestVersion(playableMedia, mime => probe.canPlayType(mime))?.id ?? selected; }}>Meilleure version disponible</button>
          {#each playableMedia as item (item.id)}<button type="button" aria-pressed={selected === item.id} aria-label={`Sélectionner la version ${sourceLabel(item)}`} onclick={() => selected = item.id}>{sourceLabel(item)}</button>{/each}
        </div>
      {/if}
      {#if currentSelectedMedia}
        <dl class="specs-grid">
          <div><dt>Vidéo</dt><dd>{currentVideoTrack?.codec_name?.toUpperCase() ?? 'Codec inconnu'}
            {#if currentVideoTrack?.width && currentVideoTrack?.height} · {currentVideoTrack.width} × {currentVideoTrack.height}{/if}
            {#if videoResolutionBadge(currentVideoTrack?.width, currentVideoTrack?.height)} · {videoResolutionBadge(currentVideoTrack?.width, currentVideoTrack?.height)}{/if}
            {#if colorProfileLabel(currentVideoTrack?.pix_fmt)} · {colorProfileLabel(currentVideoTrack?.pix_fmt)}{/if}
          </dd></div>
          <div><dt>Audio ({currentAudioTracks.length})</dt><dd>
            {#each currentAudioTracks as track, index (index)}<span><LanguageLabel language={track.language} /> · {track.codec_name?.toUpperCase() ?? 'Codec inconnu'}{track.channel_layout || track.channels ? ` · ${track.channel_layout ?? `${track.channels} canaux`}` : ''}{track.tags?.title ? ` · ${track.tags.title}` : ''}</span>{:else}Aucune piste audio analysée{/each}
          </dd></div>
          <div><dt>Conteneur</dt><dd>{containerLabel(currentSelectedMedia.sources[0]?.mime)}</dd></div>
          {#if currentSelectedMedia.subtitles.length}<div><dt>Sous-titres</dt><dd>{#each currentSelectedMedia.subtitles as subtitle (subtitle.id)}<span><LanguageLabel language={subtitle.language} />{subtitle.sourceFormat ? ` · ${subtitle.sourceFormat.toUpperCase()}` : ''}{subtitle.forced ? ' · Forcés' : ''}{subtitle.sdh ? ' · Malentendants' : ''}</span>{/each}</dd></div>{/if}
        </dl>
      {:else}<p role="status">Les informations de la version ne sont pas encore disponibles.</p>{/if}
    </details>
  {/if}

  {#if !work.synthetic}
    <details class="personal-panel">
      <summary>Mon suivi et mes préférences</summary>
      <WatchStatus workId={work.id} />
      <PersonalFeedback workId={work.id} />
    </details>
  {/if}

  <EditorialCredits {work} />
  <SimilarWorks {work} candidates={similarCandidates} />
</div>

<style>
  .film-detail-view { padding-bottom: 40px; }

  .media-experience-strip {
    display: flex;
    flex-wrap: wrap;
    gap: 16px 28px;
    padding: 14px 18px;
    margin-top: 4px;
    background: var(--color-glass-raised);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-lg);
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
    border-color: rgba(249, 115, 22, 0.45);
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
    background: rgba(249, 115, 22, 0.18);
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

  .hero-cta {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    min-height: 48px;
    max-width: 100%;
    padding: 12px 28px;
    border: 0;
    border-radius: var(--radius-pill);
    background: var(--color-play);
    color: var(--color-on-play);
    font-weight: 700;
    font-size: 15px;
    box-shadow: 0 6px 20px rgba(249, 115, 22, 0.36);
    transition: transform var(--transition-fast), box-shadow var(--transition-fast), background var(--transition-fast);
  }

  .hero-cta:hover:not(:disabled) {
    background: var(--color-play-hover, var(--color-play));
    transform: translateY(-2px);
    box-shadow: 0 10px 26px rgba(249, 115, 22, 0.5);
  }

  .unavailable { margin: 0; font-size: 13px; color: var(--color-muted); }
  .error-banner { color: var(--color-danger); margin: 0; line-height: 1.6; }

  @media (max-width: 640px) {
    .hero-cta { width: 100%; }
    .media-experience-strip { width: 100%; gap: 12px 20px; }
  }

  .versions-panel, .personal-panel { border: 1px solid var(--color-line); border-radius: var(--radius-lg); background: var(--color-surface); padding: 4px 18px; margin: 18px 0; box-shadow: var(--shadow-sm); }
  summary { padding: 14px 4px; min-height: 48px; cursor: pointer; font-size: 15px; font-weight: 650; color: var(--color-text-bright); }
  .versions { display: flex; gap: 10px; flex-wrap: wrap; padding-block: 12px; border-top: 1px solid var(--color-line-subtle); }
  .versions button { min-height: 44px; max-width: 100%; overflow-wrap: anywhere; padding: 10px 16px; border: 1px solid var(--color-line-strong); border-radius: var(--radius-pill); background: var(--color-raised); color: var(--color-text); font-size: 13px; font-weight: 500; cursor: pointer; }
  .versions button:hover { border-color: var(--color-text); color: var(--color-text-bright); }
  .versions button[aria-pressed='true'] { border-color: var(--color-accent); background: var(--color-accent-soft); color: var(--color-text-bright); font-weight: 650; }
  .specs-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 24px; padding: 14px 4px 20px; margin: 0; font-size: 14px; line-height: 1.7; border-top: 1px solid var(--color-line-subtle); }
  dt { color: var(--color-muted); font-size: 11.5px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.06em; margin-bottom: 6px; }
  dd { margin: 0; overflow-wrap: anywhere; color: var(--color-text-bright); }
  dd span { display: block; }
  @media (max-width: 640px) { .specs-grid { grid-template-columns: minmax(0, 1fr); gap: 16px; } }

</style>
