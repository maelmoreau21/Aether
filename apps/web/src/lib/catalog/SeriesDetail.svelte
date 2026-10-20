<script lang="ts">
  import type { Work } from './api';
  import PersonalFeedback from './PersonalFeedback.svelte';
  import EditorialCredits from './EditorialCredits.svelte';
  import type { Media } from '../player/types';
  import type ImmersivePlayer from '../player/ImmersivePlayer.svelte';
  import Poster from './Poster.svelte';
  import WatchStatus from './WatchStatus.svelte';
  import EpisodePlayer from './EpisodePlayer.svelte';
  import Icon from '../ui/Icon.svelte';

  let {
    work,
    season = $bindable(1),
    media = [],
    mediaError = '',
    immersive,
    launchingEpisodeId = null,
    onLaunchEpisode,
    onSelectEpisode
  }: {
    work: Work;
    season: number;
    media: Media[];
    mediaError?: string;
    immersive?: ImmersivePlayer;
    launchingEpisodeId?: string | null;
    onLaunchEpisode: (episode: Work, versionId: string) => Promise<void>;
    onSelectEpisode: (episode: Work) => void;
  } = $props();

  let backdropLoaded = $state(false);
  const activeSeason = $derived(work.seasons?.find(item => item.number === season) ?? work.seasons?.[0]);
  function selectSeason(number: number) {
    season = number;
    document.getElementById('episodes-shelf-title')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }
</script>

<div class="series-detail-view">
  <!-- Series Backdrop Banner -->
  <div class="detail-hero">
    {#if work.artwork?.backdrop || work.posterUrl}
      <div class="backdrop-art-container" aria-hidden="true">
        <img
          class="backdrop-art-image"
          class:artwork={!!work.artwork?.backdrop}
          class:loaded={backdropLoaded}
          src={work.artwork?.backdrop || work.posterUrl || ''}
          alt=""
          loading="eager"
          decoding="async"
          onload={() => { backdropLoaded = true; }}
        />
        <div class="backdrop-vignette"></div>
      </div>
    {/if}

    <div class="detail-main-layout">
      <!-- Poster Column -->
      <div class="detail-poster-column">
        <Poster {work} showMetadata={false} eager />
      </div>

      <!-- Information Column -->
      <div class="detail-info-column">
        <!-- Badges Row -->
        <div class="badge-row">
          <span class="badge kind-tag">SÉRIE</span>
          {#if work.synthetic}
            <span class="badge demo-tag">DÉMONSTRATION</span>
          {/if}
          {#if work.year}
            <span class="meta-tag">{work.year}</span>
          {/if}
          {#if work.rating}
            <span class="badge rating-tag">
              <Icon name="star" size={13} />
              {work.rating.toFixed(1)} / 10
            </span>
          {/if}
          {#if work.mpaa}
            <span class="badge mpaa-tag">{work.mpaa}</span>
          {/if}
          <span class="meta-tag seasons-count-tag">
            {work.seasonCount ?? 0} {(work.seasonCount ?? 0) > 1 ? 'saisons' : 'saison'} · {work.episodeCount ?? 0} {(work.episodeCount ?? 0) > 1 ? 'épisodes' : 'épisode'}
          </span>
        </div>

        {#if work.artwork?.logo}<img class="title-logo" src={work.artwork.logo} alt="" />{/if}
        <h1>{work.title}</h1>
        {#if !work.synthetic}<PersonalFeedback workId={work.id} />{/if}
        {#if work.seasons?.length}
          <nav class="season-shortcuts" aria-label="Saisons disponibles">
            {#each work.seasons as item (item.id)}
              <button class:chosen={activeSeason?.number === item.number} onclick={() => selectSeason(item.number)}>
                {#if work.artwork?.[`season${item.number}`]}<img src={work.artwork[`season${item.number}`] ?? ''} alt="" loading="lazy" decoding="async" width="80" height="120" />{:else}<span class="season-placeholder" aria-hidden="true">{item.number === 0 ? '★' : item.number}</span>{/if}
                {item.number === 0 ? 'Hors-série' : `Saison ${item.number}`} · {item.episodes.length} {item.episodes.length > 1 ? 'épisodes' : 'épisode'}
              </button>
            {/each}
          </nav>
        {/if}

        {#if work.originalTitle && work.originalTitle !== work.title}
          <p class="original-title">Titre original : <em>{work.originalTitle}</em></p>
        {/if}

        {#if work.genres?.length}
          <div class="genres-row">
            {#each work.genres as g (g)}
              <span class="genre-pill">{g}</span>
            {/each}
          </div>
        {/if}

        {#if work.outline && work.outline !== work.description && work.outline !== work.plot}
          <blockquote class="outline-hook">« {work.outline} »</blockquote>
        {/if}

        <p class="synopsis">{work.plot || work.description || 'Synopsis non renseigné.'}</p>

        <!-- Offline Node Notice -->
        {#if work.available === false}
          <div class="node-offline-box" role="status">
            <Icon name="alert" size={20} />
            <div>
              <strong>Média temporairement indisponible</strong>
              <p>Le Media Node propriétaire (<code>{work.nodeId ?? 'distant'}</code>) est actuellement hors ligne. L’œuvre reste indexée au catalogue et sera immédiatement lisible dès le retour du nœud.</p>
            </div>
          </div>
        {/if}

        {#if mediaError}
          <div class="error-banner" role="alert">
            <Icon name="alert" size={18} />
            <span>{mediaError}</span>
          </div>
        {/if}

        {#if work.artwork?.illustration || work.artwork?.landscape}
          <div class="artwork-gallery" aria-label="Illustrations locales">
            {#each ['illustration','landscape'] as role (role)}
              {#if work.artwork?.[role]}<img src={work.artwork[role] ?? ''} alt={role === 'illustration' ? `Illustration de ${work.title}` : `Visuel de ${work.title}`} loading="lazy" />{/if}
            {/each}
          </div>
        {/if}
        <!-- Provenance Footer -->
        <div class="metadata-footer">
          <span class="prov-tag">
            Provenance : {work.metadataProvenance === 'nfo' ? 'Fichier NFO local' : 'Indexation automatique'}
          </span>
          {#if work.addedAt}
            <span class="prov-tag">Ajouté le {new Date(work.addedAt * 1000).toLocaleDateString('fr-FR')}</span>
          {/if}
          {#if work.premiered}
            <span class="prov-tag">Date de première diffusion : {work.premiered}</span>
          {/if}
          <span class="prov-tag">Nœud source : <code>{work.nodeId ?? 'local'}</code></span>
        </div>

        {#if work.synthetic}
          <p class="detail-note">Fiche de démonstration synthétique pour la validation du lecteur.</p>
        {/if}
      </div>
    </div>
  </div>

  <!-- Seasons & Episodes Shelf -->
  <section class="shelf series-shelf" aria-labelledby="episodes-shelf-title">
    <div class="section-heading">
      <h2 id="episodes-shelf-title">Épisodes</h2>
      <label class="season-selector-label">
        <span>Sélectionner la saison :</span>
        <select aria-label="Saison" bind:value={season}>
          {#each work.seasons ?? [] as item (item.id)}
            <option value={item.number}>
              {item.title || (item.number === 0 ? 'Hors-série' : `Saison ${item.number}`)} ({item.episodes.length} épisodes)
            </option>
          {/each}
        </select>
      </label>
    </div>

    <!-- Episodes Grid / List -->
    <div class="episodes-list">
      {#each activeSeason?.episodes ?? [] as episode (episode.id)}
        <article class="episode">
          {#if episode.artwork?.thumb}<img class="episode-thumb" src={episode.artwork.thumb} alt="" loading="lazy" decoding="async" width="240" height="135" />{/if}
          <span class="episode-number">{String(episode.number).padStart(2, '0')}</span>
          <div class="episode-content">
            <div class="episode-title-row">
              <h3>{episode.title}</h3>
              {#if episode.available === false}
                <span class="badge warn-badge">Node indisponible</span>
              {/if}
            </div>
            <p class="episode-desc">{episode.description || 'Pas de synopsis pour cet épisode.'}</p>
            {#if !episode.synthetic}
              <WatchStatus workId={episode.id} />
            {/if}
            <EpisodePlayer
              {episode}
              {media}
              {immersive}
              isLaunching={launchingEpisodeId === episode.id}
              onlaunch={onLaunchEpisode}
              onselect={() => onSelectEpisode(episode)}
            />
          </div>
        </article>
      {:else}
        <div class="empty-episodes-box">
          <Icon name="tv" size={28} />
          <p>Aucun épisode disponible pour cette saison.</p>
        </div>
      {/each}
    </div>
  </section>
  <EditorialCredits {work} />
</div>

<style>
  .artwork-gallery { display:flex;flex-wrap:wrap;gap:12px;margin-block:16px; }
  .artwork-gallery img { max-width:100%;width:260px;max-height:160px;object-fit:contain;border-radius:6px; }

  .title-logo { max-width:min(340px,100%); max-height:110px; object-fit:contain; object-position:left; }

  .season-placeholder { display:grid;place-items:center;width:80px;aspect-ratio:2/3;background:var(--bg-elevated);border-radius:4px;font-size:32px;color:var(--text-secondary); }
  .season-shortcuts img { display:block; width:80px; aspect-ratio:2/3; object-fit:cover; margin-bottom:8px; border-radius:4px; }
  .episode-thumb { height:auto; width:240px; max-width:25vw; aspect-ratio:16/9; object-fit:cover; border-radius:4px; }
  .season-shortcuts { display:flex; gap:8px; overflow-x:auto; max-width:100%; padding:2px; }
  .season-shortcuts button { flex:0 0 150px;display:flex;flex-direction:column;align-items:flex-start;gap:8px;text-align:left;padding:10px 14px; border:1px solid var(--border-subtle); background:var(--bg-surface); color:var(--text-primary); border-radius:6px; }
  .season-shortcuts .chosen { border-color:var(--accent-primary); color:var(--accent-primary); }
  #episodes-shelf-title { scroll-margin-top:110px; }

  .series-detail-view {
    position: relative;
    padding-bottom: 40px;
  }

  .detail-hero {
    position: relative;
    padding-top: 20px;
  }

  .backdrop-art-container {
    position: absolute;
    top: 0;
    left: calc(-1 * var(--page-gutter));
    right: calc(-1 * var(--page-gutter));
    height: 520px;
    overflow: hidden;
    z-index: 0;
    pointer-events: none;
  }

  .backdrop-art-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center 25%;
    filter: blur(28px) brightness(0.75) saturate(1.3);
    transform: scale(1.15);
    opacity: 0;
    transition: opacity 0.4s ease;
  }

  .backdrop-art-image.artwork { filter: brightness(0.8); transform:none; }

  .backdrop-art-image.loaded {
    opacity: 1;
  }

  .backdrop-vignette {
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, color-mix(in srgb, var(--bg-main) 40%, transparent) 0%, color-mix(in srgb, var(--bg-main) 85%, transparent) 60%, var(--color-canvas) 100%);
  }

  :global([data-theme="light"]) .backdrop-vignette {
    background: linear-gradient(180deg, color-mix(in srgb, var(--bg-main) 80%, transparent), var(--bg-main));
  }

  .detail-main-layout {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: clamp(240px, 24vw, 340px) 1fr;
    gap: clamp(28px, 4vw, 56px);
    align-items: flex-start;
  }

  .detail-poster-column {
    position: sticky;
    top: 96px;
  }

  .detail-info-column {
    min-width: 0;
    overflow-wrap: anywhere;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .badge-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }

  .badge {
    font-size: 10.5px;
    font-weight: 700;
    padding: 3px 8px;
    border-radius: var(--radius-xs);
    background: rgba(255, 255, 255, 0.08);
    color: var(--color-text);
  }

  .kind-tag {
    background: var(--color-accent-soft);
    color: var(--accent-primary);
    border: 1px solid rgba(99, 102, 241, 0.3);
  }

  .demo-tag {
    background: rgba(251, 191, 36, 0.15);
    color: var(--accent-secondary);
  }

  .warn-badge {
    background: rgba(248, 113, 113, 0.15);
    color: var(--color-danger);
    font-size: 10px;
  }

  .rating-tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: rgba(251, 191, 36, 0.15);
    color: var(--accent-secondary);
    border: 1px solid rgba(251, 191, 36, 0.25);
  }

  .mpaa-tag {
    border: 1px solid rgba(255, 255, 255, 0.2);
  }

  .seasons-count-tag {
    color: var(--color-accent-hover);
    font-weight: 600;
  }

  .meta-tag {
    font-size: 12px;
    font-weight: 600;
    color: var(--color-muted);
  }

  h1 {
    font-family: var(--font-display);
    font-size: clamp(32px, 4.2vw, 52px);
    font-weight: 750;
    line-height: 1.15;
    letter-spacing: -0.04em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .original-title {
    margin: 0;
    color: var(--color-muted);
    font-size: 14px;
  }

  .genres-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .genre-pill {
    font-size: 11.5px;
    padding: 4px 10px;
    border-radius: var(--radius-pill);
    background: rgba(255, 255, 255, 0.06);
    color: var(--color-text);
    border: 1px solid var(--color-line);
  }

  .outline-hook {
    margin: 4px 0;
    padding-left: 14px;
    border-left: 3px solid var(--color-accent);
    font-style: italic;
    font-size: 15px;
    color: var(--color-accent-hover);
  }

  .synopsis {
    font-size: 15px;
    line-height: 1.65;
    color: var(--text-secondary);
    margin: 4px 0 12px;
  }

  .node-offline-box {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    padding: 16px 20px;
    border-radius: var(--radius-md);
    background: rgba(248, 113, 113, 0.1);
    border: 1px solid rgba(248, 113, 113, 0.25);
    color: var(--color-danger);
  }

  .node-offline-box strong {
    display: block;
    margin-bottom: 4px;
    font-size: 14px;
  }

  .node-offline-box p {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--status-error);
  }

  /* Metadata Footer */
  .metadata-footer {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 16px;
    padding-top: 8px;
    border-top: 1px solid var(--color-line);
  }

  .prov-tag {
    font-size: 11.5px;
    color: var(--color-subtle);
  }

  .prov-tag code {
    color: var(--color-muted);
  }

  .detail-note {
    font-size: 12px;
    color: var(--color-muted);
    font-style: italic;
    margin: 0;
  }

  /* Episodes Section */
  .series-shelf {
    margin-top: 48px;
    padding-top: 36px;
    border-top: 1px solid var(--color-line);
  }

  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
    flex-wrap: wrap;
    gap: 16px;
  }

  h2 {
    font-size: clamp(22px, 2.5vw, 28px);
    font-weight: 700;
    letter-spacing: -0.03em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .season-selector-label {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 13px;
    font-weight: 500;
    color: var(--color-muted);
  }

  .season-selector-label select {
    min-width: 200px;
    background-color: var(--color-surface);
  }

  .episodes-list {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .episode {
    display: flex;
    gap: 20px;
    padding: 22px;
    border-radius: var(--radius-md);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    transition: all var(--transition-fast);
  }

  .episode:hover {
    background: var(--color-raised);
    border-color: var(--border-subtle);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
  }

  .episode-number {
    font-family: var(--font-display);
    font-size: 26px;
    font-weight: 750;
    color: var(--color-subtle);
    min-width: 38px;
    padding-top: 2px;
  }

  .episode-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .episode-title-row {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }

  h3 {
    font-size: 16px;
    font-weight: 650;
    color: var(--color-text-bright);
    margin: 0;
  }

  .episode-desc {
    margin: 0;
    font-size: 13.5px;
    line-height: 1.55;
    color: var(--color-muted);
  }

  .empty-episodes-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 48px 24px;
    border-radius: var(--radius-md);
    background: rgba(255, 255, 255, 0.02);
    border: 1px dashed var(--color-line);
    color: var(--color-subtle);
    text-align: center;
  }

  .error-banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-radius: var(--radius-sm);
    background: rgba(248, 113, 113, 0.12);
    border: 1px solid rgba(248, 113, 113, 0.25);
    color: var(--color-danger);
    font-size: 13px;
  }

  .empty-episodes-box p {
    margin: 0;
    font-size: 14px;
    color: var(--color-muted);
  }

  @media (max-width: 820px) {
    .detail-main-layout {
      grid-template-columns: 1fr;
    }

    .detail-poster-column {
      position: static;
      max-width: 260px;
      margin: 0 auto;
    }

    .episode-thumb { width:100%;max-width:100%; }
    .episode {
      flex-wrap:wrap;
      padding: 16px;
      gap: 14px;
    }

    .episode-number {
      font-size: 20px;
      min-width: 28px;
    }
  }
</style>
