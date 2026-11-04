<script lang="ts">
  import { session } from '../auth/session';
  import { locale } from '../i18n';
  import { type Work, localizedTitle } from './api';
  import { selectArtwork } from './artwork-selection';
  import { imagePreviewUrl } from './image-url';
  import { mosaicLayout } from './tonight';

  let { works = [], detailUrl }: { works?: Work[]; detailUrl: (work: Work) => string } = $props();

  const layout = $derived(mosaicLayout(works.length));
  let failedImages = $state<string[]>([]);

  // Tuiles larges : landscape.jpg puis backdrop ; tuiles hautes et carrées : l'affiche.
  function tileImage(work: Work, wide: boolean): string {
    const artwork = selectArtwork(work, $session);
    return wide
      ? imagePreviewUrl(artwork.landscape || artwork.backdrop || artwork.poster, 1280)
      : imagePreviewUrl(artwork.poster || artwork.landscape || artwork.backdrop, 640);
  }
</script>

{#if layout.length}
  <section class="tonight" aria-labelledby="tonight-title">
    <div class="head">
      <h2 id="tonight-title">Pour ce soir</h2>
      <p>Ajouts récents de vos bibliothèques</p>
    </div>
    <ul class="mosaic">
      {#each layout as tile, index (works[index].id)}
        {@const work = works[index]}
        {@const image = tileImage(work, tile.shape === 'wide')}
        {@const hasImage = Boolean(image) && !failedImages.includes(image)}
        <li
          class="cell {tile.shape}"
          class:hide-desktop={tile.hidden.desktop}
          class:hide-tablet={tile.hidden.tablet}
          class:hide-phone={tile.hidden.phone}
          style:--span-desktop={tile.span.desktop}
          style:--span-tablet={tile.span.tablet}
          style:--span-phone={tile.span.phone}
        >
          <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- detailUrl() returns a resolved URL. -->
          <a class="tile" class:has-image={hasImage} href={detailUrl(work)}>
            {#if hasImage}
              <img src={image} alt="" width={tile.shape === 'wide' ? 1280 : 640} height={tile.shape === 'wide' ? 720 : 960} loading="lazy" decoding="async" onerror={() => (failedImages = [...failedImages, image])} />
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
  .tonight { display: flex; flex-direction: column; gap: 14px; min-width: 0; }

  .head { display: flex; flex-direction: column; gap: 4px; }

  h2 {
    margin: 0;
    font-size: 24px;
    font-weight: 800;
    letter-spacing: -0.02em;
    line-height: 1.15;
    color: var(--tx);
  }

  .head p { margin: 0; font-size: 14px; color: var(--mu); }

  .mosaic {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    grid-auto-rows: 150px;
    gap: 16px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .cell {
    grid-column: span var(--span-desktop);
    grid-row: span 2;
    min-width: 0;
  }

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

  .square img, .tall img { object-position: 50% 20%; }

  /* Voile de lisibilité : seul dégradé autorisé sur une image */
  .shade {
    position: absolute;
    inset: 0;
    z-index: -1;
    background: linear-gradient(0deg, var(--media-shade) 0%, rgb(6 8 14 / 0) 58%);
  }

  .kind {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    opacity: 0.85;
  }

  .title {
    font-size: 20px;
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

  @media (max-width: 1023px) {
    .mosaic { grid-template-columns: repeat(4, minmax(0, 1fr)); }
    .cell { grid-column: span var(--span-tablet); }
    .hide-desktop { display: block; }
    .hide-tablet { display: none; }
  }

  @media (max-width: 639px) {
    .mosaic { grid-template-columns: repeat(2, minmax(0, 1fr)); grid-auto-rows: 110px; gap: 12px; }
    .cell { grid-column: span var(--span-phone); }
    .hide-tablet { display: block; }
    .hide-phone { display: none; }
    .tile { padding: 14px; }
    h2 { font-size: 21px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .tile, .tile:hover, .tile:active { transition: none; transform: none; }
  }
</style>
