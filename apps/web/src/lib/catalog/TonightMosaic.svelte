<script lang="ts">
  import { session } from '../auth/session';
  import { locale } from '../i18n';
  import { type Work, localizedTitle } from './api';
  import { selectArtwork } from './artwork-selection';
  import { imagePreviewUrl } from './image-url';
  import { arrangeTonight, mosaicLayout } from './tonight';
  import { fitArtwork } from '../ui/fit-artwork';

  let { works = [], detailUrl }: { works?: Work[]; detailUrl: (work: Work) => string } = $props();

  const hasWideArt = (work: Work) => { const artwork = selectArtwork(work, $session); return Boolean(artwork.landscape || artwork.backdrop); };
  const ordered = $derived(arrangeTonight(works, hasWideArt));
  const layout = $derived(mosaicLayout(ordered.length));
  // Grille portée par des classes (les attributs style en ligne sont bloqués par la CSP).
  const gridClass = $derived(layout ? `rows-${layout.grids.desktop.featureRows} cols-${layout.grids.desktop.columns} tablet-cols-${layout.grids.tablet.columns} phone-cols-${layout.grids.phone.columns}` : '');
  let failedImages = $state<string[]>([]);

  // Vedette 16:9 : landscape.jpg puis backdrop ; affiches 2:3 : l'affiche. Un repli d'un autre
  // format est affiché sans recadrage abusif par fitArtwork (image entière sur fond flouté).
  function tileImage(work: Work, feature: boolean): string {
    const artwork = selectArtwork(work, $session);
    return feature
      ? imagePreviewUrl(artwork.landscape || artwork.backdrop || artwork.poster, 1280)
      : imagePreviewUrl(artwork.poster || artwork.landscape || artwork.backdrop, 640);
  }
</script>

{#if layout}
  <section class="tonight" aria-labelledby="tonight-title">
    <div class="head">
      <h2 id="tonight-title">Pour ce soir</h2>
      <p>Ajouts récents de vos bibliothèques</p>
    </div>
    <ul class="mosaic {gridClass}">
      {#each layout.tiles as tile, index (ordered[index].id)}
        {@const work = ordered[index]}
        {@const feature = tile.shape === 'feature'}
        {@const image = tileImage(work, feature)}
        {@const hasImage = Boolean(image) && !failedImages.includes(image)}
        <li
          class="cell {tile.shape}"
          class:hide-desktop={tile.hidden.desktop}
          class:hide-tablet={tile.hidden.tablet}
          class:hide-phone={tile.hidden.phone}
        >
          <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- detailUrl() returns a resolved URL. -->
          <a class="tile" class:has-image={hasImage} href={detailUrl(work)}>
            {#if hasImage}
              <img src={image} alt="" width={feature ? 1280 : 640} height={feature ? 720 : 960} loading="lazy" decoding="async" use:fitArtwork={{ adapt: false }} onerror={() => (failedImages = [...failedImages, image])} />
              <span class="shade" aria-hidden="true"></span>
            {/if}
            <span class="kind">{work.kind === 'series' ? 'Série' : 'Film'}</span>
            <span class="title">{localizedTitle(work, $locale)}</span>
          </a>
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  /* Conteneur de requête : la largeur des affiches se calcule en cqw. */
  .tonight { container-type: inline-size; display: flex; flex-direction: column; gap: 14px; min-width: 0; }

  .head { display: flex; flex-direction: column; gap: 4px; }

  h2 {
    margin: 0;
    font-size: calc(24px * var(--ui-scale, 1));
    font-weight: 800;
    letter-spacing: -0.02em;
    line-height: 1.15;
    color: var(--tx);
  }

  .head p { margin: 0; font-size: 14px; color: var(--mu); }

  /* Deux formats seulement : vedette 16:9 et affiches 2:3, fixés par aspect-ratio. */
  .rows-1 { --rows: 1; }
  .rows-2 { --rows: 2; }
  .cols-2 { --cols: 2; }
  .cols-3 { --cols: 3; }
  .cols-5 { --cols: 5; }
  .tablet-cols-3 { --tablet-cols: 3; }
  .tablet-cols-4 { --tablet-cols: 4; }
  .tablet-cols-5 { --tablet-cols: 5; }
  .tablet-cols-6 { --tablet-cols: 6; }
  .phone-cols-2 { --phone-cols: 2; }
  .phone-cols-3 { --phone-cols: 3; }

  /*
   * Ordinateur et TV : vedette à gauche sur --rows rangées d'affiches 2:3 (largeur --poster).
   * Hauteur vedette = rows × 1,5 × poster + (rows − 1) × gap ; largeur = 16/9 de cette hauteur ;
   * largeur totale = vedette + cols × (poster + gap) = 100cqw, d'où --poster.
   */
  .mosaic {
    --gap: calc(16px * var(--ui-scale, 1));
    --poster: calc((100cqw - (var(--rows) - 1) * var(--gap) * 16 / 9 - var(--cols) * var(--gap)) / (var(--rows) * 8 / 3 + var(--cols)));
    display: grid;
    grid-template-columns: minmax(0, 1fr) repeat(var(--cols), var(--poster));
    gap: var(--gap);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .cell { min-width: 0; }
  .cell.feature { grid-column: 1; grid-row: 1 / span var(--rows); }
  .cell.poster { aspect-ratio: 2 / 3; }

  .hide-desktop { display: none; }

  .tile {
    position: relative;
    isolation: isolate;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 4px;
    width: 100%;
    height: 100%;
    padding: 18px;
    overflow: hidden;
    border-radius: var(--r-lg);
    background: var(--bg-2);
    color: var(--tx);
    text-decoration: none;
    transition: transform var(--t-slow) var(--ease);
  }

  .poster .tile { padding: 12px; gap: 2px; border-radius: var(--r-md); }

  .tile.has-image { color: var(--on-media); }

  @media (hover: hover) and (pointer: fine) {
    .tile:hover { transform: scale(1.015); }
  }

  .tile:active { transform: scale(0.98); }

  .tile:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }

  img {
    position: absolute;
    inset: 0;
    z-index: -2;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* Voile de lisibilité : seul dégradé autorisé sur une image */
  .shade {
    position: absolute;
    inset: 0;
    z-index: -1;
    background: linear-gradient(0deg, var(--media-shade) 0%, rgb(6 8 14 / 0) 58%);
  }

  .kind {
    font-size: calc(12px * var(--ui-scale, 1));
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    opacity: 0.85;
  }

  .poster .kind { font-size: calc(10px * var(--ui-scale, 1)); }

  .title {
    font-size: calc(22px * var(--ui-scale, 1));
    font-weight: 800;
    letter-spacing: -0.01em;
    line-height: 1.1;
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .poster .title { font-size: calc(15px * var(--ui-scale, 1)); -webkit-line-clamp: 2; line-clamp: 2; }

  /* Tablette : vedette 16:9 pleine largeur, puis une rangée d'affiches. */
  @media (max-width: 1023px) {
    .mosaic { grid-template-columns: repeat(var(--tablet-cols), minmax(0, 1fr)); }
    .cell.feature { grid-column: 1 / -1; grid-row: auto; aspect-ratio: 16 / 9; }
    .hide-desktop { display: block; }
    .hide-tablet { display: none; }
  }

  /* Téléphone : vedette 16:9 pleine largeur, puis affiches sur deux ou trois colonnes. */
  @media (max-width: 639px) {
    .mosaic { --gap: 12px; grid-template-columns: repeat(var(--phone-cols), minmax(0, 1fr)); }
    .hide-tablet { display: block; }
    .hide-phone { display: none; }
    .tile { padding: 14px; }
    .poster .tile { padding: 10px; }
    .poster .title { font-size: 13px; }
    .kind, .poster .kind { font-size: 10px; }
    .title { font-size: 19px; }
    h2 { font-size: 21px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .tile, .tile:hover, .tile:active { transition: none; transform: none; }
  }
</style>
