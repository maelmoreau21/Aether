<script lang="ts">
  import type { Work } from './api';
  const providers:Record<string,string> = { imdb:'IMDb',tmdb:'TMDb',themoviedb:'TMDb',tvdb:'TVDB',tomatometer:'Rotten Tomatoes',metacritic:'Metacritic' };
  let { work }: { work:Work } = $props();
</script>
{#if work.ratings?.length}
  <section aria-label="Notes des fournisseurs" class="provider-ratings">
    {#each work.ratings as rating, index (index)}<span><strong>{providers[rating.provider.toLowerCase()] || rating.provider || 'Note du NFO'}</strong> {rating.value.toLocaleString('fr-FR', { maximumFractionDigits:1 })} / {rating.max}</span>{/each}
  </section>
{/if}
{#if work.directors?.length || work.writers?.length}
  <section class="crew" aria-label="Équipe technique">
    {#if work.directors?.length}<p><strong>Réalisation</strong> {work.directors.join(', ')}</p>{/if}
    {#if work.writers?.length}<p><strong>Scénario</strong> {work.writers.join(', ')}</p>{/if}
  </section>
{/if}
{#if work.actors?.length}
  <section aria-label="Distribution" class="cast">
    <h2>Distribution</h2>
    <div class="cast-list">{#each work.actors as actor, index (index)}<article><div class="portrait" aria-hidden="true">{actor.name.slice(0,1)}</div><strong>{actor.name}</strong>{#if actor.role}<span>{actor.role}</span>{/if}</article>{/each}</div>
  </section>
{/if}
<style>
.provider-ratings {display:flex;flex-wrap:wrap;gap:12px;margin-block:12px;} .provider-ratings>span {padding:10px;background:var(--bg-surface);border:1px solid var(--border-subtle);border-radius:6px;}
.crew {color:var(--text-secondary);font-size:14px;} .crew strong {color:var(--text-primary);margin-right:8px;}
.cast { margin-block:24px; } h2 {font-size:20px;} .cast-list {display:flex;gap:16px;overflow-x:auto;padding-bottom:12px;} article {flex:0 0 130px;display:flex;flex-direction:column;gap:6px;font-size:13px;} article span {color:var(--text-secondary);} .portrait {display:grid;place-items:center;aspect-ratio:1;background:var(--bg-elevated);font-size:42px;border-radius:50%;color:var(--text-secondary);}
</style>
