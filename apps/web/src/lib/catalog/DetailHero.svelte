<script lang="ts">
  import type { Snippet } from 'svelte';
  import { type Work, localizedTitle, localizedPlot, localizedTagline } from './api';
  import { locale, t } from '../i18n';
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import { imagePreviewUrl } from './image-url';
  import Poster from './Poster.svelte';
  import Icon from '../ui/Icon.svelte';
  import { fitArtwork } from '../ui/fit-artwork';

  let { work, duration, children, below }: { work: Work; duration?: number; children: Snippet; below?: Snippet } = $props();
  const artwork = $derived(selectArtwork(work, $session));
  // backdrop.jpg is the detail image; landscape.jpg is a wide fallback.
  const wideArtwork = $derived(artwork.backdrop || artwork.landscape || artwork.poster);
  // Sans image large, l'affiche floutée sert de fond d'ambiance (jamais rognée en net).
  const ambient = $derived(!artwork.backdrop && !artwork.landscape && Boolean(artwork.poster));
  const previewBackdrop = $derived(imagePreviewUrl(wideArtwork, 1280));
  const backdrop = $derived([previewBackdrop, wideArtwork ?? ''].find(url => url && !failedBackdrops.includes(url)) ?? '');
  const logo = $derived(imagePreviewUrl(artwork.logo, 640));
  const clearart = $derived(imagePreviewUrl(artwork.illustration, 640));
  let failedBackdrops = $state<string[]>([]);
  let failedLogo = $state('');
  let failedClearart = $state('');
  const hasBackdrop = $derived(Boolean(backdrop));
  const hasClearart = $derived((!hasBackdrop || ambient) && Boolean(clearart) && failedClearart !== clearart);
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

<section class="detail" aria-label={`Présentation de ${displayTitle}`}>
  <div class="stage" class:has-clearart={hasClearart}>
    <div class="hero" class:has-backdrop={hasBackdrop}>
      {#if hasBackdrop}
        <img class="backdrop" class:ambient src={backdrop} alt="" width="1280" height="720" fetchpriority="high" decoding="async" use:fitArtwork={{ adapt: false }} onerror={() => failedBackdrops = [...failedBackdrops, backdrop]} />
        <div class="shade" aria-hidden="true"></div>
      {/if}
      <div class="cap">
        <div class="essential-meta">
          <span class="badge">{work.kind === 'series' ? $t('kind_series') : $t('kind_movie')}{work.synthetic ? ` · ${$t('demonstration')}` : ''}</span>
          {#if work.year}<span>{work.year}</span>{/if}
          {#if duration && Number.isFinite(duration) && duration > 0}<span>{Math.ceil(duration / 60)} min</span>{/if}
          {#if work.kind === 'series' && work.seasonCount}<span>{work.seasonCount} {work.seasonCount > 1 ? $t('seasons') : $t('season')}</span>{/if}
          {#if work.genres?.length}<span>{work.genres.slice(0, 3).join(', ')}</span>{/if}
          {#if work.mpaa}<span class="badge">{work.mpaa}</span>{/if}
          {#if formattedRating}<span class="rating"><Icon name="star" size={13} /> {formattedRating} / 10</span>{/if}
        </div>
        <h1>
          {#if logo && logo !== failedLogo}
            <span class="h-sr">{displayTitle}</span>
            <img class="title-logo" src={logo} alt="" width="640" height="200" decoding="async" onerror={() => failedLogo = logo} />
          {:else}
            {displayTitle}
          {/if}
        </h1>
        {#if meaningful(displayTagline)}
          <p class="tagline">« {displayTagline} »</p>
        {/if}
      </div>
    </div>
    {#if hasClearart}
      <img class="clearart" src={clearart} alt="" width="640" height="360" decoding="async" loading="lazy" onerror={() => failedClearart = clearart} />
    {/if}
  </div>

  <div class="body" class:has-backdrop={hasBackdrop && !ambient}>
    <div class="main">
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
          {#if hasImdb}<span class="provider-logo">IMDb</span>{/if}
          {#if hasTmdb}<span class="provider-logo">TMDB</span>{/if}
          {#if hasTvdb}<span class="provider-logo">TVDB</span>{/if}
          {#each work.ratings ?? [] as r, idx (idx)}
            <span class="provider-score-pill">
              <strong>{r.provider.toUpperCase()}</strong>
              {Number(r.value).toLocaleString('fr-FR', { minimumFractionDigits: 1, maximumFractionDigits: 1 })} / {r.max}
            </span>
          {/each}
        </div>
      {/if}

      {#if work.directors?.length || work.writers?.length || work.genres?.length || (work.originalTitle && work.originalTitle !== work.title)}
        <dl class="meta-table">
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
      <!-- Content placed under the information stays beside the poster (no empty gap). -->
      {#if below}<div class="below">{@render below()}</div>{/if}
    </div>
    <!-- folder.jpg : l’affiche reste visible à côté des informations -->
    <aside class="side" aria-label="Affiche"><Poster {work} showMetadata={false} /></aside>
  </div>
</section>

<style>
  .detail {
    position: relative;
    margin-top: 12px;
  }

  .stage { position: relative; }

  .hero {
    position: relative;
    isolation: isolate;
    display: flex;
    align-items: flex-end;
    min-height: 260px;
    overflow: hidden;
    border-radius: var(--r-xl);
    background: var(--bg-2);
    color: var(--tx);
  }

  /* Format de l'image (16:9), plafonné à la hauteur de l'écran : peu ou pas de rognage. */
  .hero.has-backdrop {
    /* Largeur explicite : sinon max-height réduirait aussi la largeur via le ratio. */
    width: 100%;
    aspect-ratio: 16 / 9;
    min-height: 380px;
    max-height: min(72vh, 680px);
    background: #06080e;
    color: var(--on-media);
  }

  .backdrop {
    position: absolute;
    inset: 0;
    z-index: -2;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: 70% 25%;
  }

  .backdrop.ambient { object-position: center; filter: blur(36px) saturate(1.2) brightness(0.6); transform: scale(1.2); }

  /* Voile de lisibilité : seul dégradé autorisé sur une image */
  .shade {
    position: absolute;
    inset: 0;
    z-index: -1;
    background:
      linear-gradient(0deg, var(--media-shade) 0%, rgb(6 8 14 / 0.25) 55%, rgb(6 8 14 / 0) 100%),
      linear-gradient(90deg, rgb(6 8 14 / 0.45) 0%, rgb(6 8 14 / 0) 55%);
  }

  .cap {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 12px;
    width: 100%;
    max-width: 780px;
    padding: clamp(20px, 3vw, 40px);
  }

  .stage.has-clearart .cap { max-width: min(780px, 58%); }

  .essential-meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 14px;
    font-size: 14px;
    font-weight: 600;
    opacity: 0.92;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    height: 24px;
    padding: 0 9px;
    border-radius: var(--r-xs);
    background: rgb(255 255 255 / 0.18);
    font-size: 12px;
    font-weight: 700;
  }

  .hero:not(.has-backdrop) .badge { background: var(--pn-2); }

  .rating { display: inline-flex; align-items: center; gap: 4px; }

  h1 {
    margin: 0;
    font-size: clamp(34px, 5vw, 68px);
    line-height: 0.95;
    letter-spacing: -0.035em;
    font-weight: 800;
    width: 100%;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .title-logo {
    display: block;
    height: auto;
    max-width: 100%;
    width: min(100%, 520px);
    max-height: 150px;
    object-fit: contain;
    object-position: left bottom;
    filter: drop-shadow(0 0 1px rgb(255 255 255 / 0.8)) drop-shadow(0 4px 16px rgb(0 0 0 / 0.8));
  }

  .tagline {
    margin: 0;
    font-size: 15px;
    font-style: italic;
    opacity: 0.85;
  }

  /* clearart.png : dépasse du haut de la grande image */
  .clearart {
    position: absolute;
    right: clamp(24px, 5vw, 72px);
    bottom: 0;
    z-index: 2;
    width: auto;
    height: min(85%, 480px);
    max-width: 40%;
    object-fit: contain;
    object-position: bottom right;
    pointer-events: none;
    filter: drop-shadow(0 18px 40px rgb(0 0 0 / 0.45));
  }

  .body {
    display: grid;
    /* The poster gets a wide column; the information column narrows instead. */
    grid-template-columns: minmax(0, 1fr) clamp(260px, 27vw, 420px);
    gap: clamp(24px, 3vw, 40px);
    align-items: start;
    margin-top: 24px;
  }

  .main {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 16px;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .side {
    position: sticky;
    top: 88px;
  }

  .below { width: 100%; min-width: 0; margin-top: 8px; }

  .synopsis {
    max-width: 72ch;
    margin: 4px 0 0;
    font-size: 17px;
    line-height: 1.65;
    color: var(--tx);
  }

  .tags-line {
    margin: 0;
    max-width: 90ch;
    font-size: 14px;
    line-height: 1.6;
    color: var(--mu);
  }

  .tags-label { margin-right: 6px; }

  .tags-line strong {
    color: var(--tx);
    font-weight: 600;
  }

  .provider-badges-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .provider-logo,
  .provider-score-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border-radius: var(--r-xs);
    background: var(--pn-2);
    color: var(--tx);
    font-size: 12px;
    font-weight: 700;
  }

  .provider-score-pill strong { color: var(--mu); }

  .meta-table {
    display: grid;
    gap: 10px;
    margin: 4px 0 0;
    width: 100%;
    max-width: 760px;
    font-size: 14px;
  }

  .meta-row {
    display: grid;
    grid-template-columns: 130px minmax(0, 1fr);
    gap: 16px;
    align-items: baseline;
  }

  .meta-row dt {
    color: var(--mu);
    font-weight: 600;
  }

  .meta-row dd {
    margin: 0;
    color: var(--tx);
    font-weight: 600;
  }

  .availability {
    margin: 0;
    max-width: 65ch;
    color: var(--err);
    font-size: 14px;
    font-weight: 700;
    line-height: 1.6;
  }

  @media (max-width: 1023px) {
    .body { grid-template-columns: minmax(0, 1fr) clamp(220px, 30vw, 280px); }
  }

  @media (max-width: 760px) {
    .hero { border-radius: var(--r-lg); }
    /* Téléphone : l'image large reste entière (16:9) en haut, le texte passe dessous. */
    .hero.has-backdrop { min-height: 0; padding-top: calc(56.25% - 64px); }
    .hero.has-backdrop .backdrop { inset: 0 0 auto 0; height: auto; aspect-ratio: 16 / 9; }
    .hero.has-backdrop .backdrop.ambient { inset: 0; height: 100%; aspect-ratio: auto; }
    .hero.has-backdrop .shade { inset: 0 0 auto 0; aspect-ratio: 16 / 9; background: linear-gradient(0deg, #06080e 0%, rgb(6 8 14 / 0.35) 40%, rgb(6 8 14 / 0) 100%); }
    .hero.has-backdrop { aspect-ratio: auto; max-height: none; }
    .cap, .stage.has-clearart .cap { max-width: none; }
    .title-logo { max-width: 80%; max-height: 96px; }
    .clearart { display: none; }
    .body { grid-template-columns: minmax(0, 1fr); margin-top: 16px; }
    .side { display: none; }
    /* Sans image de fond, l'affiche reste la seule image : on la garde. */
    .body:not(.has-backdrop) .side { display: block; position: static; order: -1; width: min(200px, 55vw); }
    .synopsis { font-size: 15px; }
    .meta-row { grid-template-columns: 100px minmax(0, 1fr); gap: 10px; }
  }
</style>
