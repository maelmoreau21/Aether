<script lang="ts">
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import { type Work, localizedTitle } from './api';
  import { locale, t } from '../i18n';
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
  import MetadataEditorModal from './MetadataEditorModal.svelte';

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
  const isAdmin = $derived($session?.user.role === 'administrator');

  let favorite = $state<boolean | null>(null);
  let busy = $state<'' | 'play' | 'watched' | 'favorite' | 'playlist'>('');
  let actionError = $state('');
  let menuOpen = $state(false);
  let metadataOpen = $state(false);
  let playlists = $state<{ id: string; name: string }[]>([]);
  let showPlaylists = $state(false);
  let playlistNotice = $state('');

  function prime() {
    if (actions && favorite === null && $session) {
      favorite = false;
      void readFavorite(work.id).then(value => (favorite = value)).catch(() => {});
    }
  }

  async function run(kind: 'play' | 'watched' | 'favorite' | 'playlist', task: () => Promise<unknown>) {
    if (busy) return;
    busy = kind;
    actionError = '';
    try {
      await task();
    } catch (cause) {
      actionError = cause instanceof Error ? cause.message : $t('action_impossible');
    } finally {
      busy = '';
    }
  }

  // eslint-disable-next-line svelte/no-navigation-without-resolve -- href is resolved by the caller.
  const launch = () => run('play', () => (play ? play(work) : goto(href)));
  const toggleWatched = () => run('watched', () => markWatched(work.id, !isWatched));
  const toggleFav = () =>
    run('favorite', async () => {
      favorite = await toggleFavorite(work.id, $session?.csrfToken ?? '');
    });

  async function loadPlaylists() {
    try {
      const response = await fetch('/api/v1/lists');
      if (response.ok) {
        const data = await response.json();
        playlists = (data.lists ?? []).filter((list: { owned?: boolean }) => list.owned !== false);
      }
    } catch {}
  }

  async function addToPlaylist(listId: string, listName: string) {
    await run('playlist', async () => {
      const response = await fetch(`/api/v1/lists/${encodeURIComponent(listId)}/items/${encodeURIComponent(work.id)}`, {
        method: 'PUT',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': $session?.csrfToken ?? ''
        }
      });
      if (!response.ok) throw new Error($t('saving_failed'));
      playlistNotice = $t('added_to_playlist', { name: listName });
      setTimeout(() => {
        menuOpen = false;
        showPlaylists = false;
        playlistNotice = '';
      }, 1000);
    });
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && menuOpen) {
      menuOpen = false;
      showPlaylists = false;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#snippet posterImage()}
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
      <span class="watched-badge" title={$t('already_watched')}>
        <Icon name="check" size={14} />
        <span class="sr-only">{$t('already_watched')}</span>
      </span>
    {/if}
    {#if work.available === false}
      <span class="offline-badge">{$t('unavailable')}</span>
    {/if}
    {#if progressRatio > 0}
      <div
        class="poster-progress"
        role="progressbar"
        aria-label={$t('progress_of_2', { displayTitle })}
        aria-valuenow={Math.round(progressRatio)}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <span style:width={`${progressRatio}%`}></span>
      </div>
    {/if}
  </div>
{/snippet}

{#snippet metadataContent()}
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
  <!-- Buttons cannot live inside the link: poster-frame houses the link and the overlay buttons. -->
  <div
    class="card-shell has-actions"
    class:landscape={wide}
    class:has-menu-open={menuOpen}
    role="group"
    aria-label={displayTitle}
    onpointerenter={prime}
    onfocusin={prime}
  >
    <div class="poster-frame">
      <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- URL resolved by caller. -->
      <a class="poster-link" {href} aria-label={$t('catalog_view', { displayTitle })}>
        {@render posterImage()}
      </a>

      <div class="quick-actions" class:menu-active={menuOpen}>
        <button
          type="button"
          class="quick-play"
          aria-label={$t('catalog_play', { displayTitle })}
          title={$t('play_action')}
          disabled={!!busy || work.available === false}
          onclick={(e) => {
            e.stopPropagation();
            e.preventDefault();
            launch();
          }}
        >
          <Icon name="play" size={28} />
        </button>

        <div class="quick-row">
          {#if canMarkWatched}
            <button
              type="button"
              class="quick-button"
              class:on={isWatched}
              aria-pressed={isWatched}
              aria-label={isWatched ? $t('mark_as_unwatched') : $t('mark_as_watched')}
              title={isWatched ? $t('mark_as_unwatched') : $t('mark_as_watched')}
              disabled={!!busy}
              onclick={(e) => {
                e.stopPropagation();
                e.preventDefault();
                toggleWatched();
              }}
            >
              <Icon name="check" size={18} />
            </button>
          {/if}

          <button
            type="button"
            class="quick-button heart"
            class:on={favorite === true}
            aria-pressed={favorite === true}
            aria-label={favorite ? $t('remove_from_favourites') : $t('add_to_favourites')}
            title={favorite ? $t('remove_from_favourites') : $t('add_to_favourites')}
            disabled={!!busy}
            onclick={(e) => {
              e.stopPropagation();
              e.preventDefault();
              toggleFav();
            }}
          >
            <Icon name="heart" size={18} />
          </button>

          <button
            type="button"
            class="quick-button more"
            class:on={menuOpen}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            aria-label={$t('more_actions')}
            title={$t('more_actions')}
            onclick={(e) => {
              e.stopPropagation();
              e.preventDefault();
              menuOpen = !menuOpen;
              if (menuOpen) void loadPlaylists();
            }}
          >
            <Icon name="more-vertical" size={18} />
          </button>
        </div>

        {#if actionError}<p class="quick-error" role="alert">{actionError}</p>{/if}

        {#if menuOpen}
          <button
            type="button"
            class="menu-backdrop"
            aria-label={$t('close_menu')}
            tabindex="-1"
            onclick={(e) => {
              e.stopPropagation();
              e.preventDefault();
              menuOpen = false;
              showPlaylists = false;
            }}
          ></button>

          <div class="card-dropdown-menu" role="menu">
            <button
              type="button"
              class="menu-item"
              role="menuitem"
              onclick={(e) => {
                e.stopPropagation();
                e.preventDefault();
                menuOpen = false;
                launch();
              }}
            >
              <Icon name="play" size={16} />
              <span>{$t('play_action')}</span>
            </button>

            {#if canMarkWatched}
              <button
                type="button"
                class="menu-item"
                role="menuitem"
                onclick={(e) => {
                  e.stopPropagation();
                  e.preventDefault();
                  menuOpen = false;
                  toggleWatched();
                }}
              >
                <Icon name="check" size={16} />
                <span>{isWatched ? $t('mark_as_unwatched') : $t('mark_as_watched')}</span>
              </button>
            {/if}

            <button
              type="button"
              class="menu-item"
              role="menuitem"
              onclick={(e) => {
                e.stopPropagation();
                e.preventDefault();
                menuOpen = false;
                toggleFav();
              }}
            >
              <Icon name="heart" size={16} />
              <span>{favorite ? $t('remove_from_favourites') : $t('add_to_favourites')}</span>
            </button>

            <button
              type="button"
              class="menu-item"
              role="menuitem"
              onclick={(e) => {
                e.stopPropagation();
                e.preventDefault();
                showPlaylists = !showPlaylists;
                if (showPlaylists) void loadPlaylists();
              }}
            >
              <Icon name="playlist" size={16} />
              <span>{$t('add_to_playlist')}</span>
            </button>

            {#if showPlaylists}
              <div class="playlist-sublist">
                {#each playlists as list (list.id)}
                  <button
                    type="button"
                    class="menu-subitem"
                    disabled={!!busy}
                    onclick={(e) => {
                      e.stopPropagation();
                      e.preventDefault();
                      void addToPlaylist(list.id, list.name);
                    }}
                  >
                    <span>{list.name}</span>
                  </button>
                {:else}
                  <p class="empty-playlists">{$t('no_playlists')}</p>
                {/each}
                {#if playlistNotice}
                  <p class="playlist-notice">{playlistNotice}</p>
                {/if}
              </div>
            {/if}

            <button
              type="button"
              class="menu-item"
              role="menuitem"
              onclick={(e) => {
                e.stopPropagation();
                e.preventDefault();
                menuOpen = false;
                goto(href);
              }}
            >
              <Icon name="info" size={16} />
              <span>{$t('media_information')}</span>
            </button>

            {#if isAdmin}
              <div class="menu-divider" role="separator"></div>
              <button
                type="button"
                class="menu-item"
                role="menuitem"
                onclick={(e) => {
                  e.stopPropagation();
                  e.preventDefault();
                  menuOpen = false;
                  metadataOpen = true;
                }}
              >
                <Icon name="edit" size={16} />
                <span>{$t('edit_metadata')}</span>
              </button>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    {#if showMetadata}
      <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- URL resolved by caller. -->
      <a class="metadata-link" {href} aria-label={$t('catalog_view', { displayTitle })}>
        {@render metadataContent()}
      </a>
    {/if}
  </div>

  {#if metadataOpen && isAdmin}
    <MetadataEditorModal
      {work}
      onclose={() => (metadataOpen = false)}
      onsaved={() => {
        metadataOpen = false;
        window.dispatchEvent(new CustomEvent('media-artwork-updated', { detail: { id: work.id } }));
      }}
    />
  {/if}
{:else if href}
  <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- URL resolved by caller. -->
  <a class="media-card" class:landscape={wide} {href} aria-label={$t('catalog_view', { displayTitle })}>
    {@render posterImage()}
    {@render metadataContent()}
  </a>
{:else}
  <div class="media-card" class:landscape={wide}>
    {@render posterImage()}
    {@render metadataContent()}
  </div>
{/if}

<style>
  .media-card {
    display: block;
    min-width: 0;
    color: inherit;
    position: relative;
    text-decoration: none;
  }

  .poster {
    aspect-ratio: var(--artwork-ratio, 2 / 3);
    position: relative;
    overflow: hidden;
    background: var(--pn-2);
    border-radius: var(--r-md);
  }

  .landscape .poster {
    aspect-ratio: var(--artwork-ratio, 16 / 9);
  }

  .poster img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity var(--t-med) ease;
  }

  .poster img.loaded {
    opacity: 1;
  }

  .card-shell {
    position: relative;
    min-width: 0;
  }

  .card-shell.has-menu-open {
    z-index: 30;
  }

  .poster-frame {
    position: relative;
    border-radius: var(--r-md);
    transition: transform var(--t-slow) var(--ease), box-shadow var(--t-slow) ease;
  }

  .poster-link {
    display: block;
    color: inherit;
    text-decoration: none;
    border-radius: var(--r-md);
  }

  .metadata-link {
    display: block;
    color: inherit;
    text-decoration: none;
  }

  @media (hover: hover) and (pointer: fine) {
    a.media-card:hover .poster,
    .has-actions:hover .poster-frame,
    .has-actions.has-menu-open .poster-frame {
      transform: translateY(-4px);
      box-shadow: var(--shadow);
    }
  }

  a.media-card:focus-visible,
  .poster-link:focus-visible,
  .metadata-link:focus-visible {
    outline: none;
  }

  a.media-card:focus-visible .poster,
  .poster-link:focus-visible .poster {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  .metadata-link:focus-visible h3 {
    color: var(--ac);
  }

  /* Shade on hover: Jellyfin-like dark overlay */
  .card-hover-overlay {
    position: absolute;
    inset: 0;
    background: radial-gradient(circle at 50% 50%, rgb(0 0 0 / 0.35), rgb(0 0 0 / 0.65));
    opacity: 0;
    transition: opacity var(--t-med) ease;
    pointer-events: none;
  }

  /* Quick actions container: sits directly over the poster */
  .quick-actions {
    position: absolute;
    inset: 0;
    pointer-events: none;
    opacity: 0;
    transition: opacity var(--t-med) ease;
    z-index: 3;
  }

  .has-actions:hover .card-hover-overlay,
  .has-actions:focus-within .card-hover-overlay,
  .has-actions.has-menu-open .card-hover-overlay,
  .has-actions:hover .quick-actions,
  .has-actions:focus-within .quick-actions,
  .quick-actions.menu-active {
    opacity: 1;
  }

  /* Touch screens: plain card, tap opens the page */
  @media (hover: none) {
    .has-actions:not(:focus-within):not(.has-menu-open) .quick-actions {
      display: none;
    }
  }

  /* Centered Play Button */
  .quick-play {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 60px;
    height: 60px;
    border-radius: 50%;
    background: rgb(0 0 0 / 0.65);
    border: 1px solid rgb(255 255 255 / 0.25);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    color: #fff;
    cursor: pointer;
    pointer-events: auto;
    display: grid;
    place-items: center;
    padding-left: 3px;
    box-shadow: 0 4px 20px rgb(0 0 0 / 0.5);
    transition: transform var(--t-fast) var(--ease), background var(--t-fast) ease, color var(--t-fast) ease, border-color var(--t-fast) ease;
  }

  .quick-play:hover:not(:disabled) {
    transform: translate(-50%, -50%) scale(1.1);
    background: var(--ac);
    color: var(--on-ac);
    border-color: var(--ac);
  }

  .quick-play:disabled {
    opacity: 0.5;
    cursor: wait;
  }

  .quick-play:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  /* Bottom Right Actions Row */
  .quick-row {
    position: absolute;
    right: 10px;
    bottom: 10px;
    display: flex;
    align-items: center;
    gap: 8px;
    pointer-events: none;
    z-index: 4;
  }

  .quick-button {
    pointer-events: auto;
    width: 34px;
    height: 34px;
    border-radius: 50%;
    background: rgb(0 0 0 / 0.55);
    border: 1px solid rgb(255 255 255 / 0.2);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    color: #fff;
    cursor: pointer;
    display: grid;
    place-items: center;
    filter: drop-shadow(0 2px 4px rgb(0 0 0 / 0.7));
    transition: transform var(--t-fast) var(--ease), background var(--t-fast) ease, color var(--t-fast) ease, border-color var(--t-fast) ease;
  }

  .quick-button:hover:not(:disabled) {
    transform: scale(1.12);
    background: rgb(0 0 0 / 0.85);
    border-color: rgb(255 255 255 / 0.4);
  }

  .quick-button:disabled {
    opacity: 0.5;
    cursor: wait;
  }

  .quick-button:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 2px;
  }

  .quick-button.on {
    color: var(--ac);
    border-color: var(--ac);
  }

  .quick-button.heart.on :global(svg) {
    fill: currentColor;
  }

  .quick-button.more.on {
    background: rgb(0 0 0 / 0.9);
    color: var(--ac);
    border-color: var(--ac);
  }

  /* Backdrop to close menu when clicking outside */
  .menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    background: transparent;
    cursor: default;
    border: 0;
    padding: 0;
    pointer-events: auto;
  }

  /* Dropdown Menu for 3-dots */
  .card-dropdown-menu {
    position: absolute;
    right: 8px;
    bottom: 50px;
    min-width: 200px;
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.65);
    padding: 6px;
    z-index: 25;
    pointer-events: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    animation: menu-appear var(--t-fast) var(--ease);
  }

  @keyframes menu-appear {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 12px;
    border-radius: var(--r-xs);
    border: 0;
    background: transparent;
    color: var(--tx);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--t-fast) ease, color var(--t-fast) ease;
  }

  .menu-item:hover {
    background: var(--pn);
    color: var(--ac);
  }

  .menu-item:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: -2px;
  }

  .menu-divider {
    height: 1px;
    background: var(--line);
    margin: 4px 6px;
  }

  .playlist-sublist {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-left: 14px;
    max-height: 140px;
    overflow-y: auto;
    border-left: 2px solid var(--line);
    margin: 4px 8px;
  }

  .menu-subitem {
    display: block;
    width: 100%;
    padding: 6px 10px;
    border: 0;
    background: transparent;
    color: var(--tx);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    text-align: left;
    border-radius: var(--r-xs);
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: background-color var(--t-fast) ease, color var(--t-fast) ease;
  }

  .menu-subitem:hover:not(:disabled) {
    background: var(--pn);
    color: var(--ac);
  }

  .empty-playlists {
    margin: 0;
    padding: 6px 10px;
    font-size: 12px;
    color: var(--mu);
    font-style: italic;
  }

  .playlist-notice {
    margin: 0;
    padding: 4px 10px;
    font-size: 11px;
    color: var(--ac);
    font-weight: 600;
  }

  .quick-error {
    position: absolute;
    left: 8px;
    right: 8px;
    top: 8px;
    margin: 0;
    padding: 4px 8px;
    border-radius: var(--r-xs);
    background: rgb(0 0 0 / 0.85);
    color: #ff6b6b;
    font-size: 12px;
    text-align: center;
    pointer-events: none;
    z-index: 6;
  }

  /* Repli typographique quand folder.jpg manque */
  .poster-fallback {
    display: flex;
    height: 100%;
    flex-direction: column;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    padding: 14px;
    color: var(--mu);
    background: var(--bg-2);
    text-align: center;
  }

  .poster-fallback span {
    font-size: 15px;
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
    text-align: center;
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

  /* Centered metadata matching the user screenshot */
  .metadata-block {
    padding-top: 10px;
    text-align: center;
  }

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
    text-align: center;
    transition: color var(--t-fast) ease;
  }

  .has-actions:hover h3,
  .metadata-link:hover h3 {
    color: var(--ac);
  }

  p {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px;
    margin: 3px 0 0;
    color: var(--mu);
    font-size: 13px;
    text-align: center;
  }

  .rating {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--ac);
    font-weight: 800;
  }
</style>
