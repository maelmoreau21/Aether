<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { Work } from './api';
  import { session } from '../auth/session';
  import { selectArtwork } from './artwork-selection';
  import { imagePreviewUrl } from './image-url';
  import Poster from './Poster.svelte';

  let { work, duration, children }: { work: Work; duration?: number; children: Snippet } = $props();
  const artwork = $derived(selectArtwork(work, $session));
  const backdrop = $derived(imagePreviewUrl(artwork.backdrop, 1280));
  const logo = $derived(imagePreviewUrl(artwork.logo, 640));
  let failedBackdrop = $state('');
  let failedLogo = $state('');
  const hasBackdrop = $derived(Boolean(backdrop && backdrop !== failedBackdrop));
  const meaningful = (value?: string | null) => Boolean(value?.trim() && !['Média local indexé', 'Média local'].includes(value.trim()));
  const synopsis = $derived(meaningful(work.plot) ? work.plot : meaningful(work.description) ? work.description : 'Synopsis non renseigné.');
</script>

<section class="detail-hero" class:has-backdrop={hasBackdrop} aria-label={`Présentation de ${work.title}`}>
  {#if hasBackdrop}<img class="detail-backdrop" src={backdrop} alt="" width="1280" height="720" fetchpriority="high" decoding="async" onerror={() => failedBackdrop = backdrop} />{/if}
  <div class="hero-layout">
    <div class="detail-poster-column"><Poster {work} showMetadata={false} /></div>
    <div class="detail-info-column">
      <p class="eyebrow">{work.kind === 'series' ? 'Série' : 'Film'}{work.synthetic ? ' · Démonstration' : ''}</p>
      <h1>{#if logo && logo !== failedLogo}<img class="title-logo" src={logo} alt={work.title} width="640" height="200" decoding="async" onerror={() => failedLogo = logo} />{:else}{work.title}{/if}</h1>
      <div class="essential-meta">
        {#if work.year}<span>{work.year}</span>{/if}
        {#if duration && Number.isFinite(duration) && duration > 0}<span>{Math.ceil(duration / 60)} min</span>{/if}
        {#if work.mpaa}<span>{work.mpaa}</span>{/if}
        {#if work.kind === 'series' && work.seasonCount}<span>{work.seasonCount} {work.seasonCount > 1 ? 'saisons' : 'saison'}</span>{/if}
        {#if work.rating}<span>Note {work.rating.toLocaleString('fr-FR')} / 10</span>{/if}
      </div>
      {#if synopsis && synopsis.length > 300}
        <details class="long-synopsis"><summary><span>{synopsis.slice(0, 240).trimEnd()}…</span><span class="read-more">Lire le résumé</span></summary><p>{synopsis}</p></details>
      {:else}<p class="synopsis">{synopsis}</p>{/if}
      {#if work.genres?.length}<p class="genres">{work.genres.join(' · ')}</p>{/if}
      {#if work.available === false}<p class="availability" role="status">Ce contenu est temporairement indisponible. Le serveur qui l’héberge est hors ligne.</p>{/if}
      {@render children()}
      {#if (work.originalTitle && work.originalTitle !== work.title) || (meaningful(work.outline) && work.outline !== synopsis)}
        <details class="editorial-details"><summary>À propos de l’œuvre</summary>
          {#if work.originalTitle && work.originalTitle !== work.title}<p>Titre original : <em>{work.originalTitle}</em></p>{/if}
          {#if meaningful(work.outline) && work.outline !== synopsis}<p>{work.outline}</p>{/if}
        </details>
      {/if}
    </div>
  </div>
</section>

<style>
  .detail-hero { position:relative; isolation:isolate; margin-top:24px; padding:clamp(24px,4vw,48px); background:var(--color-surface); border-radius:var(--radius-2xl); overflow:hidden; }
  .detail-backdrop { position:absolute; inset:0; width:100%; height:100%; object-fit:cover; object-position:75% 25%; z-index:-2; }
  .has-backdrop::after { content:''; position:absolute; inset:0; z-index:-1; background:linear-gradient(90deg,var(--color-canvas),color-mix(in srgb,var(--color-canvas) 94%,transparent) 45%,color-mix(in srgb,var(--color-canvas) 45%,transparent)),linear-gradient(0deg,var(--color-canvas),transparent); }
  .hero-layout { display:grid; grid-template-columns:clamp(160px,18vw,240px) minmax(0,1fr); gap:clamp(24px,4vw,48px); align-items:center; }
  .detail-info-column { display:flex; flex-direction:column; align-items:start; gap:16px; min-width:0; overflow-wrap:anywhere; }
  .eyebrow { margin:0; color:var(--color-muted); font-size:12px; font-weight:600; }
  h1 { margin:0; font-family:var(--font-display); font-size:clamp(30px,4vw,52px); line-height:1.1; letter-spacing:-.035em; font-weight:750; max-width:22ch; }
  .title-logo { display:block; width:auto; height:auto; max-width:min(100%,380px); max-height:120px; object-fit:contain; object-position:left; }
  .essential-meta { display:flex; flex-wrap:wrap; gap:8px 16px; color:var(--color-muted); font-size:14px; }
  .essential-meta:empty { display:none; }
  .synopsis,.long-synopsis { max-width:65ch; margin:0; font-size:15px; line-height:1.7; }
  .long-synopsis summary { cursor:pointer; }
  .read-more { display:block; color:var(--color-accent); font-size:13px; font-weight:600; }
  .long-synopsis[open] summary>span:first-child { display:none; }
  .long-synopsis p { margin:8px 0 0; }
  .genres { margin:0; font-size:13px; color:var(--color-muted); }
  .editorial-details { max-width:65ch; font-size:13px; color:var(--color-muted); line-height:1.6; }
  .editorial-details summary { min-height:44px; display:list-item; padding-block:10px; cursor:pointer; }
  .availability { margin:0; max-width:65ch; color:var(--color-danger); font-size:14px; line-height:1.6; }
  @media(min-width:1600px) { .detail-info-column { max-width:760px; } }
  @media(max-width:820px) {
    .hero-layout { grid-template-columns:minmax(0,1fr); }
    .detail-poster-column { max-width:110px; }
    .has-backdrop .detail-poster-column { display:none; }
    .has-backdrop { padding-top:180px; }
    .detail-backdrop { height:300px; mask-image:linear-gradient(#000 30%,transparent); }
    .has-backdrop::after { background:linear-gradient(0deg,var(--color-canvas) 30%,color-mix(in srgb,var(--color-canvas) 85%,transparent) 65%,transparent); }
    .title-logo { max-height:100px; }
  }
  @media(max-width:480px) { .detail-hero { margin-top:16px; padding:24px 16px; border-radius:var(--radius-lg); } .has-backdrop { padding-top:160px; } .detail-info-column { gap:12px; } h1 { font-size:30px; } }
</style>
