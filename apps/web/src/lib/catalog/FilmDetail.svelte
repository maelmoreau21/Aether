<script lang="ts">
  import WorkTools from './WorkTools.svelte';
  import OfflineDownload from './OfflineDownload.svelte';
  import { bestVersion } from '../player/version-choice';
  import { imagePreviewUrl } from './image-url';
  import { preloadProgress } from '../player/progress';
  import type { Work } from './api';
  import PersonalFeedback from './PersonalFeedback.svelte';
  import EditorialCredits from './EditorialCredits.svelte';
  import type { Media } from '../player/types';
  import { sourceLabel } from './api';
  import {
    videoResolutionBadge,
    containerLabel,
    colorProfileLabel,
    inspectedAudioTracks
  } from '../player/track-labels';
  import WatchStatus from './WatchStatus.svelte';
  import Poster from './Poster.svelte';
  import Icon from '../ui/Icon.svelte';
  import LanguageLabel from '../ui/LanguageLabel.svelte';

  let {
    work,
    playableMedia = [],
    allowedIds = [],
    selected = $bindable(''),
    mediaError = '',
    onPlay
  }: {
    work: Work;
    playableMedia: Media[];
    allowedIds: string[];
    selected: string;
    mediaError?: string;
    onPlay: () => Promise<void>;
  } = $props();

  const currentSelectedMedia = $derived(
    playableMedia.find(item => item.id === selected) ?? playableMedia[0]
  );
  let backdropLoaded = $state(false);
  const isMeaningless = (s?: string | null) => !s || s.trim() === '' || s.trim() === 'Média local indexé' || s.trim() === 'Média local';
  const displaySynopsis = $derived(!isMeaningless(work.plot) ? work.plot : !isMeaningless(work.description) ? work.description : 'Synopsis non renseigné.');
  const showOutline = $derived(Boolean(work.outline && !isMeaningless(work.outline) && work.outline !== work.description && work.outline !== work.plot));
  const currentVideoTrack = $derived(
    currentSelectedMedia?.sources[0]?.tracks?.find(t => t.codec_type === 'video')
  );
  const currentAudioTracks = $derived(inspectedAudioTracks(currentSelectedMedia));
  const downloadOptions = $derived(playableMedia.filter(item => item.available !== false).map(item => ({
    url: `/api/v1/assets/${encodeURIComponent(item.id)}/content`,
    label: sourceLabel(item),
    mime: item.sources.find(source => !source.prepare)?.mime ?? 'application/octet-stream'
  })));
</script>

<div class="film-detail-view">
  <!-- Backdrop Banner -->
  <div class="detail-hero">
    {#if work.artwork?.backdrop || work.posterUrl}
      <div class="backdrop-art-container" aria-hidden="true">
        <img
          class="backdrop-art-image"
          class:artwork={!!work.artwork?.backdrop}
          class:loaded={backdropLoaded}
          src={imagePreviewUrl(work.artwork?.backdrop || work.posterUrl, 1280)}
          alt=""
          loading="eager"
          decoding="async"
          onload={() => { backdropLoaded = true; }}
        />
        <div class="backdrop-vignette"></div>
      </div>
    {/if}

    <div class="detail-main-layout">
      <!-- Poster Column -->
      <div class="detail-poster-column">
        <Poster {work} showMetadata={false} eager />
      </div>

      <!-- Information Column -->
      <div class="detail-info-column">
        <!-- Badges Row -->
        <div class="badge-row">
          <span class="badge kind-tag">FILM</span>
          {#if work.synthetic}
            <span class="badge demo-tag">DÉMONSTRATION</span>
          {/if}
          {#if work.year}
            <span class="meta-tag">{work.year}</span>
          {/if}
          {#if work.rating}
            <span class="badge rating-tag">
              <Icon name="star" size={13} />
              {work.rating.toFixed(1)} / 10
            </span>
          {/if}
          {#if work.mpaa}
            <span class="badge mpaa-tag">{work.mpaa}</span>
          {/if}
          {#if currentVideoTrack?.width && currentVideoTrack?.height}
            <span class="badge tech-tag">
              {videoResolutionBadge(currentVideoTrack.width, currentVideoTrack.height)}
            </span>
          {/if}
        </div>

        {#if work.artwork?.logo}<img class="title-logo" src={imagePreviewUrl(work.artwork.logo, 640)} alt="" decoding="async" />{/if}
        <h1>{work.title}</h1>
        {#if !work.synthetic}<PersonalFeedback workId={work.id} />{/if}
        {#if !work.synthetic}<WorkTools {work} selectedMedia={currentSelectedMedia} />{/if}
        {#if currentSelectedMedia && !work.synthetic}<OfflineDownload options={downloadOptions} url={`/api/v1/assets/${currentSelectedMedia.id}/content`} title={work.title} mime={currentSelectedMedia.sources.find(s=>!s.prepare)?.mime??'application/octet-stream'} />{/if}

        {#if work.originalTitle && work.originalTitle !== work.title}
          <p class="original-title">Titre original : <em>{work.originalTitle}</em></p>
        {/if}

        {#if work.genres?.length}
          <div class="genres-row">
            {#each work.genres as g (g)}
              <span class="genre-pill">{g}</span>
            {/each}
          </div>
        {/if}

        {#if showOutline}
          <blockquote class="outline-hook">« {work.outline} »</blockquote>
        {/if}

        <p class="synopsis">{displaySynopsis}</p>

        <!-- Offline Node Notice -->
        {#if work.available === false}
          <div class="node-offline-box" role="status">
            <Icon name="alert" size={20} />
            <div>
              <strong>Média temporairement indisponible</strong>
              <p>Le Media Node propriétaire (<code>{work.nodeId ?? 'distant'}</code>) est actuellement hors ligne. L’œuvre reste indexée au catalogue et sera immédiatement lisible dès le retour du nœud.</p>
            </div>
          </div>
        {/if}

        <!-- Actions Section -->
        <div class="film-actions">
          {#if !work.synthetic}
            <WatchStatus workId={work.id} />
          {/if}

          <!-- Versions Selector -->
          {#if playableMedia.length > 1}
            <div class="versions-section">
              <span class="versions-label">Versions disponibles :</span>
              <div class="versions" aria-label="Versions disponibles">
                <button class="version-choice" onclick={()=>{const probe=document.createElement('video');selected=bestVersion(playableMedia,mime=>probe.canPlayType(mime))?.id??selected;}}>Meilleure version disponible</button>
                {#each playableMedia as item (item.id)}
                  <button
                    class="version-choice"
                    aria-pressed={selected === item.id}
                    aria-label={`Sélectionner la version ${sourceLabel(item)}`}
                    onclick={() => (selected = item.id)}
                  >
                    {sourceLabel(item)}
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          <div class="play-action-row">
            <button
              class="hero-cta"
              onpointerenter={() => { if (!work.synthetic) preloadProgress(work.id); }}
              onfocus={() => { if (!work.synthetic) preloadProgress(work.id); }}
              disabled={work.available === false || (!playableMedia.length && !allowedIds.length)}
              onclick={onPlay}
            >
              <Icon name="play" size={22} />
              <span>{work.available === false ? 'Node indisponible' : 'Lancer le film'}</span>
            </button>
            {#if !playableMedia.length && !allowedIds.length}
              <p class="unavailable">Aucune version disponible pour la lecture.</p>
            {/if}
          </div>
        </div>

        {#if mediaError}
          <div class="error-banner" role="alert">
            <Icon name="alert" size={18} />
            <span>{mediaError}</span>
          </div>
        {/if}

        <!-- Technical Specifications Inspector -->
        {#if currentSelectedMedia}
          <div class="tech-specs-card">
            <h4>Vidéo, audio et sous-titres</h4>
            <div class="specs-grid">
              <div>
                <span class="spec-label">🎬 Vidéo</span>
                <span class="spec-val">
                  {currentVideoTrack?.codec_name?.toUpperCase() ?? 'Codec inconnu'}
                  {#if currentVideoTrack?.width && currentVideoTrack?.height}
                    · {currentVideoTrack.width} × {currentVideoTrack.height}
                  {/if}
                  {#if videoResolutionBadge(currentVideoTrack?.width, currentVideoTrack?.height)}
                    <span class="tech-badge">{videoResolutionBadge(currentVideoTrack?.width, currentVideoTrack?.height)}</span>
                  {/if}
                  {#if colorProfileLabel(currentVideoTrack?.pix_fmt)}
                    · {colorProfileLabel(currentVideoTrack?.pix_fmt)}
                  {/if}
                </span>
              </div>
              <div>
                <span class="spec-label">🔊 Audio ({currentAudioTracks.length})</span>
                <span class="spec-val">
                  {#if currentAudioTracks.length > 0}
                    {#each currentAudioTracks as at, idx (idx)}
                      <span class="audio-track-spec">
                        <LanguageLabel language={at.language} /> · {at.codec_name?.toUpperCase() ?? 'Codec inconnu'}{at.channel_layout || at.channels ? ` · ${at.channel_layout ?? `${at.channels} canaux`}` : ''}{at.tags?.title ? ` · ${at.tags.title}` : ''}
                      </span>
                    {/each}
                  {:else}
                    Aucune piste audio analysée
                  {/if}
                </span>
              </div>
              <div>
                <span class="spec-label">📦 Conteneur</span>
                <span class="spec-val">
                  {containerLabel(currentSelectedMedia.sources[0]?.mime)}
                </span>
              </div>
              {#if currentSelectedMedia.subtitles.length > 0}
                <div>
                  <span class="spec-label">💬 Sous-titres</span>
                  <span class="spec-val">
                    {#each currentSelectedMedia.subtitles as subtitle (subtitle.id)}<span class="audio-track-spec"><LanguageLabel language={subtitle.language} />{subtitle.sourceFormat ? ` · ${subtitle.sourceFormat.toUpperCase()}` : ''}{subtitle.forced ? ' · Forcés' : ''}{subtitle.sdh ? ' · Malentendants' : ''}</span>{/each}
                  </span>
                </div>
              {/if}
            </div>
          </div>
        {/if}

        {#if work.artwork?.illustration || work.artwork?.landscape}
          <div class="artwork-gallery" aria-label="Illustrations locales">
            {#each ['illustration','landscape'] as role (role)}
              {#if work.artwork?.[role]}<img src={imagePreviewUrl(work.artwork[role], 640)} alt={role === 'illustration' ? `Illustration de ${work.title}` : `Visuel de ${work.title}`} loading="lazy" decoding="async" />{/if}
            {/each}
          </div>
        {/if}
        <EditorialCredits {work} />

        {#if work.synthetic}
          <p class="detail-note">Fiche de démonstration synthétique pour la validation du lecteur.</p>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .artwork-gallery { display:flex;flex-wrap:wrap;gap:12px;margin-block:16px; }
  .artwork-gallery img { max-width:100%;width:260px;max-height:160px;object-fit:contain;border-radius:6px; }

  .title-logo { max-width:min(340px,100%); max-height:110px; object-fit:contain; object-position:left; }

  .film-detail-view {
    position: relative;
    padding-bottom: 40px;
  }

  .detail-hero {
    position: relative;
    padding-top: 20px;
  }

  .backdrop-art-container {
    position: absolute;
    top: 0;
    left: calc(-1 * var(--page-gutter));
    right: calc(-1 * var(--page-gutter));
    height: 520px;
    overflow: hidden;
    z-index: 0;
    pointer-events: none;
  }

  .backdrop-art-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center 25%;
    filter: blur(28px) brightness(0.75) saturate(1.3);
    transform: scale(1.15);
    opacity: 0;
    transition: opacity 0.4s ease;
  }

  .backdrop-art-image.artwork { filter: brightness(0.8); transform:none; }

  .backdrop-art-image.loaded {
    opacity: 1;
  }

  .backdrop-vignette {
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, color-mix(in srgb, var(--bg-main) 40%, transparent) 0%, color-mix(in srgb, var(--bg-main) 85%, transparent) 60%, var(--color-canvas) 100%);
  }

  :global([data-theme="light"]) .backdrop-vignette {
    background: linear-gradient(180deg, color-mix(in srgb, var(--bg-main) 80%, transparent), var(--bg-main));
  }

  .detail-main-layout {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: clamp(240px, 24vw, 340px) 1fr;
    gap: clamp(28px, 4vw, 56px);
    align-items: flex-start;
  }

  .detail-poster-column {
    position: sticky;
    top: 96px;
  }

  .detail-info-column {
    min-width: 0;
    overflow-wrap: anywhere;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .badge-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }

  .badge {
    font-size: 10.5px;
    font-weight: 700;
    padding: 3px 8px;
    border-radius: var(--radius-xs);
    background: rgba(255, 255, 255, 0.08);
    color: var(--color-text);
  }

  .kind-tag {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border: 1px solid var(--border-subtle);
  }

  .demo-tag {
    background: rgba(251, 191, 36, 0.15);
    color: var(--accent-secondary);
  }

  .rating-tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: rgba(251, 191, 36, 0.15);
    color: var(--accent-secondary);
    border: 1px solid rgba(251, 191, 36, 0.25);
  }

  .mpaa-tag {
    border: 1px solid rgba(255, 255, 255, 0.2);
  }

  .tech-tag {
    background: var(--color-accent-soft);
    color: var(--accent-primary);
    border: 1px solid var(--border-subtle);
  }

  .meta-tag {
    font-size: 12px;
    font-weight: 600;
    color: var(--color-muted);
  }

  h1 {
    font-family: var(--font-display);
    font-size: clamp(32px, 4.2vw, 52px);
    font-weight: 750;
    line-height: 1.15;
    letter-spacing: -0.04em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .original-title {
    margin: 0;
    color: var(--color-muted);
    font-size: 14px;
  }

  .genres-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .genre-pill {
    font-size: 11.5px;
    padding: 4px 10px;
    border-radius: var(--radius-pill);
    background: rgba(255, 255, 255, 0.06);
    color: var(--color-text);
    border: 1px solid var(--color-line);
  }

  .outline-hook {
    margin: 4px 0;
    padding-left: 14px;
    border-left: 3px solid var(--color-accent);
    font-style: italic;
    font-size: 15px;
    color: var(--color-accent-hover);
  }

  .synopsis {
    font-size: 15px;
    line-height: 1.65;
    color: var(--text-secondary);
    margin: 4px 0 12px;
  }

  .node-offline-box {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    padding: 16px 20px;
    border-radius: var(--radius-md);
    background: rgba(248, 113, 113, 0.1);
    border: 1px solid rgba(248, 113, 113, 0.25);
    color: var(--color-danger);
  }

  .node-offline-box strong {
    display: block;
    margin-bottom: 4px;
    font-size: 14px;
  }

  .node-offline-box p {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--status-error);
  }

  .film-actions {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 20px 24px;
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    box-shadow: var(--shadow-sm);
  }

  .versions-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .versions-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--color-muted);
  }

  .versions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .version-choice {
    padding: 7px 16px;
    border-radius: var(--radius-md);
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    color: var(--color-text);
    font-size: 13px;
    font-weight: 600;
    transition: all var(--transition-fast);
  }

  .version-choice:hover {
    border-color: var(--accent-primary);
    background: var(--color-raised-hover);
  }

  .version-choice[aria-pressed='true'] {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
    color: var(--color-accent-hover);
    box-shadow: 0 0 12px var(--color-accent-soft);
  }

  .play-action-row {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
  }

  .hero-cta {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    min-height: 52px;
    padding: 0 32px;
    border: 0;
    border-radius: var(--radius-md);
    background: var(--color-play);
    color: var(--color-on-play);
    font-size: 15px;
    font-weight: 750;
    box-shadow: var(--shadow-sm);
    transition: all var(--transition-fast);
  }

  .hero-cta:hover:not(:disabled) {
    background: var(--color-play);
    transform: translateY(-2px);
    box-shadow: var(--shadow-md);
  }

  .hero-cta:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .unavailable {
    color: var(--color-muted);
    font-size: 13px;
    margin: 0;
  }

  .error-banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-radius: var(--radius-md);
    background: rgba(248, 113, 113, 0.12);
    border: 1px solid rgba(248, 113, 113, 0.25);
    color: var(--color-danger);
    font-size: 13px;
  }

  /* Technical Specs Card */
  .tech-specs-card {
    padding: 20px 24px;
    border-radius: var(--radius-lg);
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid var(--color-line);
  }

  .tech-specs-card h4 {
    font-size: 13px;
    font-weight: 700;
    letter-spacing: 0.04em;
    color: var(--color-text-bright);
    margin: 0 0 14px;
  }

  .specs-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(240px, 100%), 1fr));
    gap: 16px 20px;
  }

  .spec-label {
    display: block;
    font-size: 11px;
    font-weight: 600;
    color: var(--color-muted);
    margin-bottom: 4px;
  }

  .spec-val {
    font-size: 13px;
    color: var(--color-text);
  }

  .tech-badge {
    display: inline-block;
    padding: 1px 6px;
    border-radius: var(--radius-xs);
    background: var(--color-accent-soft);
    color: var(--color-accent);
    font-size: 10.5px;
    font-weight: 700;
    margin-left: 6px;
  }

  .audio-track-spec {
    display: block;
    margin-block: 4px;
    overflow-wrap: anywhere;
  }

  .detail-note {
    font-size: 12px;
    color: var(--color-muted);
    font-style: italic;
    margin: 0;
  }

  @media (max-width: 820px) {
    .detail-main-layout {
      grid-template-columns: 1fr;
    }

    .detail-poster-column {
      position: static;
      max-width: 260px;
      margin: 0 auto;
    }
  }
</style>
