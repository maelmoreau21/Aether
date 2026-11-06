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
  import { goto } from '$app/navigation';
  import { markWatched } from './watch-state';
  import { quickPlay, readFavorite, toggleFavorite } from './quick-play';

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

  // Quick actions on hover: play, watched and favorite without opening the page.
  const play = quickPlay();
  const actions = $derived(Boolean(href) && showMetadata && !work.synthetic);
  const canMarkWatched = $derived(work.kind !== 'series');
  let favorite = $state<boolean | null>(null);
  let busy = $state<'' | 'play' | 'watched' | 'favorite'>('');
  let actionError = $state('');
  function prime() { if (actions && favorite === null && $session) { favorite = false; void readFavorite(work.id).then(value => favorite = value).catch(() => {}); } }
  async function run(kind: 'play' | 'watched' | 'favorite', task: () => Promise<unknown>) {
    if (busy) return;
    busy = kind; actionError = '';
    try { await task(); } catch (cause) { actionError = cause instanceof Error ? cause.message : 'Action impossible.'; } finally { busy = ''; }
  }
  // eslint-disable-next-line svelte/no-navigation-without-resolve -- href is resolved by the caller.
  const launch = () => run('play', () => play ? play(work) : goto(href));
  const toggleWatched = () => run('watched', () => markWatched(work.id, !isWatched));
  const toggleFav = () => run('favorite', async () => { favorite = await toggleFavorite(work.id, $session?.csrfToken ?? ''); });
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

    {#if actions}<div class="card-hover-overlay" aria-hidden="true"></div>{/if}

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

{#if href && actions}
  <!-- Buttons cannot live inside the link: they sit above the poster, beside it. -->
  <div class="media-card has-actions" class:landscape={wide} role="group" aria-label={displayTitle} onpointerenter={prime} onfocusin={prime}>
    <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- URL resolved by caller. -->
    <a class="card-link" {href} aria-label={`Voir ${displayTitle}`}>{@render posterBody()}</a>
    <div class="quick-actions">
      <button type="button" class="quick-play" aria-label={`Lire ${displayTitle}`} title="Lire" disabled={!!busy || work.available === false} onclick={launch}>
        <Icon name="play" size={26} />
      </button>
      <div class="quick-row">
        {#if canMarkWatched}
          <button type="button" class="quick-button" class:on={isWatched} aria-pressed={isWatched} aria-label={isWatched ? 'Marquer non vu' : 'Marquer comme vu'} title={isWatched ? 'Marquer non vu' : 'Marquer comme vu'} disabled={!!busy} onclick={toggleWatched}>
            <Icon name="check" size={20} />
          </button>
        {/if}
        <button type="button" class="quick-button heart" class:on={favorite === true} aria-pressed={favorite === true} aria-label={favorite ? 'Retirer des favoris' : 'Ajouter aux favoris'} title={favorite ? 'Retirer des favoris' : 'Ajouter aux favoris'} disabled={!!busy} onclick={toggleFav}>
          <Icon name="heart" size={20} />
        </button>
      </div>
      {#if actionError}<p class="quick-error" role="alert">{actionError}</p>{/if}
    </div>
  </div>
{:else if href}
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
    aspect-ratio: var(--artwork-ratio, 2 / 3);
    position: relative;
    overflow: hidden;
    background: var(--pn-2);
    border-radius: var(--r-md);
    transition: transform var(--t-slow) var(--ease), box-shadow var(--t-slow) ease;
  }

  .landscape .poster { aspect-ratio: var(--artwork-ratio, 16 / 9); }

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

  .card-link { display: block; color: inherit; }

  @media (hover: hover) and (pointer: fine) {
    a.media-card:hover .poster,
    .has-actions:hover .poster,
    .has-actions:hover .quick-actions {
      transform: translateY(-4px);
    }
    a.media-card:hover .poster,
    .has-actions:hover .poster { box-shadow: var(--shadow); }
  }

  a.media-card:focus-visible, .card-link:focus-visible { outline: none; }
  a.media-card:focus-visible .poster,
  .card-link:focus-visible .poster {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  /* Shade + quick actions, shown on hover or keyboard focus (Jellyfin-like). */
  .card-hover-overlay {
    position: absolute;
    inset: 0;
    background: radial-gradient(circle at 50% 50%, rgb(0 0 0 / 0.25), rgb(0 0 0 / 0.55));
    opacity: 0;
    transition: opacity var(--t-med) ease;
    pointer-events: none;
  }

  .quick-actions {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    aspect-ratio: var(--artwork-ratio, 2 / 3);
    display: grid;
    place-items: center;
    pointer-events: none;
    opacity: 0;
    transition: opacity var(--t-med) ease, transform var(--t-slow) var(--ease);
  }
  .landscape .quick-actions { aspect-ratio: var(--artwork-ratio, 16 / 9); }

  .has-actions:hover .card-hover-overlay,
  .has-actions:focus-within .card-hover-overlay,
  .has-actions:hover .quick-actions,
  .has-actions:focus-within .quick-actions { opacity: 1; }
  /* Touch screens keep the plain card: a tap opens the page. */
  @media (hover: none) { .has-actions:not(:focus-within) .quick-actions { display: none; } }

  .quick-actions button {
    pointer-events: auto;
    display: grid;
    place-items: center;
    border: 0;
    color: #fff;
    cursor: pointer;
    transition: transform var(--t-fast) var(--ease), background-color var(--t-fast) ease, color var(--t-fast) ease;
  }
  .quick-actions button:disabled { opacity: 0.5; cursor: wait; }
  .quick-actions button:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }

  .quick-actions .quick-play {
    width: 56px;
    height: 56px;
    border-radius: 50%;
    background: rgb(12 14 20 / 0.55);
    border: 1px solid rgb(255 255 255 / 0.25);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    padding-left: 3px;
  }
  .quick-actions .quick-play:hover:not(:disabled) { transform: scale(1.08); background: var(--ac); color: var(--on-ac); }

  .quick-row {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 18px;
    display: flex;
    justify-content: center;
    gap: 14px;
  }
  .quick-button {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    background: rgb(12 14 20 / 0.45);
    filter: drop-shadow(0 1px 3px rgb(0 0 0 / 0.6));
  }
  .quick-button:hover:not(:disabled) { transform: scale(1.1); background: rgb(12 14 20 / 0.7); }
  .quick-button.on { color: var(--ac); }
  .quick-button.heart.on :global(svg) { fill: currentColor; }

  .quick-error {
    position: absolute;
    left: 8px;
    right: 8px;
    top: 8px;
    margin: 0;
    padding: 4px 8px;
    border-radius: var(--r-xs);
    background: rgb(0 0 0 / 0.75);
    color: #fff;
    font-size: 12px;
    text-align: center;
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
