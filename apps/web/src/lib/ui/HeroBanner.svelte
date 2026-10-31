<script lang="ts">
  import { session } from '../auth/session';
  import { selectArtwork } from '../catalog/artwork-selection';
  import { resolve } from '$app/paths';
  import { type Work, localizedTitle, localizedPlot } from '../catalog/api';
  import { locale } from '../i18n';
  import Icon from './Icon.svelte';
  import { imagePreviewUrl } from '../catalog/image-url';
  import { history } from '../catalog/watch-state';

  let { featured = null, onPlay }: { featured?: Work | null; onPlay?: (work: Work) => void } = $props();
  const detailUrl = $derived(
    featured
      ? featured.kind === 'series'
        ? resolve(`series/?work=${encodeURIComponent(featured.id)}`)
        : resolve(`films/?work=${encodeURIComponent(featured.id)}`)
      : resolve('films/')
  );
  const artwork = $derived(selectArtwork(featured, $session));
  const imageUrl = $derived(imagePreviewUrl(artwork.backdrop, 1280));
  const logoUrl = $derived(imagePreviewUrl(artwork.logo, 640));
  let failedBackdrop = $state('');
  let failedLogo = $state('');
  const continuing = $derived($history.some(item => item.workId === featured?.id && !item.watched && item.position > 0));
  const displayTitle = $derived(featured ? localizedTitle(featured, $locale) : '');
  const displayPlot = $derived(featured ? localizedPlot(featured, $locale) : '');
</script>

<section class="hero-banner" class:has-image={!!imageUrl && failedBackdrop !== imageUrl} aria-label="À l’affiche dans votre médiathèque">
  {#if imageUrl && failedBackdrop !== imageUrl}
    <img class="backdrop-img" src={imageUrl} alt="" width="1280" height="720" loading="eager" fetchpriority="high" decoding="async" onerror={() => failedBackdrop = imageUrl} />
  {/if}
  <div class="hero-content">
    <p class="hero-eyebrow">{featured ? 'À L’AFFICHE DANS VOTRE MÉDIATHÈQUE' : 'VOTRE MÉDIATHÈQUE'}</p>
    <h1>
      {#if featured}
        {#if logoUrl && failedLogo !== logoUrl}<img class="hero-title-logo" src={logoUrl} alt={displayTitle} width="440" height="150" decoding="async" onerror={() => failedLogo = logoUrl} />{:else}{displayTitle}{/if}
      {:else}Vos histoires commencent ici.{/if}
    </h1>
    {#if featured}
      <div class="hero-meta">
        <span class="meta-badge kind-badge">{featured.kind === 'series' ? 'Série' : 'Film'}</span>
        {#if featured.year}<span class="meta-item">{featured.year}</span>{/if}
        {#if featured.mpaa}<span class="meta-badge age-badge">{featured.mpaa}</span>{/if}
        {#if featured.rating}<span class="meta-badge rating-badge"><Icon name="star" size={12} /> {featured.rating.toFixed(1)}</span>{/if}
        {#if featured.kind === 'series' && featured.seasonCount}<span class="meta-item">{featured.seasonCount} {featured.seasonCount > 1 ? 'saisons' : 'saison'}</span>{/if}
        {#if featured.genres?.length}<span class="meta-item meta-genres">{featured.genres.slice(0, 3).join(' · ')}</span>{/if}
      </div>
      {#if displayPlot}<p class="hero-synopsis">{displayPlot}</p>{/if}
      {#if featured.available === false}<p class="availability" role="status">Ce contenu est temporairement indisponible.</p>{/if}
    {:else}<p class="hero-synopsis">Les films et séries de vos bibliothèques apparaîtront ici une fois ajoutés au catalogue.</p>{/if}
    <div class="hero-actions">
      {#if featured}
        {#if onPlay}<button class="primary" onclick={() => onPlay(featured)} disabled={featured.available === false}><Icon name="play" size={20} /><span>{featured.kind === 'series' ? 'Voir les épisodes' : continuing ? 'Reprendre' : 'Lire'}</span></button>{/if}
        <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- URL resolved above. -->
        <a class="secondary" href={detailUrl}><Icon name="info" size={20} /><span>Plus d’infos</span></a>
      {:else}
        <a class="secondary" href={resolve('films/')}><Icon name="film" size={20} /><span>Explorer les films</span></a>
      {/if}
    </div>
  </div>
</section>

<style>
  .hero-banner {
    position: relative;
    isolation: isolate;
    display: flex;
    align-items: flex-end;
    min-height: 380px;
    margin-top: 20px;
    padding: clamp(28px, 4.5vw, 56px);
    border-radius: var(--radius-2xl);
    border: 1px solid var(--color-line);
    overflow: hidden;
    background: radial-gradient(circle at 80% 20%, rgba(249, 115, 22, 0.14), transparent 55%), var(--color-surface);
    box-shadow: var(--shadow-lg);
  }

  .hero-banner.has-image {
    min-height: clamp(430px, 42vw, 580px);
    background: var(--color-canvas);
  }

  .backdrop-img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: 70% 25%;
    z-index: -2;
    transform: scale(1.015);
    transition: transform 8s ease-out;
  }

  .hero-banner:hover .backdrop-img {
    transform: scale(1.04);
  }

  .has-image::after {
    content: '';
    position: absolute;
    inset: 0;
    z-index: -1;
    background:
      linear-gradient(90deg, var(--color-canvas) 0%, color-mix(in srgb, var(--color-canvas) 92%, transparent) 36%, color-mix(in srgb, var(--color-canvas) 35%, transparent) 72%, transparent 100%),
      linear-gradient(0deg, var(--color-canvas) 4%, color-mix(in srgb, var(--color-canvas) 75%, transparent) 38%, transparent 85%);
  }

  .hero-content {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 16px;
    max-width: 780px;
    min-width: 0;
  }

  .hero-eyebrow {
    margin: 0;
    font-size: 11px;
    font-weight: 750;
    color: var(--color-accent);
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: clamp(32px, 4.2vw, 60px);
    font-weight: 800;
    line-height: 1.06;
    letter-spacing: -0.035em;
    color: var(--color-text-bright);
    text-shadow: 0 4px 24px rgba(0, 0, 0, 0.55);
    overflow-wrap: anywhere;
  }

  .hero-title-logo {
    display: block;
    width: auto;
    height: auto;
    max-width: min(100%, 400px);
    max-height: 135px;
    object-fit: contain;
    object-position: left;
    filter: drop-shadow(0 6px 20px rgba(0, 0, 0, 0.65));
  }

  .hero-meta {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    color: var(--color-muted);
    font-size: 13px;
    font-weight: 500;
  }

  .meta-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 9px;
    border-radius: var(--radius-xs);
    font-size: 11.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
  }

  .kind-badge {
    background: var(--color-accent-soft);
    color: var(--color-text-bright);
    border: 1px solid rgba(249, 115, 22, 0.35);
    text-transform: uppercase;
  }

  .age-badge {
    background: rgba(255, 255, 255, 0.08);
    color: var(--color-text-bright);
    border: 1px solid var(--color-line-strong);
  }

  .rating-badge {
    background: rgba(245, 158, 11, 0.14);
    color: #FBBF24;
    border: 1px solid rgba(245, 158, 11, 0.3);
  }

  .meta-item {
    color: var(--color-text);
  }

  .meta-genres {
    color: var(--color-muted);
  }

  .hero-synopsis {
    margin: 0;
    max-width: 56ch;
    font-size: 15px;
    line-height: 1.65;
    color: var(--color-text);
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
  }

  .availability {
    margin: 0;
    color: var(--color-danger);
    font-size: 14px;
    font-weight: 500;
  }

  .hero-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    margin-top: 6px;
  }

  .hero-actions a,
  .hero-actions button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    min-height: 48px;
    padding: 12px 26px;
    border-radius: var(--radius-md);
    font-size: 14.5px;
    font-weight: 700;
    transition: transform var(--transition-fast), box-shadow var(--transition-fast), background var(--transition-fast), border-color var(--transition-fast);
  }

  .primary {
    background: var(--color-play);
    color: var(--color-on-play);
    border: 1px solid transparent;
    box-shadow: 0 6px 22px rgba(249, 115, 22, 0.38);
  }

  .primary:hover:not(:disabled) {
    background: var(--color-play-hover);
    transform: translateY(-2px);
    box-shadow: 0 10px 28px rgba(249, 115, 22, 0.52);
  }

  .secondary {
    background: var(--color-glass-raised);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    color: var(--color-text-bright);
    border: 1px solid var(--color-line-strong);
  }

  .secondary:hover {
    background: var(--color-raised-hover);
    border-color: var(--color-text);
    transform: translateY(-2px);
  }

  @media (min-width: 1920px) {
    .hero-banner.has-image {
      min-height: 620px;
    }
    .hero-content {
      max-width: 960px;
    }
    .hero-synopsis {
      font-size: 16.5px;
    }
    .hero-actions a,
    .hero-actions button {
      min-height: 54px;
      padding: 14px 32px;
      font-size: 16px;
    }
  }

  @media (min-width: 641px) and (max-width: 1024px) {
    .hero-content {
      max-width: 68%;
    }
    .has-image::after {
      background:
        linear-gradient(90deg, var(--color-canvas), color-mix(in srgb, var(--color-canvas) 90%, transparent) 42%, transparent),
        linear-gradient(0deg, var(--color-canvas), transparent);
    }
  }

  @media (max-width: 640px) {
    .hero-banner {
      margin-top: 14px;
      padding: 24px 16px;
      border-radius: var(--radius-lg);
      min-height: 300px;
    }
    .hero-banner.has-image {
      padding-top: 170px;
      min-height: 450px;
    }
    .backdrop-img {
      height: 300px;
      object-position: 65% 25%;
      mask-image: linear-gradient(#000 45%, transparent);
    }
    .has-image::after {
      background: linear-gradient(0deg, var(--color-canvas) 35%, color-mix(in srgb, var(--color-canvas) 95%, transparent) 48%, transparent 95%);
    }
    .hero-content {
      max-width: 100%;
      gap: 12px;
    }
    h1 {
      font-size: clamp(28px, 8vw, 38px);
    }
    .hero-title-logo {
      max-height: 90px;
      max-width: 100%;
    }
    .hero-synopsis {
      font-size: 14px;
      -webkit-line-clamp: 2;
      line-clamp: 2;
    }
    .hero-actions {
      width: 100%;
      gap: 8px;
    }
    .hero-actions a,
    .hero-actions button {
      flex: 1;
      padding: 12px;
      white-space: nowrap;
    }
  }
</style>
