<script lang="ts">
  import { resolve } from '$app/paths';
  import type { Work } from '../catalog/api';
  import Icon from './Icon.svelte';
  import { imagePreviewUrl, isImageLoaded, markImageLoaded } from '../catalog/image-url';

  

  let {
    featured = null,
    onPlay,
    onRandomize
  }: {
    featured?: Work | null;
    onPlay?: (work: Work) => void;
    onRandomize?: () => void;
  } = $props();

  const detailUrl = $derived(
    featured
      ? featured.kind === 'series'
        ? resolve(`series/?work=${encodeURIComponent(featured.id)}`)
        : resolve(`films/?work=${encodeURIComponent(featured.id)}`)
      : resolve('films/')
  );

  const imageUrl = $derived(imagePreviewUrl(featured?.artwork?.backdrop,1280));
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
</script>

<section class="hero-banner" class:has-featured={!!featured} aria-label="Média en vedette">
  <!-- Backdrop Background -->
  <div class="backdrop-wrapper" aria-hidden="true">
    <div
      class="backdrop-art"
      style:background-image="radial-gradient(ellipse at 75% 40%, var(--color-accent-soft), transparent 65%)"
    ></div>
    {#if featured?.artwork?.backdrop}
      <img
        class="backdrop-img"
        class:loaded={loadedUrl === imageUrl}
        src={imageUrl}
        alt=""
        loading="eager"
        fetchpriority="high"
        decoding="async"
        use:checkLoaded
        onload={() => {
          loadedUrl = imageUrl;
          markImageLoaded(imageUrl);
        }}
      />
    {/if}
    <div class="backdrop-gradient-vignette"></div>
    <div class="backdrop-gradient-bottom"></div>
  </div>

  <div class="hero-content">
    <div class="hero-meta-row">
      <span class="hero-eyebrow">
        {#if featured}
          <Icon name="sparkles" size={12} />
          <span>{featured.kind === 'series' ? 'SÉRIE DU MOMENT' : 'FILM EN VEDETTE'}</span>
        {:else}
          <span>LE CINÉMA, CHEZ VOUS</span>
        {/if}
      </span>

      {#if featured?.year}
        <span class="meta-pill">{featured.year}</span>
      {/if}
      {#if featured?.rating}
        <span class="meta-pill rating-pill">
          <Icon name="star" size={12} />
          {featured.rating.toFixed(1)}
        </span>
      {/if}
      {#if featured?.mpaa}
        <span class="meta-pill mpaa-pill">{featured.mpaa}</span>
      {/if}
      {#if featured?.kind === 'series'}
        <span class="meta-pill">
          {featured.seasonCount ?? 0} {(featured.seasonCount ?? 0) > 1 ? 'saisons' : 'saison'}
        </span>
      {/if}
    </div>

    <h1 class="hero-title">
      {#if featured}
        {featured.title}
      {:else}
        Une envie de cinéma.<br />
        <span class="hero-title-accent">Une place pour chaque histoire.</span>
      {/if}
    </h1>

    {#if featured?.outline}
      <p class="hero-outline">« {featured.outline} »</p>
    {/if}

    <p class="hero-synopsis">
      {#if featured}
        {featured.plot || featured.description || ''}
      {:else}
        Retrouvez vos films préférés et explorez vos séries en lecture directe haute fidélité. Sans transcodage forcé, vos médias sont diffusés dans leur format d’origine.
      {/if}
    </p>

    <div class="hero-actions">
      {#if featured}
        {#if onPlay}
          <button
            type="button"
            class="btn-primary"
            onclick={() => onPlay(featured)}
            disabled={featured.available === false}
          >
            <Icon name="play" size={20} />
            <span>Lire</span>
          </button>
        {:else}
          <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- detailUrl already resolved -->
          <a class="btn-primary" href={detailUrl}>
            <Icon name="play" size={20} />
            <span>Lire</span>
          </a>
        {/if}

        <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- detailUrl already resolved -->
        <a class="btn-secondary" href={detailUrl}>
          <Icon name="info" size={20} />
          <span>Plus d'infos</span>
        </a>

        {#if onRandomize}
          <button
            type="button"
            class="btn-tertiary"
            onclick={onRandomize}
            title="Découvrir un autre titre au hasard"
            aria-label="Découvrir un autre titre au hasard"
          >
            <Icon name="sparkles" size={18} />
            <span>Surprenez-moi</span>
          </button>
        {/if}
      {:else}
        <a class="btn-primary" href={resolve('films/')}>
          <Icon name="film" size={18} />
          <span>Explorer les films</span>
        </a>
        <a class="btn-secondary" href={resolve('series/')}>
          <Icon name="tv" size={18} />
          <span>Voir les séries</span>
        </a>
      {/if}
    </div>
  </div>
</section>

<style>
  .hero-banner {
    position: relative;
    min-height: clamp(350px, 30vw, 480px);
    display: flex;
    align-items: flex-end;
    padding: clamp(24px, 4vw, 48px);
    margin: 24px 0 0;
    border-radius: var(--radius-2xl);
    overflow: hidden;
  }

  .backdrop-wrapper {
    position: absolute;
    inset: 0;
    z-index: 1;
    overflow: hidden;
  }

  .backdrop-art {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    background-size: cover;
    background-position: center 48%;
    filter: brightness(0.6) saturate(0.9);
  }

  .backdrop-img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center 25%;
    filter: brightness(0.65) saturate(1.1);
    transform: scale(1.02);
    opacity: 0;
    transition: opacity 0.5s ease-out;
  }

  .backdrop-img.loaded {
    opacity: 1;
  }

  .backdrop-gradient-vignette {
    position: absolute;
    inset: 0;
    background: radial-gradient(circle at 75% 30%, transparent 20%, color-mix(in srgb, var(--bg-main) 70%, transparent) 70%, var(--bg-main) 100%),
                linear-gradient(90deg, var(--bg-main) 0%, color-mix(in srgb, var(--bg-main) 85%, transparent) 45%, transparent 100%);
  }

  .backdrop-gradient-bottom {
    position: absolute;
    inset: auto 0 0 0;
    height: 65%;
    background: linear-gradient(180deg, transparent 0%, color-mix(in srgb, var(--bg-main) 85%, transparent) 60%, var(--bg-main) 100%);
  }

  .hero-content {
    position: relative;
    z-index: 2;
    max-width: 680px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .hero-meta-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }

  .hero-eyebrow {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    text-transform: uppercase;
    font-size: 11px;
    font-weight: 750;
    letter-spacing: 0.06em;
    color: var(--color-accent);
    background: var(--color-accent-soft);
    border: 1px solid var(--border-subtle);
    padding: 4px 10px;
    border-radius: var(--radius-pill);
    backdrop-filter: blur(8px);
  }

  .meta-pill {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-text);
    background: rgba(255, 255, 255, 0.08);
    padding: 3px 8px;
    border-radius: var(--radius-xs);
    backdrop-filter: blur(8px);
  }

  .rating-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--accent-secondary);
    background: rgba(251, 191, 36, 0.15);
    border: 1px solid rgba(251, 191, 36, 0.25);
    backdrop-filter: blur(8px);
  }

  .mpaa-pill {
    border: 1px solid rgba(255, 255, 255, 0.2);
  }

  .hero-title {
    font-family: var(--font-display);
    font-size: clamp(32px, 4.5vw, 56px);
    font-weight: 750;
    line-height: 1.1;
    letter-spacing: -0.04em;
    color: var(--color-text-bright);
    margin: 0;
    text-shadow: 0 2px 12px color-mix(in srgb, var(--bg-main) 70%, transparent);
  }

  .hero-title-accent {
    font-style: italic;
    font-weight: 500;
    background: linear-gradient(135deg, var(--text-primary) 40%, var(--color-accent) 100%);
    background-clip: text;
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
  }

  .hero-outline {
    font-size: 15px;
    font-style: italic;
    color: var(--color-accent-hover);
    margin: 0;
    opacity: 0.95;
  }

  .hero-synopsis {
    font-size: clamp(13.5px, 1.2vw, 15px);
    line-height: 1.6;
    color: var(--text-secondary);
    margin: 0;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
    text-shadow: none;
  }

  .hero-actions {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 8px;
    flex-wrap: wrap;
  }

  .btn-primary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    min-height: 48px;
    padding: 0 26px;
    border: 0;
    border-radius: var(--radius-md);
    background: var(--color-play);
    color: var(--color-on-play);
    font-size: 14px;
    font-weight: 700;
    text-decoration: none;
    box-shadow: var(--shadow-sm);
    transition: all var(--transition-fast);
  }

  .btn-primary:hover:not(:disabled) {
    background: var(--color-play);
    transform: translateY(-2px);
    box-shadow: var(--shadow-sm);
  }

  .btn-secondary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    min-height: 48px;
    padding: 0 22px;
    border: 1px solid rgba(255, 255, 255, 0.16);
    border-radius: var(--radius-md);
    background: rgba(255, 255, 255, 0.08);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    color: var(--color-text-bright);
    font-size: 14px;
    font-weight: 600;
    text-decoration: none;
    transition: all var(--transition-fast);
  }

  .btn-secondary:hover {
    background: rgba(255, 255, 255, 0.16);
    border-color: rgba(255, 255, 255, 0.3);
    transform: translateY(-2px);
  }

  .btn-tertiary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 48px;
    padding: 0 20px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: rgba(255, 255, 255, 0.05);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    color: var(--color-accent);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-tertiary:hover {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
    color: var(--color-accent-hover);
    transform: translateY(-2px);
  }

  @media (max-width: 768px) {
    .hero-banner {
      min-height: 360px;
      padding: 24px;
      margin-bottom: 16px;
    }

    .hero-actions {
      width: 100%;
    }

    .btn-primary, .btn-secondary, .btn-tertiary {
      flex: 1;
      min-height: 44px;
      padding: 0 14px;
      font-size: 13px;
    }
  }
</style>