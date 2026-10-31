<script lang="ts">
  import type { Snippet } from 'svelte';
  import { type Work, localizedTitle, localizedPlot, localizedTagline } from './api';
  import { locale } from '../i18n';
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import { imagePreviewUrl } from './image-url';
  import Poster from './Poster.svelte';
  import Icon from '../ui/Icon.svelte';

  let { work, duration, children }: { work: Work; duration?: number; children: Snippet } = $props();
  const artwork = $derived(selectArtwork(work, $session));
  const backdrop = $derived(imagePreviewUrl(artwork.backdrop, 1280));
  const logo = $derived(imagePreviewUrl(artwork.logo, 640));
  let failedBackdrop = $state('');
  let failedLogo = $state('');
  const hasBackdrop = $derived(Boolean(backdrop && backdrop !== failedBackdrop));
  const meaningful = (value?: string | null) => Boolean(value?.trim() && !['Média local indexé', 'Média local'].includes(value.trim()));
  const displayTitle = $derived(localizedTitle(work, $locale));
  const displayPlot = $derived(localizedPlot(work, $locale));
  const displayTagline = $derived(localizedTagline(work, $locale));
  const synopsis = $derived(meaningful(displayPlot) ? displayPlot : 'Synopsis non renseigné.');
</script>

<section class="detail-hero" class:has-backdrop={hasBackdrop} aria-label={`Présentation de ${displayTitle}`}>
  {#if hasBackdrop}
    <img class="detail-backdrop" src={backdrop} alt="" width="1280" height="720" fetchpriority="high" decoding="async" onerror={() => failedBackdrop = backdrop} />
  {/if}
  <div class="hero-layout">
    <div class="detail-poster-column"><Poster {work} showMetadata={false} /></div>
    <div class="detail-info-column">
      <p class="eyebrow">{work.kind === 'series' ? 'Série' : 'Film'}{work.synthetic ? ' · Démonstration' : ''}</p>
      <h1>
        {#if logo && logo !== failedLogo}
          <img class="title-logo" src={logo} alt={displayTitle} width="640" height="200" decoding="async" onerror={() => failedLogo = logo} />
        {:else}
          {displayTitle}
        {/if}
      </h1>
      {#if meaningful(displayTagline)}
        <p class="tagline">« {displayTagline} »</p>
      {/if}
      <div class="essential-meta">
        {#if work.year}<span class="meta-pill">{work.year}</span>{/if}
        {#if duration && Number.isFinite(duration) && duration > 0}<span class="meta-pill">{Math.ceil(duration / 60)} min</span>{/if}
        {#if work.mpaa}<span class="meta-badge mpaa">{work.mpaa}</span>{/if}
        {#if work.kind === 'series' && work.seasonCount}<span class="meta-pill">{work.seasonCount} {work.seasonCount > 1 ? 'saisons' : 'saison'}</span>{/if}
        {#if work.rating}<span class="meta-badge rating"><Icon name="star" size={12} /> Note {work.rating.toLocaleString('fr-FR')} / 10</span>{/if}
        {#if work.directors?.length}<span class="meta-pill director-meta">De <strong>{work.directors.join(', ')}</strong></span>{/if}
      </div>
      <p class="synopsis">{synopsis}</p>
      {#if work.genres?.length}
        <div class="genres-row">
          {#each work.genres as genre (genre)}
            <span class="genre-badge">{genre}</span>
          {/each}
        </div>
      {/if}
      {#if work.available === false}
        <p class="availability" role="status">Ce contenu est temporairement indisponible. Le serveur qui l’héberge est hors ligne.</p>
      {/if}
      {@render children()}
      {#if (work.originalTitle && work.originalTitle !== work.title) || (meaningful(work.outline) && work.outline !== synopsis)}
        <details class="editorial-details">
          <summary>À propos de l’œuvre</summary>
          {#if work.originalTitle && work.originalTitle !== work.title}<p>Titre original : <em>{work.originalTitle}</em></p>{/if}
          {#if meaningful(work.outline) && work.outline !== synopsis}<p>{work.outline}</p>{/if}
        </details>
      {/if}
    </div>
  </div>
</section>

<style>
  .detail-hero {
    position: relative;
    isolation: isolate;
    margin-top: 20px;
    padding: clamp(28px, 4vw, 52px);
    background: radial-gradient(circle at 80% 15%, rgba(249, 115, 22, 0.14), transparent 55%), var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-2xl);
    box-shadow: var(--shadow-lg);
    overflow: hidden;
  }

  .detail-backdrop {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: 75% 25%;
    z-index: -2;
  }

  .has-backdrop::after {
    content: '';
    position: absolute;
    inset: 0;
    z-index: -1;
    background:
      linear-gradient(90deg, var(--color-canvas) 0%, color-mix(in srgb, var(--color-canvas) 93%, transparent) 44%, color-mix(in srgb, var(--color-canvas) 45%, transparent) 100%),
      linear-gradient(0deg, var(--color-canvas) 5%, transparent 85%);
  }

  .hero-layout {
    display: grid;
    grid-template-columns: clamp(160px, 18vw, 240px) minmax(0, 1fr);
    gap: clamp(24px, 4vw, 48px);
    align-items: center;
  }

  .detail-info-column {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 15px;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .eyebrow {
    margin: 0;
    color: var(--color-accent);
    font-size: 11px;
    font-weight: 750;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: clamp(30px, 4vw, 54px);
    line-height: 1.08;
    letter-spacing: -0.035em;
    font-weight: 800;
    color: var(--color-text-bright);
    max-width: 22ch;
  }

  .title-logo {
    display: block;
    width: auto;
    height: auto;
    max-width: min(100%, 380px);
    max-height: 125px;
    object-fit: contain;
    object-position: left;
    filter: drop-shadow(0 6px 20px rgba(0, 0, 0, 0.6));
  }

  .tagline {
    margin: -4px 0 0;
    font-size: 14px;
    font-style: italic;
    color: var(--color-muted);
  }

  .essential-meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    color: var(--color-muted);
    font-size: 13.5px;
    font-weight: 500;
  }

  .essential-meta:empty {
    display: none;
  }

  .meta-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 9px;
    border-radius: var(--radius-xs);
    font-size: 12px;
    font-weight: 700;
  }

  .meta-badge.mpaa {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid var(--color-line-strong);
    color: var(--color-text-bright);
  }

  .meta-badge.rating {
    background: rgba(245, 158, 11, 0.15);
    border: 1px solid rgba(245, 158, 11, 0.3);
    color: #FBBF24;
  }

  .director-meta strong {
    color: var(--color-text-bright);
    font-weight: 650;
  }

  .synopsis {
    max-width: 105ch;
    margin: 0;
    font-size: 15.5px;
    line-height: 1.72;
    color: var(--color-text);
  }

  .genres-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .genre-badge {
    padding: 4px 11px;
    border-radius: var(--radius-pill);
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid var(--color-line);
    color: var(--color-muted);
    font-size: 12px;
    font-weight: 500;
  }

  .editorial-details {
    max-width: 65ch;
    font-size: 13px;
    color: var(--color-muted);
    line-height: 1.6;
  }

  .editorial-details summary {
    min-height: 44px;
    display: list-item;
    padding-block: 10px;
    cursor: pointer;
  }

  .availability {
    margin: 0;
    max-width: 65ch;
    color: var(--color-danger);
    font-size: 14px;
    line-height: 1.6;
  }

  @media (min-width: 1600px) {
    .detail-info-column {
      max-width: 1200px;
    }
  }

  @media (max-width: 820px) {
    .hero-layout {
      grid-template-columns: minmax(0, 1fr);
    }
    .detail-poster-column {
      max-width: 110px;
    }
    .has-backdrop .detail-poster-column {
      display: none;
    }
    .has-backdrop {
      padding-top: 180px;
    }
    .detail-backdrop {
      height: 300px;
      mask-image: linear-gradient(#000 30%, transparent);
    }
    .has-backdrop::after {
      background: linear-gradient(0deg, var(--color-canvas) 30%, color-mix(in srgb, var(--color-canvas) 85%, transparent) 65%, transparent);
    }
    .title-logo {
      max-height: 100px;
    }
  }

  @media (max-width: 480px) {
    .detail-hero {
      margin-top: 16px;
      padding: 24px 16px;
      border-radius: var(--radius-lg);
    }
    .has-backdrop {
      padding-top: 160px;
    }
    .detail-info-column {
      gap: 12px;
    }
    h1 {
      font-size: 30px;
    }
  }
</style>
