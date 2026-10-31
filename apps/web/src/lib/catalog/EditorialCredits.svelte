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

{#if work.directors?.length || work.writers?.length || work.actors?.length}
  <section aria-label="Distribution et équipe" class="credits-section">
    <h2>Distribution et équipe</h2>

    {#if work.directors?.length || work.writers?.length}
      <div class="crew-cards">
        {#if work.directors?.length}
          <div class="crew-card">
            <span class="crew-role">{work.directors.length > 1 ? 'Réalisateurs' : 'Réalisateur'}</span>
            <strong class="crew-names">{work.directors.join(', ')}</strong>
          </div>
        {/if}
        {#if work.writers?.length}
          <div class="crew-card">
            <span class="crew-role">{work.writers.length > 1 ? 'Scénaristes' : 'Scénario'}</span>
            <strong class="crew-names">{work.writers.join(', ')}</strong>
          </div>
        {/if}
      </div>
    {/if}

    {#if work.actors?.length}
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
    {/if}
  </section>
{/if}

<style>
  .provider-ratings { display: flex; flex-wrap: wrap; gap: 10px; margin-block: 16px; }
  .provider-ratings > span { padding: 7px 14px; background: var(--color-surface); border: 1px solid var(--color-line); border-radius: var(--radius-pill); font-size: 13px; color: var(--color-text); }
  .provider-ratings > span strong { color: var(--color-accent); margin-right: 6px; }

  .credits-section { margin-block: 32px; }
  h2 { font-family: var(--font-display); font-size: 22px; font-weight: 750; letter-spacing: -0.025em; margin: 0 0 18px; color: var(--color-text-bright); }

  .crew-cards {
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
    margin-bottom: 22px;
  }

  .crew-card {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px 18px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    min-width: 180px;
  }

  .crew-role {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--color-accent);
  }

  .crew-names {
    font-size: 14.5px;
    font-weight: 650;
    color: var(--color-text-bright);
  }

  .cast-list { display: flex; gap: 18px; overflow-x: auto; padding: 6px 4px 14px; scrollbar-width: thin; }
  article { flex: 0 0 132px; display: flex; flex-direction: column; gap: 6px; font-size: 13px; text-align: center; transition: transform var(--transition-fast); }
  article:hover { transform: translateY(-3px); }
  article strong { color: var(--color-text-bright); font-weight: 650; }
  article span { color: var(--text-secondary); font-size: 12px; }
  .portrait { display: grid; place-items: center; aspect-ratio: 1; background: var(--color-raised); border: 1px solid var(--color-line); font-size: 34px; font-weight: 700; border-radius: 50%; color: var(--text-secondary); overflow: hidden; margin: 0 auto 6px; width: 98px; height: 98px; box-shadow: var(--shadow-sm); }
  .portrait-frame { border: 2px solid var(--color-line-strong); background: var(--color-surface); }
  .portrait-img { width: 100%; height: 100%; object-fit: cover; }
</style>
