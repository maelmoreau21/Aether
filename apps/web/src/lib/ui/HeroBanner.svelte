<script lang="ts">
  import { session } from '../auth/session';
  import { selectArtwork } from '../catalog/artwork-selection';
  import { resolve } from '$app/paths';
  import type { Work } from '../catalog/api';
  import Icon from './Icon.svelte';
  import { imagePreviewUrl } from '../catalog/image-url';
  import { history } from '../catalog/watch-state';

  let { featured = null, onPlay }: { featured?: Work | null; onPlay?: (work: Work) => void } = $props();
  const detailUrl = $derived(featured ? featured.kind === 'series' ? resolve(`series/?work=${encodeURIComponent(featured.id)}`) : resolve(`films/?work=${encodeURIComponent(featured.id)}`) : resolve('films/'));
  const artwork = $derived(selectArtwork(featured, $session));
  const imageUrl = $derived(imagePreviewUrl(artwork.backdrop, 1280));
  const logoUrl = $derived(imagePreviewUrl(artwork.logo, 640));
  let failedBackdrop = $state('');
  let failedLogo = $state('');
  const continuing = $derived($history.some(item => item.workId === featured?.id && !item.watched && item.position > 0));
</script>

<section class="hero-banner" class:has-image={!!imageUrl && failedBackdrop !== imageUrl} aria-label="À l’affiche dans votre médiathèque">
  {#if imageUrl && failedBackdrop !== imageUrl}
    <img class="backdrop-img" src={imageUrl} alt="" width="1280" height="720" loading="eager" fetchpriority="high" decoding="async" onerror={() => failedBackdrop = imageUrl} />
  {/if}
  <div class="hero-content">
    <p class="hero-eyebrow">{featured ? 'Dans votre médiathèque' : 'Votre médiathèque'}</p>
    <h1>
      {#if featured}
        {#if logoUrl && failedLogo !== logoUrl}<img class="hero-title-logo" src={logoUrl} alt={featured.title} width="440" height="150" decoding="async" onerror={() => failedLogo = logoUrl} />{:else}{featured.title}{/if}
      {:else}Vos histoires commencent ici.{/if}
    </h1>
    {#if featured}
      <div class="hero-meta">
        <span>{featured.kind === 'series' ? 'Série' : 'Film'}</span>
        {#if featured.year}<span>{featured.year}</span>{/if}
        {#if featured.mpaa}<span>{featured.mpaa}</span>{/if}
        {#if featured.kind === 'series' && featured.seasonCount}<span>{featured.seasonCount} {featured.seasonCount > 1 ? 'saisons' : 'saison'}</span>{/if}
      </div>
      {#if featured.plot || featured.description}<p class="hero-synopsis">{featured.plot || featured.description}</p>{/if}
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
  .hero-banner { position:relative; isolation:isolate; display:flex; align-items:end; min-height:360px; margin-top:24px; padding:clamp(24px,4vw,48px); border-radius:var(--radius-2xl); overflow:hidden; background:var(--color-surface); }
  .hero-banner.has-image { min-height:clamp(400px,34vw,500px); }
  .backdrop-img { position:absolute; inset:0; width:100%; height:100%; object-fit:cover; object-position:70% 30%; z-index:-2; }
  .has-image::after { content:''; position:absolute; inset:0; z-index:-1; background:linear-gradient(90deg,var(--color-canvas),color-mix(in srgb,var(--color-canvas) 90%,transparent) 32%,transparent 85%),linear-gradient(0deg,var(--color-canvas),transparent 80%); }
  .hero-content { display:flex; flex-direction:column; align-items:start; gap:16px; max-width:560px; min-width:0; }
  .hero-eyebrow { margin:0; font-size:12px; font-weight:600; color:var(--color-muted); letter-spacing:.06em; }
  h1 { margin:0; font-family:var(--font-display); font-size:clamp(32px,4vw,56px); font-weight:750; line-height:1.08; letter-spacing:-.035em; overflow-wrap:anywhere; }
  .hero-title-logo { display:block; width:auto; height:auto; max-width:min(100%,380px); max-height:130px; object-fit:contain; object-position:left; }
  .hero-meta { display:flex; gap:12px; flex-wrap:wrap; color:var(--color-muted); font-size:13px; }
  .hero-meta span+span { border-left:1px solid var(--color-line-strong); padding-left:12px; }
  .hero-synopsis { margin:0; max-width:52ch; font-size:15px; line-height:1.65; color:var(--color-text); display:-webkit-box; -webkit-box-orient:vertical; -webkit-line-clamp:3; line-clamp:3; overflow:hidden; }
  .availability { margin:0; color:var(--color-danger); font-size:14px; }
  .hero-actions { display:flex; flex-wrap:wrap; gap:12px; margin-top:6px; }
  .hero-actions a,.hero-actions button { display:flex; align-items:center; justify-content:center; gap:10px; min-height:48px; padding:12px 24px; border-radius:var(--radius-md); font-size:14px; font-weight:650; }
  .primary { background:var(--color-play); color:var(--color-on-play); border:1px solid transparent; }
  .secondary { background:var(--color-surface); color:var(--color-text); border:1px solid var(--color-line-strong); }
  .primary:hover:not(:disabled),.secondary:hover { filter:brightness(1.08); }
  @media(min-width:641px) and (max-width:1024px) { .hero-content { max-width:65%; } .has-image::after { background:linear-gradient(90deg,var(--color-canvas),color-mix(in srgb,var(--color-canvas) 90%,transparent) 40%,transparent),linear-gradient(0deg,var(--color-canvas),transparent); } }
  @media(max-width:640px) {
    .hero-banner { margin-top:16px; padding:24px 16px; border-radius:var(--radius-lg); min-height:300px; }
    .hero-banner.has-image { padding-top:170px; min-height:450px; }
    .backdrop-img { height:300px; object-position:65% 25%; mask-image:linear-gradient(#000 45%,transparent); }
    .has-image::after { background:linear-gradient(0deg,var(--color-canvas) 35%,color-mix(in srgb,var(--color-canvas) 95%,transparent) 48%,transparent 95%); }
    .hero-content { max-width:100%; gap:12px; }
    h1 { font-size:clamp(28px,8vw,38px); }
    .hero-title-logo { max-height:90px; max-width:100%; }
    .hero-synopsis { font-size:14px; -webkit-line-clamp:2; line-clamp:2; }
    .hero-actions { width:100%; gap:8px; }
    .hero-actions a,.hero-actions button { flex:1; padding:12px; white-space:nowrap; }
  }
</style>
