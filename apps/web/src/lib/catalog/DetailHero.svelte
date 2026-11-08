<script lang="ts">
  import type { Snippet } from 'svelte';
  import { type Work, localizedTitle, localizedPlot, localizedTagline } from './api';
  import { locale, t, fmt } from '../i18n';
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import { imagePreviewUrl } from './image-url';
  import { meaningfulText } from './detail-facts';
  import Poster from './Poster.svelte';
  import Icon from '../ui/Icon.svelte';

  // En-tête « Affiche » : affiche nette + identité + actions + fiche rapide, sur le
  // fond de l'œuvre. Tout ce qui sert à agir tient dans le premier écran ; le reste
  // de la fiche vit dans les onglets placés juste dessous.
  let { work, duration, children, facts }: { work: Work; duration?: number; children: Snippet; facts?: Snippet } = $props();
  const artwork = $derived(selectArtwork(work, $session));
  // backdrop.jpg est l'image de fond ; landscape.jpg un repli large.
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
  let expanded = $state(false);
  $effect(() => { void work.id; expanded = false; });
  const hasBackdrop = $derived(Boolean(backdrop));
  const hasClearart = $derived((!hasBackdrop || ambient) && Boolean(clearart) && failedClearart !== clearart);
  const placeholders = $derived([$t('indexed_local_media'), $t('local_media')]);
  const displayTitle = $derived(localizedTitle(work, $locale));
  const displayPlot = $derived(localizedPlot(work, $locale));
  const displayTagline = $derived(localizedTagline(work, $locale));
  const synopsis = $derived(meaningfulText(displayPlot, placeholders) ? displayPlot : $t('detail_no_synopsis'));
  const longSynopsis = $derived(synopsis.length > 240);
  const formattedRating = $derived(
    work.rating !== null && work.rating !== undefined && Number.isFinite(Number(work.rating))
      ? $fmt.decimal(Number(work.rating), 1)
      : null
  );
  const ratingsSummary = $derived(
    (work.ratings ?? []).slice(0, 2).map(r => `${r.provider.toUpperCase()} ${$fmt.decimal(Number(r.value), 1)}`).join(' · ')
  );
  const hasFacts = $derived(Boolean(facts || ratingsSummary || work.directors?.length || work.writers?.length || work.genres?.length));
  const hasAside = $derived(hasFacts || hasClearart);
</script>

<section class="detail" aria-label={$t('about', { displayTitle })}>
  <!-- Sur une image, l'en-tête garde les jetons sombres (`.halo.dark`) quel que soit le thème :
       boutons et fiche rapide restent lisibles sur le voile. -->
  <div class="masthead" class:has-backdrop={hasBackdrop} class:halo={hasBackdrop} class:dark={hasBackdrop}>
    {#if hasBackdrop}
      <!-- Calque image à part : lui seul est rogné aux coins arrondis, pour que les menus et
           fenêtres des actions (⋯, listes) passent au-dessus du reste de la page. -->
      <div class="media" class:ambient aria-hidden="true">
        {#if ambient}
          <!-- Affiche seule : floutée en fond d'ambiance, jamais recadrée en net. -->
          <img class="fill" src={backdrop} alt="" width="1280" height="720" decoding="async" onerror={() => failedBackdrops = [...failedBackdrops, backdrop]} />
        {:else}
          <!-- Image large en fond de tout le bandeau (recadrée comme au cinéma, cadrée vers le haut
               où se trouvent en général les visages). -->
          <img class="backdrop" src={backdrop} alt="" width="1280" height="720" fetchpriority="high" decoding="async" onerror={() => failedBackdrops = [...failedBackdrops, backdrop]} />
        {/if}
        <div class="shade"></div>
      </div>
    {/if}

    <div class="layout" class:has-aside={hasAside}>
      <!-- folder.jpg : l'affiche reste nette ; son cadre suit le ratio réel de l'image -->
      <div class="poster-col"><Poster {work} showMetadata={false} /></div>

      <div class="identity">
        <div class="essential-meta">
          <span class="badge">{work.kind === 'series' ? $t('kind_series') : $t('kind_movie')}{work.synthetic ? ` · ${$t('demonstration')}` : ''}</span>
          {#if work.year}<span>{work.year}</span>{/if}
          {#if duration && Number.isFinite(duration) && duration > 0}<span>{$t('minutes_short', { count: Math.ceil(duration / 60) })}</span>{/if}
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
        {#if meaningfulText(displayTagline, placeholders)}
          <p class="tagline">« {displayTagline} »</p>
        {/if}
      </div>

      <div class="main">
        {#if work.available === false}
          <p class="availability" role="status">{$t('this_content_is_temporarily_unavailable_the')}</p>
        {/if}
        <div class="synopsis-block">
          <p class="synopsis" class:clamped={longSynopsis && !expanded} id="detail-synopsis">{synopsis}</p>
          {#if longSynopsis}
            <button type="button" class="read-more" aria-expanded={expanded} aria-controls="detail-synopsis" onclick={() => (expanded = !expanded)}>
              {expanded ? $t('detail_read_less') : $t('detail_read_more')}
            </button>
          {/if}
        </div>
        {@render children()}
      </div>

      {#if hasAside}
        <aside class="aside" aria-label={$t('detail_quick_facts')}>
          {#if hasClearart}
            <img class="clearart" src={clearart} alt="" width="640" height="360" decoding="async" loading="lazy" onerror={() => failedClearart = clearart} />
          {/if}
          {#if hasFacts}
            <dl class="facts">
              {#if ratingsSummary}
                <div class="fact"><dt>{$t('detail_ratings')}</dt><dd>{ratingsSummary}</dd></div>
              {/if}
              {#if work.directors?.length}
                <div class="fact"><dt>{work.directors.length > 1 ? $t('detail_directors') : $t('director')}</dt><dd>{work.directors.slice(0, 2).join(', ')}</dd></div>
              {:else if work.writers?.length}
                <div class="fact"><dt>{work.writers.length > 1 ? $t('detail_writers') : $t('screenplay')}</dt><dd>{work.writers.slice(0, 2).join(', ')}</dd></div>
              {/if}
              {#if facts}
                {@render facts()}
              {:else if work.genres?.length}
                <div class="fact"><dt>{$t('stats_genres_title')}</dt><dd>{work.genres.join(', ')}</dd></div>
              {/if}
            </dl>
          {/if}
        </aside>
      {/if}
    </div>
  </div>
</section>

<style>
  .detail {
    position: relative;
    margin-top: 12px;
  }

  /* Ni overflow ni contexte d'empilement ici : les menus déroulants et fenêtres des
     actions doivent pouvoir sortir de l'en-tête et passer au-dessus de la page. */
  .masthead {
    position: relative;
    border-radius: var(--r-xl);
    background: var(--bg-2);
    color: var(--tx);
  }

  /* Avec une image large : elle couvre tout le bandeau, qui occupe le premier écran.
     Le contenu se cale en bas : identité, synopsis et actions à gauche, fiche rapide
     (notes, qualité, langues) à droite. Si le contenu est plus haut que l'écran
     (téléphone, synopsis déplié), le bandeau grandit avec lui. */
  .masthead.has-backdrop {
    display: grid;
    min-height: clamp(460px, calc(100dvh - 116px), 1080px);
    background: #06080e;
    color: var(--on-media);
  }

  .media {
    position: absolute;
    inset: 0;
    overflow: hidden;
    border-radius: inherit;
    pointer-events: none;
  }

  .backdrop {
    position: absolute;
    inset: 0;
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center 22%;
  }

  .fill {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    filter: blur(36px) saturate(1.2) brightness(0.6);
    transform: scale(1.2);
  }

  /* Voile de lisibilité : seul dégradé autorisé sur une image. En bas, il rejoint
     exactement le fond du bandeau pour qu'aucune limite d'image ne se voie. */
  .shade {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(0deg, rgb(6 8 14 / 0.92) 0%, rgb(6 8 14 / 0.55) 26%, rgb(6 8 14 / 0.08) 58%, rgb(6 8 14 / 0.3) 100%),
      linear-gradient(90deg, rgb(6 8 14 / 0.8) 0%, rgb(6 8 14 / 0.45) 38%, rgb(6 8 14 / 0) 62%),
      linear-gradient(270deg, rgb(6 8 14 / 0.55) 0%, rgb(6 8 14 / 0) 28%);
  }

  .masthead.has-backdrop .layout {
    grid-area: 1 / 1;
    align-self: end;
  }

  @media (min-width: 1101px) {
    .masthead.has-backdrop .layout.has-aside {
      grid-template-columns: clamp(140px, 14vw, 230px) minmax(0, 1fr) minmax(260px, 340px);
      column-gap: clamp(20px, 3vw, 48px);
    }
  }

  .layout {
    position: relative;
    display: grid;
    grid-template-columns: clamp(140px, 14vw, 230px) minmax(0, 1fr);
    grid-template-rows: 1fr auto;
    grid-template-areas:
      'poster identity'
      'poster main';
    column-gap: clamp(20px, 3vw, 40px);
    row-gap: 16px;
    padding: clamp(28px, 6vw, 88px) clamp(20px, 3vw, 40px) clamp(20px, 3vw, 36px);
  }

  .layout.has-aside {
    grid-template-columns: clamp(140px, 14vw, 230px) minmax(0, 1fr) minmax(240px, 320px);
    grid-template-areas:
      'poster identity .'
      'poster main aside';
  }

  .poster-col {
    grid-area: poster;
    align-self: end;
    min-width: 0;
    filter: drop-shadow(0 18px 40px rgb(0 0 0 / 0.4));
  }

  .identity {
    grid-area: identity;
    align-self: end;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 12px;
    min-width: 0;
  }

  .main {
    grid-area: main;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 14px;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .aside {
    grid-area: aside;
    align-self: end;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 14px;
    min-width: 0;
  }

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

  .masthead:not(.has-backdrop) .badge { background: var(--pn-2); }

  .rating { display: inline-flex; align-items: center; gap: 4px; }

  h1 {
    margin: 0;
    font-size: clamp(32px, 4vw, 58px);
    line-height: 0.98;
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
    width: min(100%, 480px);
    max-height: 130px;
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

  .synopsis-block {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    max-width: 72ch;
  }

  .synopsis {
    margin: 0;
    font-size: 15px;
    line-height: 1.6;
    opacity: 0.92;
  }

  .synopsis.clamped {
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .read-more {
    min-height: 32px;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    font-size: 13px;
    font-weight: 800;
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
  }

  .read-more:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; border-radius: var(--r-xs); }

  /* clearart.png : illustration détourée, au-dessus de la fiche rapide */
  .clearart {
    display: block;
    width: 100%;
    height: auto;
    max-height: 160px;
    object-fit: contain;
    object-position: bottom center;
    filter: drop-shadow(0 18px 40px rgb(0 0 0 / 0.45));
  }

  .facts {
    display: grid;
    gap: 10px;
    margin: 0;
    padding: 16px 18px;
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    background: var(--glass);
    color: var(--tx);
    font-size: 13px;
  }

  .facts :global(.fact) {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    min-width: 0;
  }

  .facts :global(dt) {
    flex: 0 0 auto;
    color: var(--mu);
    font-weight: 600;
  }

  .facts :global(dd) {
    margin: 0;
    min-width: 0;
    font-weight: 700;
    text-align: right;
    overflow-wrap: anywhere;
  }

  .availability {
    margin: 0;
    max-width: 65ch;
    color: var(--err);
    font-size: 14px;
    font-weight: 700;
    line-height: 1.6;
  }

  .masthead.has-backdrop .availability { color: #ff9b9b; }

  /* Tablette et petit portable : la fiche rapide passe sous les actions, en colonnes. */
  @media (max-width: 1100px) {
    .layout.has-aside {
      grid-template-columns: clamp(140px, 18vw, 210px) minmax(0, 1fr);
      grid-template-rows: 1fr auto auto;
      grid-template-areas:
        'poster identity'
        'poster main'
        'aside aside';
    }

    .aside { align-self: stretch; }
    .facts { grid-template-columns: repeat(auto-fit, minmax(min(100%, 220px), 1fr)); column-gap: 28px; }
    .facts :global(.fact) { flex-direction: column; justify-content: flex-start; gap: 2px; }
    .facts :global(dd) { text-align: left; }
    .clearart { display: none; }
  }

  /* Téléphone : affiche réduite à côté du titre, puis actions et fiche en pleine largeur. */
  @media (max-width: 760px) {
    .masthead { border-radius: var(--r-lg); }
    .masthead.has-backdrop { min-height: min(78dvh, 640px); }
    .layout,
    .layout.has-aside {
      grid-template-columns: 104px minmax(0, 1fr);
      grid-template-rows: auto auto auto;
      grid-template-areas:
        'poster identity'
        'main main'
        'aside aside';
      column-gap: 16px;
      padding: 20px 16px 18px;
    }

    .identity { gap: 8px; }
    .essential-meta { gap: 4px 10px; font-size: 12px; }
    .badge { height: 22px; padding: 0 7px; font-size: 11px; }
    h1 { font-size: clamp(24px, 7vw, 32px); }
    .title-logo { max-height: 72px; }
    .tagline { display: none; }
    .synopsis { font-size: 14px; }
    .facts { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px 16px; padding: 14px; }
  }
</style>
