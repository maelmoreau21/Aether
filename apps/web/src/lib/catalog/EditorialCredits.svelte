<script lang="ts">
  import type { Work } from './api';
  const providers: Record<string, string> = {
    imdb: 'IMDb',
    tmdb: 'TMDb',
    themoviedb: 'TMDb',
    tvdb: 'TVDB',
    tomatometer: 'Rotten Tomatoes',
    metacritic: 'Metacritic'
  };
  let { work }: { work: Work } = $props();
</script>

{#if work.ratings?.length}
  <section aria-label="Notes des fournisseurs" class="provider-ratings">
    {#each work.ratings as rating, index (index)}
      <span>
        <strong>{providers[rating.provider.toLowerCase()] || rating.provider || 'Note'}</strong>
        {rating.value.toLocaleString('fr-FR', { maximumFractionDigits: 1 })} / {rating.max}
      </span>
    {/each}
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
    <div class="cast-list">
      {#each work.actors as actor, index (index)}
        {@const photoUrl = actor.thumbUrl || actor.thumb_url}
        <article>
          {#if photoUrl}
            <div class="portrait portrait-frame">
              <img class="portrait-img" src={photoUrl} alt={actor.name} loading="lazy" />
            </div>
          {:else}
            <div class="portrait" aria-hidden="true">{actor.name.slice(0, 1)}</div>
          {/if}
          <strong>{actor.name}</strong>
          {#if actor.role}<span>{actor.role}</span>{/if}
        </article>
      {/each}
    </div>
  </section>
{/if}

<style>
  .provider-ratings { display: flex; flex-wrap: wrap; gap: 10px; margin-block: 16px; }
  .provider-ratings > span { padding: 7px 14px; background: var(--color-surface); border: 1px solid var(--color-line); border-radius: var(--radius-pill); font-size: 13px; color: var(--color-text); }
  .provider-ratings > span strong { color: var(--color-accent); margin-right: 6px; }
  .crew { color: var(--text-secondary); font-size: 14px; margin-block: 10px; }
  .crew strong { color: var(--text-primary); margin-right: 8px; }
  .cast { margin-block: 28px; }
  h2 { font-family: var(--font-display); font-size: 21px; font-weight: 700; letter-spacing: -0.02em; margin-bottom: 16px; color: var(--color-text-bright); }
  .cast-list { display: flex; gap: 18px; overflow-x: auto; padding: 6px 4px 14px; scrollbar-width: thin; }
  article { flex: 0 0 130px; display: flex; flex-direction: column; gap: 6px; font-size: 13px; text-align: center; transition: transform var(--transition-fast); }
  article:hover { transform: translateY(-3px); }
  article strong { color: var(--color-text-bright); font-weight: 600; }
  article span { color: var(--text-secondary); font-size: 12px; }
  .portrait { display: grid; place-items: center; aspect-ratio: 1; background: var(--color-raised); border: 1px solid var(--color-line); font-size: 34px; font-weight: 700; border-radius: 50%; color: var(--text-secondary); overflow: hidden; margin: 0 auto 6px; width: 96px; height: 96px; box-shadow: var(--shadow-sm); }
  .portrait-frame { border: 2px solid var(--color-line-strong); background: var(--color-surface); }
  .portrait-img { width: 100%; height: 100%; object-fit: cover; }
</style>
