<script lang="ts">
  import { session } from '../auth/session';
  import { selectArtwork } from '../catalog/artwork-selection';
  import { resolve } from '$app/paths';
  import { type Work, localizedTitle, localizedPlot } from '../catalog/api';
  import { locale, t } from '../i18n';
  import Icon from './Icon.svelte';
  import { imagePreviewUrl } from '../catalog/image-url';
  import { history } from '../catalog/watch-state';
  import { isResumable } from '../player/progress';
  import { serverPresentation } from '../config/presentation';
  import { imageTint } from './ambient-tint';
  import { fitArtwork } from './fit-artwork';

  let { featured = null, candidates = [], onPlay, onTint }: {
    featured?: Work | null;
    /** Jusqu'à quatre œuvres à l'affiche ; la première est montrée d'abord. */
    candidates?: Work[];
    onPlay?: (work: Work) => void;
    /** Couleur dominante de la grande image active, pour la couche d'ambiance de la page. */
    onTint?: (color: string | null) => void;
  } = $props();

  const uid = $props.id();
  const panelId = `${uid}-hero`;
  const list = $derived((candidates.length ? candidates : featured ? [featured] : []).slice(0, 4));
  let activeId = $state<string | null>(null);
  const active = $derived(list.find(work => work.id === activeId) ?? list[0] ?? null);
  const switcher = $derived(list.length > 1);

  // backdrop.jpg first, landscape.jpg as a wide fallback.
  // Sans image large, l'affiche floutée sert de fond d'ambiance (jamais rognée en net).
  function heroImage(work: Work): string {
    const artwork = selectArtwork(work, $session);
    return imagePreviewUrl(artwork.backdrop || artwork.landscape || artwork.poster, 1280);
  }
  function isAmbient(work: Work): boolean {
    const artwork = selectArtwork(work, $session);
    return !artwork.backdrop && !artwork.landscape && Boolean(artwork.poster);
  }
  // Vignettes du sélecteur : landscape.jpg, puis backdrop.
  function tabImage(work: Work): string {
    const artwork = selectArtwork(work, $session);
    return imagePreviewUrl(artwork.landscape || artwork.backdrop || artwork.poster, 640);
  }

  const detailUrl = $derived(
    active
      ? active.kind === 'series'
        ? resolve(`series/?work=${encodeURIComponent(active.id)}`)
        : resolve(`films/?work=${encodeURIComponent(active.id)}`)
      : resolve('films/')
  );
  const artwork = $derived(selectArtwork(active, $session));
  const imageUrl = $derived(active ? heroImage(active) : '');
  const logoUrl = $derived(imagePreviewUrl(artwork.logo, 640));
  const clearartUrl = $derived(imagePreviewUrl(artwork.illustration, 640));
  let failedImages = $state<string[]>([]);
  const usable = (url: string) => Boolean(url) && !failedImages.includes(url);
  const fail = (url: string) => { if (!failedImages.includes(url)) failedImages = [...failedImages, url]; };
  const hasImage = $derived(usable(imageUrl));
  const hasClearart = $derived(usable(clearartUrl));
  const continuing = $derived($history.some(item => item.workId === active?.id && isResumable(item, $serverPresentation)));
  const displayTitle = $derived(active ? localizedTitle(active, $locale) : '');
  const displayPlot = $derived(active ? localizedPlot(active, $locale) : '');

  // Teinte d'ambiance : calculée une fois par image chargée, réutilisée au changement d'œuvre.
  let tints = $state<Record<string, string | null>>({});
  const activeTint = $derived(hasImage ? tints[imageUrl] ?? null : null);
  $effect(() => { onTint?.(activeTint); });
  $effect(() => () => onTint?.(null));

  function sampleTint(node: HTMLImageElement, url: string) {
    let current = url;
    const read = () => { if (!(current in tints)) tints[current] = imageTint(node); };
    const listen = () => {
      if (node.complete && node.naturalWidth > 0) read();
      else node.addEventListener('load', read, { once: true });
    };
    listen();
    return {
      update(next: string) { node.removeEventListener('load', read); current = next; listen(); },
      destroy() { node.removeEventListener('load', read); }
    };
  }

  let tabs = $state<HTMLButtonElement[]>([]);
  let tablist = $state<HTMLDivElement>();

  function select(work: Work) { activeId = work.id; }

  // Flèches selon l'orientation affichée (colonne sur PC, rangée sur tablette et téléphone).
  function onTabKeydown(event: KeyboardEvent, index: number) {
    const vertical = tablist ? getComputedStyle(tablist).flexDirection === 'column' : false;
    const previous = vertical ? 'ArrowUp' : 'ArrowLeft';
    const next = vertical ? 'ArrowDown' : 'ArrowRight';
    let target = -1;
    if (event.key === next) target = (index + 1) % list.length;
    else if (event.key === previous) target = (index - 1 + list.length) % list.length;
    else if (event.key === 'Home') target = 0;
    else if (event.key === 'End') target = list.length - 1;
    if (target < 0) return;
    event.preventDefault();
    select(list[target]);
    tabs[target]?.focus();
  }
</script>

<section class="stage" class:has-switcher={switcher} aria-label={$t('now_showing_in_your_library')}>
  <div class="main" class:has-clearart={hasClearart}>
    <div
      class="hero"
      class:has-image={hasImage}
      id={panelId}
      role={switcher ? 'tabpanel' : undefined}
      aria-labelledby={switcher && active ? `${uid}-tab-${active.id}` : undefined}
    >
      {#each list as work, index (work.id)}
        {@const url = heroImage(work)}
        {#if usable(url)}
          <img
            class="backdrop"
            class:on={work.id === active?.id}
            class:ambient={isAmbient(work)}
            src={url}
            alt=""
            width="1280"
            height="720"
            loading={index === 0 ? 'eager' : 'lazy'}
            fetchpriority={index === 0 ? 'high' : 'low'}
            decoding="async"
            use:sampleTint={url}
            use:fitArtwork={{ adapt: false }}
            onerror={() => fail(url)}
          />
        {/if}
      {/each}
      {#if hasImage}<div class="shade" aria-hidden="true"></div>{/if}
      {#key active?.id}
        <div class="cap">
          {#if active}
            <div class="tag">
              <span class="badge">{active.kind === 'series' ? 'Série' : 'Film'}</span>
              {#if active.year}<span>{active.year}</span>{/if}
              {#if active.kind === 'series' && active.seasonCount}<span>{active.seasonCount} {active.seasonCount > 1 ? 'saisons' : 'saison'}</span>{/if}
              {#if active.genres?.length}<span>{active.genres.slice(0, 3).join(' · ')}</span>{/if}
              {#if active.mpaa}<span class="badge">{active.mpaa}</span>{/if}
              {#if active.rating}<span class="rating"><Icon name="star" size={13} /> {active.rating.toFixed(1)}</span>{/if}
            </div>
            <h1>
              {#if usable(logoUrl)}
                <span class="h-sr">{displayTitle}</span>
                <img class="title-logo" src={logoUrl} alt="" width="460" height="150" decoding="async" onerror={() => fail(logoUrl)} />
              {:else}{displayTitle}{/if}
            </h1>
            {#if displayPlot}<p class="synopsis">{displayPlot}</p>{/if}
            {#if active.available === false}<p class="availability" role="status">{$t('this_content_is_temporarily_unavailable')}</p>{/if}
            <div class="acts">
              {#if onPlay}
                {@const work = active}
                <button class="play" onclick={() => onPlay(work)} disabled={work.available === false}>
                  <Icon name="play" size={20} /><span>{work.kind === 'series' ? $t('view_episodes') : continuing ? 'Reprendre' : 'Lecture'}</span>
                </button>
              {/if}
              <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- URL resolved above. -->
              <a class="more" href={detailUrl}><Icon name="info" size={20} /><span>{$t('more_info')}</span></a>
            </div>
          {:else}
            <h1>{$t('your_stories_start_here')}</h1>
            <p class="synopsis">{$t('the_films_and_series_in_your')}</p>
            <div class="acts"><a class="more" href={resolve('films/')}><Icon name="film" size={20} /><span>{$t('explore_films')}</span></a></div>
          {/if}
        </div>
      {/key}
    </div>
    {#if hasClearart}
      {#key clearartUrl}
        <img class="clearart" src={clearartUrl} alt="" width="640" height="360" decoding="async" loading="lazy" onerror={() => fail(clearartUrl)} />
      {/key}
    {/if}
  </div>

  {#if switcher}
    <div class="queue" role="tablist" aria-label={$t('choose_the_featured_work')} bind:this={tablist}>
      {#each list as work, index (work.id)}
        {@const thumb = tabImage(work)}
        {@const selected = work.id === active?.id}
        <button
          type="button"
          class="qi"
          class:on={selected}
          class:has-image={usable(thumb)}
          role="tab"
          id={`${uid}-tab-${work.id}`}
          aria-selected={selected}
          aria-controls={panelId}
          tabindex={selected ? 0 : -1}
          bind:this={tabs[index]}
          onclick={() => select(work)}
          onkeydown={event => onTabKeydown(event, index)}
        >
          {#if usable(thumb)}
            <img src={thumb} alt="" width="640" height="360" loading="lazy" decoding="async" use:fitArtwork={{ adapt: false }} onerror={() => fail(thumb)} />
            <span class="qi-shade" aria-hidden="true"></span>
          {/if}
          <span class="qi-title">{localizedTitle(work, $locale)}</span>
        </button>
      {/each}
    </div>
  {/if}
</section>

<style>
  .stage {
    position: relative;
    margin-top: 12px;
  }

  /* PC : grande image + colonne de vignettes à droite */
  .stage.has-switcher {
    display: grid;
    grid-template-columns: minmax(0, 1fr) clamp(232px, 12vw, 420px);
    gap: 16px;
  }

  .main {
    position: relative;
    min-width: 0;
  }

  .hero {
    position: relative;
    isolation: isolate;
    display: flex;
    align-items: flex-end;
    min-height: 360px;
    overflow: hidden;
    border-radius: var(--r-xl);
    background: var(--bg-2);
    color: var(--tx);
  }

  .hero.has-image {
    min-height: clamp(440px, 40vw, 580px);
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
    opacity: 0;
    transform: scale(1.01);
    transition: opacity var(--t-slow) var(--ease), transform 8s ease-out;
  }

  /* Fondu enchaîné entre les œuvres à l'affiche */
  .backdrop.on { opacity: 1; }

  /* Seule l'affiche existe : fond d'ambiance flouté plutôt qu'une affiche rognée */
  .backdrop.ambient { object-position: center; filter: blur(36px) saturate(1.2) brightness(0.6); transform: scale(1.2); }
  .hero:hover .backdrop.ambient { transform: scale(1.2); }

  .hero:hover .backdrop { transform: scale(1.035); }

  /* Voile de lisibilité : seul dégradé autorisé sur une image */
  .shade {
    position: absolute;
    inset: 0;
    z-index: -1;
    background:
      linear-gradient(0deg, var(--media-shade) 0%, rgb(6 8 14 / 0.35) 45%, rgb(6 8 14 / 0) 80%),
      linear-gradient(90deg, rgb(6 8 14 / 0.55) 0%, rgb(6 8 14 / 0) 60%);
  }

  .cap {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 14px;
    width: 100%;
    max-width: calc(680px * var(--ui-scale, 1));
    padding: clamp(20px, 3vw, calc(40px * var(--ui-scale, 1)));
    animation: cap-in var(--t-slow) var(--ease);
  }

  @keyframes cap-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .main.has-clearart .cap { max-width: min(calc(680px * var(--ui-scale, 1)), 58%); }

  .tag {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 12px;
    font-size: calc(13px * var(--ui-scale, 1));
    font-weight: 700;
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
  }

  .hero:not(.has-image) .badge { background: var(--pn-2); }

  .rating { display: inline-flex; align-items: center; gap: 4px; }

  h1 {
    margin: 0;
    font-size: clamp(36px, 5vw, calc(68px * var(--ui-scale, 1)));
    font-weight: 800;
    letter-spacing: -0.035em;
    line-height: 0.95;
    max-width: 16ch;
  }

  .title-logo {
    display: block;
    width: auto;
    height: auto;
    max-width: min(100%, calc(460px * var(--ui-scale, 1)));
    max-height: calc(150px * var(--ui-scale, 1));
    object-fit: contain;
    object-position: left bottom;
    filter: drop-shadow(0 4px 24px rgb(0 0 0 / 0.35));
  }

  .synopsis {
    margin: 0;
    max-width: 56ch;
    font-size: calc(15px * var(--ui-scale, 1));
    line-height: 1.6;
    opacity: 0.88;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .availability {
    margin: 0;
    font-size: 14px;
    font-weight: 700;
    color: var(--warn);
  }

  .acts {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 6px;
  }

  .play,
  .more {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    height: calc(56px * var(--ui-scale, 1));
    padding: 0 calc(24px * var(--ui-scale, 1));
    border: 0;
    border-radius: 18px;
    font-size: calc(15px * var(--ui-scale, 1));
    font-weight: 700;
    transition: transform var(--t-fast) var(--ease), background-color var(--t-fast) ease, opacity var(--t-fast) ease;
  }

  .play {
    background: var(--on-media);
    color: #0b0e18;
  }

  .play:hover:not(:disabled) { opacity: 0.9; }
  .play:active, .more:active { transform: scale(0.98); }

  .more {
    background: rgb(255 255 255 / 0.16);
    color: var(--on-media);
  }

  .more:hover { background: rgb(255 255 255 / 0.24); }

  .hero:not(.has-image) .play { background: var(--btn); color: var(--btn-tx); }
  .hero:not(.has-image) .more { background: var(--pn-2); color: var(--tx); }

  /* clearart.png : dépasse du haut de la grande image */
  .clearart {
    position: absolute;
    right: clamp(24px, 6vw, 96px);
    bottom: 0;
    z-index: 2;
    width: auto;
    height: min(116%, 640px);
    max-width: 40%;
    object-fit: contain;
    object-position: bottom right;
    pointer-events: none;
    filter: drop-shadow(0 18px 40px rgb(0 0 0 / 0.45));
  }

  /* Sélecteur d'œuvres : vignettes 16:9 (landscape.jpg) */
  /* Colonne de vignettes : réparties sur toute la hauteur de la grande image, sans vide en haut. */
  .queue {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    gap: 12px;
    min-width: 0;
  }

  .qi {
    position: relative;
    isolation: isolate;
    display: flex;
    align-items: flex-end;
    width: 100%;
    min-height: 44px;
    aspect-ratio: 16 / 9;
    padding: 12px;
    overflow: hidden;
    border: 0;
    border-radius: var(--r-md);
    background: var(--pn-2);
    color: var(--tx);
    font: inherit;
    text-align: left;
    cursor: pointer;
    opacity: 0.6;
    transition: opacity var(--t-med) var(--ease), transform var(--t-fast) var(--ease);
  }

  .qi.has-image { background: var(--bg-2); color: var(--on-media); }

  .qi:hover { opacity: 0.85; }
  .qi:active { transform: scale(0.98); }
  .qi.on { opacity: 1; }

  /* Œuvre sélectionnée : contour en plus de l'opacité, jamais la couleur seule */
  .qi.on::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    box-shadow: inset 0 0 0 2px var(--on-media);
    pointer-events: none;
  }

  .qi:not(.has-image).on::after { box-shadow: inset 0 0 0 2px var(--tx); }

  .qi:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; opacity: 1; }

  .qi img {
    position: absolute;
    inset: 0;
    z-index: -2;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .qi-shade {
    position: absolute;
    inset: 0;
    z-index: -1;
    background: linear-gradient(0deg, var(--media-shade) 0%, rgb(6 8 14 / 0) 70%);
  }

  .qi-title {
    display: block;
    min-width: 0;
    overflow: hidden;
    font-size: calc(14px * var(--ui-scale, 1));
    font-weight: 700;
    line-height: 1.2;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* TV / grands écrans : la grande image garde des proportions de salon
     au lieu de plafonner à 580 px de haut sur toute la largeur. */
  @media (min-width: 1920px) {
    .hero.has-image { min-height: clamp(580px, 34vw, min(1100px, 70vh)); }
  }

  /* Tablette et téléphone : vignettes en rangée sous la grande image */
  @media (max-width: 1023px) {
    .stage.has-switcher { grid-template-columns: minmax(0, 1fr); gap: 12px; }
    .queue { flex-direction: row; }
    .qi { flex: 1 1 0; min-width: 0; }
  }

  @media (max-width: 760px) {
    /* Téléphone : l'image large reste entière (16:9) en haut, le texte passe dessous. */
    .hero.has-image { min-height: 0; padding-top: calc(56.25% - 64px); }
    .hero.has-image .backdrop { inset: 0 0 auto 0; height: auto; aspect-ratio: 16 / 9; }
    .hero.has-image .backdrop.ambient { inset: 0; height: 100%; aspect-ratio: auto; }
    .hero.has-image .shade { inset: 0 0 auto 0; aspect-ratio: 16 / 9; background: linear-gradient(0deg, #06080e 0%, rgb(6 8 14 / 0.35) 40%, rgb(6 8 14 / 0) 100%); }

    .hero { border-radius: var(--r-lg); }
    .cap, .main.has-clearart .cap { max-width: none; }
    .acts { width: 100%; }
    .play { flex: 1; justify-content: center; height: 50px; }
    .more { height: 50px; }
    .title-logo { max-width: 80%; max-height: 96px; }
    .clearart { display: none; }
  }

  @media (max-width: 639px) {
    /* Téléphone : quatre vignettes en 2 × 2 plutôt qu'une rangée illisible. */
    .queue { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
    .qi { padding: 8px; border-radius: var(--r-sm); }
    .qi-title { font-size: 12px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .backdrop, .hero:hover .backdrop { transform: none; transition: none; }
    .cap { animation: none; }
    .qi, .qi:active { transition: none; transform: none; }
  }
</style>
