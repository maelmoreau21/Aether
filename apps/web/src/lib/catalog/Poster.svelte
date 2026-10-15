<script lang="ts">
  import type { Work } from './api';
  let { work, href = '', showMetadata = true }: { work: Work; href?: string; showMetadata?: boolean } = $props();
</script>
{#if href}
  <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- href already resolved by the parent route. -->
  <a class="media-card" {href} aria-label={`Voir ${work.title}`}>
    <div class="poster" class:series={work.kind === 'series'}>
      <span class="format">{work.kind === 'series' ? 'SÉRIE' : 'FILM'}</span>
      <span class="poster-type" aria-hidden="true">{work.kind === 'series' ? 'S' : 'F'}{#if work.synthetic}<span>DÉMO</span>{/if}</span>
      <span class="card-open" aria-hidden="true">↗</span>
    </div>
    {#if showMetadata}<h3>{work.title}</h3><p>{work.year ?? 'Année non renseignée'}{work.kind === 'series' ? ` · ${work.episodeCount ?? 0} ${(work.episodeCount ?? 0) > 1 ? 'épisodes' : 'épisode'}` : ''}</p>{/if}
  </a>
{:else}
  <div class="media-card">
    <div class="poster" class:series={work.kind === 'series'}>
      <span class="format">{work.kind === 'series' ? 'SÉRIE' : 'FILM'}</span>
      <span class="poster-type" aria-hidden="true">{work.kind === 'series' ? 'S' : 'F'}{#if work.synthetic}<span>DÉMO</span>{/if}</span>
    </div>
    {#if showMetadata}<h3>{work.title}</h3><p>{work.year ?? 'Année non renseignée'}{work.kind === 'series' ? ` · ${work.episodeCount ?? 0} ${(work.episodeCount ?? 0) > 1 ? 'épisodes' : 'épisode'}` : ''}</p>{/if}
  </div>
{/if}
<style>
  .media-card { display:block; min-width:0; }.poster { aspect-ratio:2/3; background:#191523; border:1px solid #b4a1f823; border-radius:10px; position:relative; overflow:hidden; display:flex; flex-direction:column; padding:20px; justify-content:space-between; transition:transform .2s,border-color .2s; }.series { background:#141b24; border-color:#9fbbd926; }a.media-card:hover .poster { transform:translateY(-4px); border-color:var(--color-accent); }.format { font-size:9px; letter-spacing:.2em; color:#c3b6e0; }.poster-type { align-self:center; font-family:Georgia,serif; font-size:clamp(60px,8vw,110px); color:#b4a1f86b; text-align:center; line-height:1; }.poster-type span { display:block; font-family:var(--font-sans); font-size:8px; line-height:1.8; letter-spacing:.22em; margin-top:16px; color:#a59db5; }.card-open { position:absolute; bottom:16px; right:16px; color:var(--color-accent); }h3 { font-size:14px; font-weight:550; line-height:1.5; margin:13px 0 4px; overflow-wrap:anywhere; }p { color:var(--color-muted); font-size:11px; line-height:1.6; margin:0; }
</style>
