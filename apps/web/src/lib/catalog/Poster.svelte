<script lang="ts">
  import type { Work } from './api';
  let failedUrl = $state('');
  let { work, href = '', showMetadata = true, eager = false }: { work: Work; href?: string; showMetadata?: boolean; eager?: boolean } = $props();
  const imageUrl = $derived(work.posterUrl && work.posterUrl !== failedUrl ? work.posterUrl : '');
</script>
{#snippet posterBody()}
  <div class="poster" class:series={work.kind === 'series'}>
    {#if imageUrl}<img class="poster-image" src={imageUrl} alt={`Affiche de ${work.title}`} width="400" height="600" loading={eager ? 'eager' : 'lazy'} decoding="async" onerror={() => { failedUrl = imageUrl; }} />{/if}
    <span class="format" class:image-label={!!imageUrl}>{work.kind === 'series' ? 'SÉRIE' : 'FILM'}</span>
    {#if !imageUrl}<span class="poster-type" aria-hidden="true">{work.kind === 'series' ? 'S' : 'F'}{#if work.synthetic}<span>DÉMO</span>{/if}</span>{/if}
    {#if href}<span class="card-open" aria-hidden="true">↗</span>{/if}
  </div>
  {#if showMetadata}<h3>{work.title}</h3><p>{work.year ?? 'Année non renseignée'}{work.kind === 'series' ? ` · ${work.episodeCount ?? 0} ${(work.episodeCount ?? 0) > 1 ? 'épisodes' : 'épisode'}` : ''}</p>{#if work.genres?.length}<p class="genres">{work.genres.join(' · ')}</p>{/if}{#if work.description}<p class="synopsis">{work.description}</p>{/if}{/if}
{/snippet}
{#if href}
  <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- href already resolved by the parent route. -->
  <a class="media-card" {href} aria-label={`Voir ${work.title}`}>
    {@render posterBody()}
  </a>
{:else}
  <div class="media-card">
    {@render posterBody()}
  </div>
{/if}
<style>
  .poster-image { position:absolute; inset:0; width:100%; height:100%; object-fit:cover; }.image-label { position:relative; align-self:flex-start; padding:4px 6px; background:#101017d9; border-radius:3px; }.card-open { text-shadow:0 1px 4px #000; }
  .synopsis { display:-webkit-box; -webkit-box-orient:vertical; -webkit-line-clamp:3; line-clamp:3; overflow:hidden; margin-top:5px; overflow-wrap:anywhere; }
  .media-card { display:block; min-width:0; }.poster { aspect-ratio:2/3; background:#191523; border:1px solid #b4a1f823; border-radius:10px; position:relative; overflow:hidden; display:flex; flex-direction:column; padding:20px; justify-content:space-between; transition:transform .2s,border-color .2s; }.series { background:#141b24; border-color:#9fbbd926; }a.media-card:hover .poster { transform:translateY(-4px); border-color:var(--color-accent); }.format { font-size:9px; letter-spacing:.2em; color:#c3b6e0; }.poster-type { align-self:center; font-family:Georgia,serif; font-size:clamp(60px,8vw,110px); color:#b4a1f86b; text-align:center; line-height:1; }.poster-type span { display:block; font-family:var(--font-sans); font-size:8px; line-height:1.8; letter-spacing:.22em; margin-top:16px; color:#a59db5; }.card-open { position:absolute; bottom:16px; right:16px; color:var(--color-accent); }h3 { font-size:14px; font-weight:550; line-height:1.5; margin:13px 0 4px; overflow-wrap:anywhere; }p { color:var(--color-muted); font-size:11px; line-height:1.6; margin:0; }
</style>
