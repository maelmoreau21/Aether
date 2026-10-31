<script lang="ts">
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import { type Work, localizedTitle } from './api';
  import { locale } from '../i18n';
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

  const displayTitle = $derived(localizedTitle(work, $locale));
  const posterUrl = $derived(selectArtwork(work, $session).poster);
  const previewUrl = $derived(imagePreviewUrl(posterUrl, showMetadata ? 320 : 640));
  const imageUrl = $derived([previewUrl, posterUrl ?? ''].find(url => url && !failedUrls.includes(url)) ?? '');
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
  <div class="poster">
    {#if imageUrl}
      <img
        class:loaded={loadedUrl === imageUrl}
        src={imageUrl}
        alt=""
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
        onerror={() => {
          failedUrls = [...failedUrls.slice(-3), imageUrl];
        }}
      />
    {:else}
      <div class="poster-fallback" aria-hidden="true">
        <Icon name={work.kind === 'series' ? 'tv' : 'film'} size={34} />
        <span>{displayTitle}</span>
      </div>
    {/if}

    {#if showMetadata && href}
      <div class="card-hover-overlay" aria-hidden="true">
        <span class="hover-action-pill">
          <Icon name="info" size={14} />
          <span>Découvrir</span>
        </span>
      </div>
    {/if}

    {#if isWatched}
      <span class="watched-badge" title="Déjà visionné">
        <Icon name="check" size={14} />
        <span class="sr-only">Déjà visionné</span>
      </span>
    {/if}
    {#if work.available === false}
      <span class="offline-badge">Indisponible</span>
    {/if}
    {#if progressRatio > 0}
      <div
        class="poster-progress"
        role="progressbar"
        aria-label={`Progression de ${displayTitle}`}
        aria-valuenow={Math.round(progressRatio)}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <span style:width={`${progressRatio}%`}></span>
      </div>
    {/if}
  </div>
  {#if showMetadata}
    <div class="metadata-block">
      <h3>{displayTitle}</h3>
      {#if work.year || work.rating}
        <p>
          {#if work.year}<span>{work.year}</span>{/if}
          {#if work.rating}<span class="rating"><Icon name="star" size={12} />{work.rating.toFixed(1)}</span>{/if}
        </p>
      {/if}
    </div>
  {/if}
{/snippet}

{#if href}
  <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- URL resolved by caller. -->
  <a class="media-card" {href} aria-label={`Voir ${displayTitle}`}>{@render posterBody()}</a>
{:else}
  <div class="media-card">{@render posterBody()}</div>
{/if}

<style>
  .media-card {
    display: block;
    min-width: 0;
    color: inherit;
    position: relative;
  }

  .poster {
    aspect-ratio: 2 / 3;
    position: relative;
    overflow: hidden;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-card);
    transition:
      transform 250ms cubic-bezier(0.25, 0.8, 0.25, 1),
      border-color 250ms cubic-bezier(0.25, 0.8, 0.25, 1),
      box-shadow 250ms cubic-bezier(0.25, 0.8, 0.25, 1);
  }

  .poster img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity var(--transition-normal);
  }

  .poster img.loaded {
    opacity: 1;
  }

  @media (hover: hover) and (pointer: fine) {
    a.media-card:hover .poster,
    a.media-card:focus-visible .poster {
      transform: scale(1.06);
      border-color: var(--color-line-strong);
      box-shadow: var(--shadow-card-hover);
      z-index: 5;
    }
  }

  .media-card:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 4px;
    border-radius: var(--radius-md);
  }

  .card-hover-overlay {
    position: absolute;
    inset: 0;
    background: var(--gradient-card);
    display: flex;
    align-items: flex-end;
    justify-content: center;
    padding: 16px 12px;
    opacity: 0;
    transition: opacity 220ms ease;
    pointer-events: none;
  }

  @media (hover: hover) and (pointer: fine) {
    a.media-card:hover .card-hover-overlay,
    a.media-card:focus-visible .card-hover-overlay {
      opacity: 1;
    }
  }

  .hover-action-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 14px;
    border-radius: var(--radius-pill);
    background: var(--color-glass-raised);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    border: 1px solid var(--color-line-strong);
    color: var(--color-text-bright);
    font-size: 12px;
    font-weight: 650;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.45);
  }

  .poster-fallback {
    display: flex;
    height: 100%;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    padding: 20px 12px;
    text-align: center;
    color: var(--color-muted);
    background: radial-gradient(circle at 50% 30%, rgba(255, 255, 255, 0.04), transparent 70%), var(--color-surface);
  }

  .poster-fallback span {
    font-size: 14.5px;
    font-weight: 600;
    line-height: 1.4;
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    overflow: hidden;
  }

  .watched-badge {
    position: absolute;
    top: 10px;
    right: 10px;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    color: var(--color-on-play);
    background: var(--color-play);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.5);
  }

  .offline-badge {
    position: absolute;
    bottom: 10px;
    left: 8px;
    right: 8px;
    padding: 6px;
    font-size: 11px;
    font-weight: 600;
    text-align: center;
    color: #fff;
    background: rgba(24, 24, 27, 0.92);
    backdrop-filter: blur(8px);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-xs);
  }

  .poster-progress {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    height: 4px;
    background: rgba(24, 24, 27, 0.85);
  }

  .poster-progress span {
    display: block;
    height: 100%;
    background: var(--color-play);
    box-shadow: 0 0 8px var(--color-accent-glow);
  }

  .metadata-block {
    padding-top: 10px;
  }

  h3 {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    line-height: 1.4;
    color: var(--color-text-bright);
    overflow-wrap: anywhere;
    transition: color var(--transition-fast);
  }

  a.media-card:hover h3 {
    color: var(--color-accent-hover);
  }

  p {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin: 4px 0 0;
    color: var(--color-muted);
    font-size: 12px;
  }

  .rating {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: #FBBF24;
    font-weight: 600;
  }
</style>
