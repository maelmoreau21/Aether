<script lang="ts">
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import { type Work, localizedTitle } from './api';
  import { locale } from '../i18n';
  import { history } from './watch-state';
  import { isResumable } from '../player/progress';
  import { serverPresentation } from '../config/presentation';
  import Icon from '../ui/Icon.svelte';
  import { imagePreviewUrl, isImageLoaded, markImageLoaded } from './image-url';
  import { tvLayout } from '../theme';
  import { fitArtwork } from '../ui/fit-artwork';

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
  const artwork = $derived(selectArtwork(work, $session));
  // TV : landscape.jpg (16:9) dans les rails et grilles ; ailleurs folder.jpg (affiche).
  const landscapeUrl = $derived(artwork.landscape || artwork.backdrop);
  const wide = $derived($tvLayout && showMetadata && Boolean(landscapeUrl));
  const posterUrl = $derived(wide ? landscapeUrl : artwork.poster);
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
    watchProgress && isResumable(watchProgress, $serverPresentation) && watchProgress.duration > 0
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
        use:fitArtwork
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
  <a class="media-card" class:landscape={wide} {href} aria-label={`Voir ${displayTitle}`}>{@render posterBody()}</a>
{:else}
  <div class="media-card" class:landscape={wide}>{@render posterBody()}</div>
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
    background: var(--pn-2);
    border-radius: var(--r-md);
    transition: transform var(--t-slow) var(--ease), box-shadow var(--t-slow) ease;
  }

  .landscape .poster { aspect-ratio: 16 / 9; }

  .poster img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity var(--t-med) ease;
  }

  .poster img.loaded { opacity: 1; }

  @media (hover: hover) and (pointer: fine) {
    a.media-card:hover .poster {
      transform: translateY(-4px);
      box-shadow: var(--shadow);
    }
  }

  a.media-card:focus-visible { outline: none; }
  a.media-card:focus-visible .poster {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  .card-hover-overlay {
    position: absolute;
    inset: 0;
    background: linear-gradient(0deg, var(--media-shade) 0%, rgb(6 8 14 / 0) 58%);
    display: flex;
    align-items: flex-end;
    justify-content: flex-start;
    padding: 12px;
    opacity: 0;
    transition: opacity var(--t-med) ease;
    pointer-events: none;
  }

  @media (hover: hover) and (pointer: fine) {
    a.media-card:hover .card-hover-overlay,
    a.media-card:focus-visible .card-hover-overlay { opacity: 1; }
  }

  .hover-action-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
    border-radius: 10px;
    background: var(--on-media);
    color: #0b0e18;
    font-size: 12px;
    font-weight: 800;
  }

  /* Repli typographique quand folder.jpg manque */
  .poster-fallback {
    display: flex;
    height: 100%;
    flex-direction: column;
    align-items: flex-start;
    justify-content: flex-end;
    gap: 10px;
    padding: 14px;
    color: var(--mu);
    background: var(--bg-2);
  }

  .poster-fallback span {
    font-size: 16px;
    font-weight: 800;
    line-height: 1.15;
    letter-spacing: -0.01em;
    color: var(--tx);
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
    border-radius: 9px;
    color: var(--on-media);
    background: rgb(6 8 14 / 0.6);
  }

  .offline-badge {
    position: absolute;
    bottom: 10px;
    left: 10px;
    right: 10px;
    padding: 6px;
    font-size: 11px;
    font-weight: 700;
    text-align: center;
    color: var(--on-media);
    background: rgb(6 8 14 / 0.75);
    border-radius: var(--r-xs);
  }

  .poster-progress {
    position: absolute;
    left: 10px;
    right: 10px;
    bottom: 10px;
    height: 4px;
    border-radius: 4px;
    overflow: hidden;
    background: rgb(255 255 255 / 0.25);
  }

  .poster-progress span {
    display: block;
    height: 100%;
    border-radius: 4px;
    background: var(--ac);
  }

  .metadata-block { padding-top: 10px; }

  h3 {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    margin: 0;
    font-size: 14px;
    font-weight: 700;
    line-height: 1.3;
    color: var(--tx);
    overflow-wrap: anywhere;
  }

  p {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 3px 0 0;
    color: var(--mu);
    font-size: 13px;
  }

  .rating {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--ac);
    font-weight: 800;
  }
</style>
