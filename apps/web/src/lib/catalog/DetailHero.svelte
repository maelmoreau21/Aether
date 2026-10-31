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
  const formattedRating = $derived(
    work.rating !== null && work.rating !== undefined && Number.isFinite(Number(work.rating))
      ? Number(work.rating).toLocaleString('fr-FR', { minimumFractionDigits: 1, maximumFractionDigits: 1 })
      : null
  );
  const hasImdb = $derived(
    Boolean(work.externalIds?.imdb || work.ratings?.some(r => r.provider.toLowerCase() === 'imdb'))
  );
  const hasTmdb = $derived(
    Boolean(work.externalIds?.tmdb || work.ratings?.some(r => ['tmdb', 'themoviedb'].includes(r.provider.toLowerCase())))
  );
  const hasTvdb = $derived(
    Boolean(work.externalIds?.tvdb || work.ratings?.some(r => r.provider.toLowerCase() === 'tvdb'))
  );
</script>

<section class="detail-hero" class:has-backdrop={hasBackdrop} aria-label={`Présentation de ${displayTitle}`}>
  <div class="detail-backdrop-clip" class:has-backdrop={hasBackdrop}>
    {#if hasBackdrop}
      <img class="detail-backdrop" src={backdrop} alt="" width="1280" height="720" fetchpriority="high" decoding="async" onerror={() => failedBackdrop = backdrop} />
    {/if}
  </div>

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
        {#if formattedRating}<span class="meta-badge rating"><Icon name="star" size={13} /> {formattedRating} / 10</span>{/if}
      </div>

      {#if work.available === false}
        <p class="availability" role="status">Ce contenu est temporairement indisponible. Le serveur qui l’héberge est hors ligne.</p>
      {/if}

      {@render children()}

      <p class="synopsis">{synopsis}</p>

      {#if work.tags?.length}
        <p class="tags-line">
          <span class="tags-label">Étiquettes :</span>
          <strong>{work.tags.join(', ')}</strong>
        </p>
      {/if}

      {#if hasImdb || hasTmdb || hasTvdb || work.ratings?.length}
        <div class="provider-badges-row" aria-label="Sources et notes externes">
          {#if hasImdb}
            <span class="provider-logo imdb">IMDb</span>
          {/if}
          {#if hasTmdb}
            <span class="provider-logo tmdb">TMDB</span>
          {/if}
          {#if hasTvdb}
            <span class="provider-logo tvdb">TVDB</span>
          {/if}
          {#each work.ratings ?? [] as r, idx (idx)}
            <span class="provider-score-pill">
              <strong>{r.provider.toUpperCase()}</strong>
              {Number(r.value).toLocaleString('fr-FR', { minimumFractionDigits: 1, maximumFractionDigits: 1 })} / {r.max}
            </span>
          {/each}
        </div>
      {/if}

      {#if work.directors?.length || work.writers?.length || work.genres?.length || (work.originalTitle && work.originalTitle !== work.title)}
        <dl class="jellyfin-meta-table">
          {#if work.directors?.length}
            <div class="meta-row">
              <dt>{work.directors.length > 1 ? 'Réalisateurs' : 'Réalisateur'}</dt>
              <dd>{work.directors.join(', ')}</dd>
            </div>
          {/if}
          {#if work.writers?.length}
            <div class="meta-row">
              <dt>{work.writers.length > 1 ? 'Scénaristes' : 'Scénario'}</dt>
              <dd>{work.writers.join(', ')}</dd>
            </div>
          {/if}
          {#if work.genres?.length}
            <div class="meta-row">
              <dt>Genres</dt>
              <dd>{work.genres.join(', ')}</dd>
            </div>
          {/if}
          {#if work.originalTitle && work.originalTitle !== work.title}
            <div class="meta-row">
              <dt>Titre original</dt>
              <dd>{work.originalTitle}</dd>
            </div>
          {/if}
        </dl>
      {/if}
    </div>
  </div>
</section>

<style>
  .detail-hero {
    position: relative;
    z-index: 10;
    margin-top: 20px;
    padding: clamp(28px, 4vw, 52px);
    background: radial-gradient(circle at 80% 15%, rgba(249, 115, 22, 0.14), transparent 55%), var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-2xl);
    box-shadow: var(--shadow-lg);
    overflow: visible;
  }

  .detail-backdrop-clip {
    position: absolute;
    inset: 0;
    overflow: hidden;
    border-radius: inherit;
    z-index: 0;
    pointer-events: none;
  }

  .detail-backdrop {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: 75% 25%;
  }

  .detail-backdrop-clip.has-backdrop::after {
    content: '';
    position: absolute;
    inset: 0;
    background:
      linear-gradient(90deg, var(--color-canvas) 0%, color-mix(in srgb, var(--color-canvas) 92%, transparent) 45%, color-mix(in srgb, var(--color-canvas) 45%, transparent) 100%),
      linear-gradient(0deg, var(--color-canvas) 5%, transparent 85%);
  }

  .hero-layout {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: clamp(170px, 18vw, 250px) minmax(0, 1fr);
    gap: clamp(24px, 4vw, 48px);
    align-items: start;
  }

  .detail-info-column {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 14px;
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
    font-size: clamp(30px, 4vw, 52px);
    line-height: 1.08;
    letter-spacing: -0.035em;
    font-weight: 800;
    color: var(--color-text-bright);
    max-width: 24ch;
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
    gap: 8px 14px;
    color: var(--color-text);
    font-size: 14px;
    font-weight: 600;
  }

  .essential-meta:empty {
    display: none;
  }

  .meta-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 10px;
    border-radius: var(--radius-xs);
    font-size: 12.5px;
    font-weight: 750;
  }

  .meta-badge.mpaa {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid var(--color-line-strong);
    color: var(--color-text-bright);
  }

  .meta-badge.rating {
    background: rgba(245, 158, 11, 0.16);
    border: 1px solid rgba(245, 158, 11, 0.38);
    color: #FBBF24;
  }

  .synopsis {
    max-width: 110ch;
    margin: 4px 0 0;
    font-size: 15.5px;
    line-height: 1.72;
    color: var(--color-text-bright);
  }

  .tags-line {
    margin: 0;
    max-width: 105ch;
    font-size: 13.5px;
    line-height: 1.6;
    color: var(--color-muted);
  }

  .tags-label {
    margin-right: 6px;
  }

  .tags-line strong {
    color: var(--color-text-bright);
    font-weight: 650;
  }

  .provider-badges-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    margin-top: 2px;
  }

  .provider-logo {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 3px 9px;
    border-radius: 5px;
    font-size: 11.5px;
    font-weight: 900;
    letter-spacing: 0.03em;
    line-height: 1.2;
  }

  .provider-logo.imdb {
    background: #F5C518;
    color: #000000;
  }

  .provider-logo.tmdb {
    background: linear-gradient(90deg, #90CEA1 0%, #01B4E4 100%);
    color: #081C24;
  }

  .provider-logo.tvdb {
    background: #35C759;
    color: #071F0E;
  }

  .provider-score-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px;
    border-radius: var(--radius-pill);
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    font-size: 12px;
    color: var(--color-text);
  }

  .provider-score-pill strong {
    color: var(--color-accent);
  }

  .jellyfin-meta-table {
    display: grid;
    gap: 8px;
    margin: 4px 0 0;
    width: 100%;
    max-width: 900px;
    font-size: 14px;
  }

  .meta-row {
    display: grid;
    grid-template-columns: 120px minmax(0, 1fr);
    gap: 16px;
    align-items: baseline;
  }

  .meta-row dt {
    color: var(--color-muted);
    font-weight: 500;
  }

  .meta-row dd {
    margin: 0;
    color: var(--color-text-bright);
    font-weight: 650;
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
      max-width: 1250px;
    }
  }

  @media (max-width: 820px) {
    .hero-layout {
      grid-template-columns: minmax(0, 1fr);
    }
    .detail-poster-column {
      max-width: 120px;
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
    .detail-backdrop-clip.has-backdrop::after {
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
    .meta-row {
      grid-template-columns: 95px minmax(0, 1fr);
      gap: 10px;
    }
    h1 {
      font-size: 30px;
    }
  }
</style>
