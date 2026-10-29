<script lang="ts">
  import WorkTools from './WorkTools.svelte';
  import OfflineDownload from './OfflineDownload.svelte';
  import { bestVersion } from '../player/version-choice';
  import { preloadProgress } from '../player/progress';
  import { history } from './watch-state';
  import type { Work } from './api';
  import PersonalFeedback from './PersonalFeedback.svelte';
  import EditorialCredits from './EditorialCredits.svelte';
  import DetailHero from './DetailHero.svelte';
  import type { Media } from '../player/types';
  import { sourceLabel } from './api';
  import { videoResolutionBadge, containerLabel, colorProfileLabel, inspectedAudioTracks } from '../player/track-labels';
  import WatchStatus from './WatchStatus.svelte';
  import Icon from '../ui/Icon.svelte';
  import LanguageLabel from '../ui/LanguageLabel.svelte';

  let { work, playableMedia = [], allowedIds = [], selected = $bindable(''), mediaError = '', onPlay }: {
    work: Work; playableMedia: Media[]; allowedIds: string[]; selected: string; mediaError?: string; onPlay: () => Promise<void>;
  } = $props();
  const currentSelectedMedia = $derived(playableMedia.find(item => item.id === selected) ?? playableMedia[0]);
  const continuing = $derived($history.some(item => item.workId === work.id && !item.watched && item.position > 0));
  const currentVideoTrack = $derived(currentSelectedMedia?.sources[0]?.tracks?.find(track => track.codec_type === 'video'));
  const currentAudioTracks = $derived(inspectedAudioTracks(currentSelectedMedia));
  const downloadOptions = $derived(playableMedia.filter(item => item.available !== false).map(item => ({ url: `/api/v1/assets/${encodeURIComponent(item.id)}/content`, label: sourceLabel(item), mime: item.sources.find(source => !source.prepare)?.mime ?? 'application/octet-stream' })));
</script>

<div class="film-detail-view">
  <DetailHero {work} duration={currentSelectedMedia?.sources[0]?.durationSeconds}>
    <div class="play-action-row">
      <button class="hero-cta" onpointerenter={() => { if (!work.synthetic) preloadProgress(work.id); }} onfocus={() => { if (!work.synthetic) preloadProgress(work.id); }} disabled={work.available === false || (!playableMedia.length && !allowedIds.length)} onclick={onPlay}>
        <Icon name="play" size={20} /><span>{work.available === false ? 'Temporairement indisponible' : continuing ? 'Reprendre le film' : 'Lancer le film'}</span>
      </button>
      {#if !playableMedia.length && !allowedIds.length}<p class="unavailable">Aucune version disponible pour la lecture.</p>{/if}
    </div>
    {#if mediaError}<p class="error-banner" role="alert">{mediaError}</p>{/if}
  </DetailHero>
  {#if !work.synthetic}<div class="secondary-actions"><WorkTools {work} selectedMedia={currentSelectedMedia} />
    {#if currentSelectedMedia}<OfflineDownload options={downloadOptions} url={`/api/v1/assets/${encodeURIComponent(currentSelectedMedia.id)}/content`} title={work.title} mime={currentSelectedMedia.sources.find(source => !source.prepare)?.mime ?? 'application/octet-stream'} />{/if}
  </div>{/if}

  {#if playableMedia.length || allowedIds.length}
    <details class="versions-panel">
      <summary>Versions et informations techniques</summary>
      {#if playableMedia.length > 1}
        <div class="versions" aria-label="Versions disponibles">
          <button onclick={() => { const probe = document.createElement('video'); selected = bestVersion(playableMedia, mime => probe.canPlayType(mime))?.id ?? selected; }}>Meilleure version disponible</button>
          {#each playableMedia as item (item.id)}<button aria-pressed={selected === item.id} aria-label={`Sélectionner la version ${sourceLabel(item)}`} onclick={() => selected = item.id}>{sourceLabel(item)}</button>{/each}
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
    <details class="personal-panel"><summary>Mon suivi et mes préférences</summary><WatchStatus workId={work.id} /><PersonalFeedback workId={work.id} />
    </details>
  {/if}
  <EditorialCredits {work} />
</div>

<style>
  .film-detail-view { padding-bottom:32px; }
  .play-action-row { display:flex; flex-wrap:wrap; align-items:center; gap:12px; width:100%; }
  .hero-cta { display:inline-flex; align-items:center; justify-content:center; gap:10px; min-height:48px; max-width:100%; padding:12px 24px; border:0; border-radius:var(--radius-md); background:var(--color-play); color:var(--color-on-play); font-weight:650; font-size:15px; }
  .hero-cta:hover:not(:disabled) { background:var(--color-play-hover,var(--color-play)); }
  .unavailable { margin:0; font-size:13px; color:var(--color-muted); }
  .error-banner { color:var(--color-danger); margin:0; line-height:1.6; }
  .secondary-actions { display:flex; align-items:center; flex-wrap:wrap; gap:12px; width:100%; margin-top:16px; }
  .versions-panel,.personal-panel { border-bottom:1px solid var(--color-line); padding:8px 0; margin-top:20px; }
  summary { padding:12px 4px; min-height:48px; cursor:pointer; font-size:15px; font-weight:600; }
  .versions { display:flex; gap:10px; flex-wrap:wrap; padding-block:12px; }
  .versions button { min-height:44px; max-width:100%; overflow-wrap:anywhere; padding:10px 14px; border:1px solid var(--color-line-strong); border-radius:var(--radius-sm); background:var(--color-surface); color:var(--color-text); font-size:13px; }
  .versions button[aria-pressed='true'] { border-color:var(--color-accent); background:var(--color-accent-soft); }
  .specs-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:24px; padding:12px 4px 20px; margin:0; font-size:14px; line-height:1.7; }
  dt { color:var(--color-muted); font-size:12px; margin-bottom:6px; }
  dd { margin:0; overflow-wrap:anywhere; } dd span { display:block; }
  @media(max-width:640px) { .specs-grid { grid-template-columns:minmax(0,1fr); gap:16px; } .hero-cta { width:100%; } }
</style>
