<script lang="ts">
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import type { Work } from './api';
  import { history } from './watch-state';
  import Icon from '../ui/Icon.svelte';
  import { imagePreviewUrl, isImageLoaded, markImageLoaded } from './image-url';

  let failedUrls = $state<string[]>([]);
  let {
    work,
    href = '',
    showMetadata = true,
    eager = false
  }: {
    work: Work;
    href?: string;
    showMetadata?: boolean;
    eager?: boolean;
  } = $props();

  const posterUrl = $derived(selectArtwork(work, $session).poster);
  const previewUrl = $derived(imagePreviewUrl(posterUrl,showMetadata ? 320 : 640));
  const imageUrl = $derived([previewUrl,posterUrl ?? ''].find(url => url && !failedUrls.includes(url)) ?? '');
  let loadedUrl = $state('');

  $effect(() => {
    if (imageUrl && isImageLoaded(imageUrl)) {
      loadedUrl = imageUrl;
    }
  });

  function checkLoaded(node: HTMLImageElement) {
    if (node.complete && node.naturalWidth > 0 && imageUrl) {
      loadedUrl = imageUrl;
      markImageLoaded(imageUrl);
    }
  }
  const watchProgress = $derived($history.find(item => item.workId === work.id));
  const isWatched = $derived(watchProgress?.watched ?? false);
  const progressRatio = $derived(
    watchProgress && watchProgress.duration > 0 && !watchProgress.watched
      ? Math.min(100, Math.max(0, (watchProgress.position / watchProgress.duration) * 100))
      : 0
  );

</script>

{#snippet posterBody()}
  <div class="poster" class:series={work.kind === 'series'} class:has-image={!!imageUrl}>
    {#if imageUrl}
      <img
        class="poster-image"
        class:loaded={loadedUrl === imageUrl}
        src={imageUrl}
        alt={`Affiche de ${work.title}`}
        width="400"
        height="600"
        loading={eager ? 'eager' : 'lazy'}
        fetchpriority={eager ? 'high' : 'auto'}
        decoding="async"
        use:checkLoaded
        onload={() => {
          loadedUrl = imageUrl;
          markImageLoaded(imageUrl);
        }}
        onerror={() => { failedUrls = [...failedUrls.slice(-3), imageUrl]; }}
      />
      <div class="poster-gradient" aria-hidden="true"></div>
    {/if}

    <!-- Badges Row -->
    <div class="poster-badges-top">
      <span class="format" class:image-label={!!imageUrl}>
        {work.kind === 'series' ? 'SÉRIE' : 'FILM'}
        {#if work.available === false} · INDISPONIBLE{/if}
      </span>

      {#if isWatched}
        <span class="watched-badge" title="Déjà visionné">
          <Icon name="check" size={11} />
          <span>VU</span>
        </span>
      {/if}
    </div>

    <!-- Center Monogram when no poster -->
    {#if !imageUrl}
      <div class="poster-fallback-art" aria-hidden="true">
        <span class="poster-type">{work.kind === 'series' ? 'S' : 'F'}</span>
        {#if work.synthetic}
          <span class="badge-sub">DÉMO</span>
        {:else if work.available === false}
          <span class="badge-sub">HORS LIGNE</span>
        {/if}
      </div>
    {/if}

    <!-- Bottom Actions / Indicators -->
    <div class="poster-bottom">
      {#if href}
        <div class="play-hover-indicator" aria-hidden="true">
          <Icon name="play" size={16} />
        </div>
      {/if}
    </div>

    <!-- Progress Bar if partially watched -->
    {#if progressRatio > 0}
      <div class="poster-progress-bar" aria-hidden="true">
        <div class="poster-progress-fill" style:width="{progressRatio}%"></div>
      </div>
    {/if}
  </div>

  {#if showMetadata}
    <div class="metadata-block">
      <h3>{work.title}</h3>
      <p class="meta-line">
        <span class="meta-year">{work.year ?? 'Année non renseignée'}</span>
        {#if work.kind === 'series'}
          <span class="meta-sep">·</span>
          <span>{work.episodeCount ?? 0} {(work.episodeCount ?? 0) > 1 ? 'épisodes' : 'épisode'}</span>
        {/if}
        {#if work.rating}
          <span class="meta-sep">·</span>
          <span class="meta-rating">
            <Icon name="star" size={12} />
            {work.rating.toFixed(1)}
          </span>
        {/if}
        {#if work.available === false}
          <span class="meta-sep">·</span>
          <span class="meta-offline">Media Node indisponible</span>
        {/if}
      </p>
      {#if work.genres?.length}
        <p class="genres">{work.genres.join(' · ')}</p>
      {/if}
      {#if work.description}
        <p class="synopsis">{work.description}</p>
      {/if}
    </div>
  {/if}
{/snippet}

{#if href}
  <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- href already resolved by the parent route. -->
  <a class="media-card" {href} aria-label={`Voir ${work.title}`}>
    {@render posterBody()}
  </a>
{:else}
  <div class="media-card">
    {@render posterBody()}
  </div>
{/if}

<style>
  .media-card {
    display: block;
    min-width: 0;
    text-decoration: none;
    color: inherit;
    outline: none;
    position: relative;
  }

  .poster {
    aspect-ratio: 2 / 3;
    background: linear-gradient(135deg, var(--bg-elevated), var(--bg-surface));
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    position: relative;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    padding: 14px;
    justify-content: space-between;
    transition: transform 0.28s cubic-bezier(0.16, 1, 0.3, 1),
                border-color 0.28s ease,
                box-shadow 0.28s ease;
    box-shadow: var(--shadow-card);
  }

  .poster.series {
    background: linear-gradient(135deg, var(--bg-elevated), var(--bg-surface));
    border-color: var(--border-subtle);
  }

  .media-card:hover .poster {
    transform: translateY(-2px) scale(1.02);
    border-color: var(--accent-primary);
    box-shadow: var(--shadow-card-hover);
  }

  .media-card:focus-visible .poster {
    outline: 2px solid var(--color-accent);
    outline-offset: 3px;
  }

  .poster-image {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: transform 0.35s cubic-bezier(0.16, 1, 0.3, 1);
  }

  .poster-image.loaded {
    opacity: 1;
  }

  .media-card:hover .poster-image {
    transform: scale(1.025);
  }

  .poster-gradient {
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, rgba(9, 10, 16, 0.4) 0%, transparent 40%, rgba(9, 10, 16, 0.85) 100%);
    pointer-events: none;
  }

  .poster-badges-top {
    position: relative;
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    width: 100%;
  }

  .format {
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.14em;
    color: #e2d9f3;
    padding: 3px 6px;
    background: rgba(17, 19, 28, 0.85);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border-radius: var(--radius-xs);
    border: 1px solid var(--border-subtle);
  }

  .watched-badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 3px 6px;
    background: var(--status-success);
    color: #1C1917;
    font-size: 9px;
    font-weight: 800;
    letter-spacing: 0.08em;
    border-radius: var(--radius-xs);
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.4);
  }

  .poster-fallback-art {
    align-self: center;
    text-align: center;
    margin: auto 0;
    z-index: 2;
  }

  .poster-type {
    font-family: var(--font-display);
    font-size: clamp(52px, 7vw, 90px);
    font-weight: 700;
    color: var(--accent-primary);
    line-height: 1;
    display: block;
    text-shadow: none;
  }

  .badge-sub {
    display: inline-block;
    font-family: var(--font-sans);
    font-size: 8.5px;
    font-weight: 700;
    letter-spacing: 0.2em;
    color: var(--color-muted);
    background: rgba(255, 255, 255, 0.06);
    padding: 3px 8px;
    border-radius: var(--radius-xs);
    margin-top: 8px;
  }

  .poster-bottom {
    position: relative;
    z-index: 2;
    display: flex;
    justify-content: flex-end;
    align-items: center;
  }

  .play-hover-indicator {
    width: 34px;
    height: 34px;
    border-radius: 50%;
    background: var(--color-accent-solid);
    color: #ffffff;
    display: grid;
    place-items: center;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.5), 0 0 16px var(--color-accent-glow);
    opacity: 0;
    transform: scale(0.85);
    transition: opacity 0.22s ease, transform 0.22s cubic-bezier(0.16, 1, 0.3, 1);
  }

  .media-card:hover .play-hover-indicator {
    opacity: 1;
    transform: scale(1);
  }

  .poster-progress-bar {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    height: 4px;
    background: rgba(255, 255, 255, 0.2);
    z-index: 3;
    overflow: hidden;
  }

  .poster-progress-fill {
    height: 100%;
    background: var(--gradient-accent);
    box-shadow: 0 0 8px var(--color-accent-glow);
  }

  .metadata-block {
    padding-top: 10px;
  }

  h3 {
    font-size: 14.5px;
    font-weight: 600;
    line-height: 1.35;
    margin: 0 0 4px;
    color: var(--color-text-bright);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: color var(--transition-fast);
  }

  .media-card:hover h3 {
    color: var(--color-accent);
  }

  .meta-line {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 5px;
    color: var(--color-muted);
    font-size: 11.5px;
    line-height: 1.4;
    margin: 0 0 4px;
  }

  .meta-sep {
    opacity: 0.5;
  }

  .meta-rating {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--accent-secondary);
    font-weight: 600;
  }

  .meta-offline {
    color: var(--color-danger);
  }

  .genres {
    color: var(--color-subtle);
    font-size: 11px;
    margin: 0 0 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .synopsis {
    display: none; /* Synopsis belongs on the detail view. */
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    color: var(--color-muted);
    font-size: 11.5px;
    line-height: 1.45;
    margin: 4px 0 0;
    opacity: 0.85;
  }
</style>
