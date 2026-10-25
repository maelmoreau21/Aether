<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import { history, loadHistory } from './watch-state';
  import Icon from '../ui/Icon.svelte';
  import type { Work } from './api';
  import { imagePreviewUrl } from './image-url';
  let { works = [] }: { works?: Work[] } = $props();
  function thumbnail(id: string, seriesId?: string) { const work=works.find(work=>work.id===(seriesId || id)); return imagePreviewUrl(work?.artwork?.backdrop || work?.posterUrl,640); }

  let error = $state('');
  let track = $state<HTMLDivElement>();
  let canScrollLeft = $state(false);
  let canScrollRight = $state(false);

  onMount(() => {
    void loadHistory().catch(e => (error = e.message));
  });

  $effect(() => {
    if ($session) void loadHistory().catch(e => (error = e.message));
  });

  const continuing = $derived($history.filter(item => !item.watched && item.position > 0));

  function updateScrollState() {
    if (!track) return;
    canScrollLeft = track.scrollLeft > 10;
    canScrollRight = track.scrollLeft + track.clientWidth < track.scrollWidth - 10;
  }

  function scroll(direction: 'left' | 'right') {
    if (!track) return;
    const distance = track.clientWidth * 0.75;
    track.scrollBy({
      left: direction === 'left' ? -distance : distance,
      behavior: 'smooth'
    });
  }
</script>

{#if error || continuing.length}
<section class="continue-shelf" aria-label="Continuer à regarder">
  <div class="shelf-header">
    <div class="title-group">
      <h2>Continuer à regarder</h2>
      {#if continuing.length > 0}
        <span class="count-badge">{continuing.length}</span>
      {/if}
    </div>

    {#if continuing.length > 2}
      <div class="carousel-nav" aria-hidden="true">
        <button
          type="button"
          class="nav-arrow"
          disabled={!canScrollLeft}
          onclick={() => scroll('left')}
          aria-label="Faire défiler vers la gauche"
          tabindex="-1"
        >
          <Icon name="chevron-left" size={18} />
        </button>
        <button
          type="button"
          class="nav-arrow"
          disabled={!canScrollRight}
          onclick={() => scroll('right')}
          aria-label="Faire défiler vers la droite"
          tabindex="-1"
        >
          <Icon name="chevron-right" size={18} />
        </button>
      </div>
    {/if}
  </div>

  {#if error}
    <div class="error-box" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button
        class="retry-btn"
        onclick={() => {
          error = '';
          void loadHistory().catch(e => (error = e.message));
        }}
      >
        Réessayer
      </button>
    </div>
  {:else if !continuing.length}
    <div class="empty-state">
      <div class="empty-icon" aria-hidden="true">
        <Icon name="clock" size={28} />
      </div>
      <div class="empty-copy">
        <strong>Aucune séance en cours</strong>
        <p>Vos séances en cours apparaîtront ici.</p>
      </div>
    </div>
  {:else}
    <div
      class="continue-list"
      bind:this={track}
      onscroll={updateScrollState}
    >
      {#each continuing as item (item.workId)}
        <a
          class="continue-card"
          href={resolve(`${item.kind === 'episode' ? 'series/' : 'films/'}?work=${encodeURIComponent(item.seriesId ?? item.workId)}`)}
          aria-label={`Reprendre la lecture de ${item.title}`}
        >
          <div class="card-thumb">
            {#if thumbnail(item.workId,item.seriesId)}<img class="resume-image" src={thumbnail(item.workId,item.seriesId)} alt="" loading="lazy" />{/if}
            <div class="thumb-bg" aria-hidden="true">
              <span class="thumb-letter">{(item.title ?? 'M').charAt(0)}</span>
            </div>
            <div class="thumb-overlay" aria-hidden="true">
              <div class="play-btn-circle">
                <Icon name="play" size={18} />
              </div>
            </div>
            <div class="card-progress-track">
              <progress
                value={item.position}
                max={item.duration}
                aria-label={`Progression de ${item.title}`}
              ></progress>
            </div>
          </div>

          <div class="card-info">
            <div class="card-header-row">
              <span class="card-kind-badge">
                {item.kind === 'episode' ? 'ÉPISODE' : 'FILM'}
              </span>
              <span class="card-time-remaining">
                {Math.max(1, Math.round((item.duration - item.position) / 60))} min restantes
              </span>
            </div>
            <strong>{item.title}</strong>
            <span class="card-position-text">
              {Math.floor(item.position / 60)} / {Math.ceil(item.duration / 60)} min
            </span>
          </div>
        </a>
      {/each}
    </div>
  {/if}
</section>
{/if}

<style>
  .resume-image { position:absolute; inset:0; width:100%;height:100%;object-fit:cover;z-index:1; }
  .card-thumb { position:relative; }
  .thumb-overlay,.card-progress-track { z-index:2; }
  .continue-shelf {
    margin: 36px 0;
  }

  .shelf-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
    gap: 16px;
  }

  .title-group {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  h2 {
    font-size: clamp(20px, 2.2vw, 24px);
    font-weight: 650;
    letter-spacing: -0.03em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .count-badge {
    font-size: 11px;
    font-weight: 700;
    padding: 2px 8px;
    border-radius: var(--radius-full);
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border: 1px solid var(--color-accent-soft);
  }

  .carousel-nav {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .nav-arrow {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid var(--color-line);
    color: var(--color-text);
    transition: all var(--transition-fast);
  }

  .nav-arrow:hover:not(:disabled) {
    background: var(--color-accent-soft);
    border-color: var(--color-accent-soft);
    color: var(--color-accent-hover);
    transform: scale(1.08);
  }

  .nav-arrow:disabled {
    opacity: 0.25;
    cursor: default;
  }

  .error-box {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 18px;
    border-radius: var(--radius-sm);
    background: rgba(248, 113, 113, 0.12);
    border: 1px solid rgba(248, 113, 113, 0.25);
    color: var(--color-danger);
    font-size: 13px;
  }

  .retry-btn {
    margin-left: auto;
    padding: 6px 14px;
    border: 1px solid currentColor;
    border-radius: var(--radius-xs);
    background: transparent;
    color: inherit;
    font-size: 12px;
  }

  .empty-state {
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 24px 28px;
    border-radius: var(--radius-md);
    background: rgba(255, 255, 255, 0.02);
    border: 1px dashed var(--color-line);
  }

  .empty-icon {
    display: grid;
    place-items: center;
    width: 50px;
    height: 50px;
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-subtle);
  }

  .empty-copy strong {
    display: block;
    font-size: 14px;
    font-weight: 600;
    color: var(--color-text);
    margin-bottom: 4px;
  }

  .empty-copy p {
    margin: 0;
    color: var(--color-muted);
    font-size: 12.5px;
  }

  .continue-list {
    display: flex;
    gap: 18px;
    overflow-x: auto;
    scroll-behavior: smooth;
    padding: 6px 4px 14px;
    scrollbar-width: none;
    -ms-overflow-style: none;
  }

  .continue-list::-webkit-scrollbar {
    display: none;
  }

  .continue-card {
    flex: 0 0 clamp(240px, 24vw, 300px);
    display: flex;
    flex-direction: column;
    text-decoration: none;
    color: inherit;
    border-radius: var(--radius-md);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    overflow: hidden;
    transition: transform var(--transition-normal), border-color var(--transition-normal), box-shadow var(--transition-normal);
    box-shadow: var(--shadow-card);
  }

  .continue-card:hover {
    transform: translateY(-4px);
    border-color: var(--color-accent-soft);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.65), 0 0 16px var(--color-accent-soft);
  }

  .card-thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    background: linear-gradient(135deg, #1b1929 0%, #10111a 100%);
    overflow: hidden;
  }

  .thumb-bg {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    background: radial-gradient(circle at center, #26213b 0%, #10111a 100%);
  }

  .thumb-letter {
    font-family: var(--font-display);
    font-size: 42px;
    font-weight: 700;
    color: var(--color-accent-soft);
    user-select: none;
  }

  .thumb-overlay {
    position: absolute;
    inset: 0;
    background: rgba(9, 10, 16, 0.4);
    display: grid;
    place-items: center;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }

  .continue-card:hover .thumb-overlay {
    opacity: 1;
  }

  .play-btn-circle {
    width: 44px;
    height: 44px;
    border-radius: 50%;
    background: var(--color-text-bright);
    color: #0b0c13;
    display: grid;
    place-items: center;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.6);
    transform: scale(0.9);
    transition: transform var(--transition-fast);
  }

  .continue-card:hover .play-btn-circle {
    transform: scale(1);
  }

  .card-progress-track {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    height: 4px;
    background: rgba(255, 255, 255, 0.2);
  }

  progress {
    display: block;
    width: 100%;
    height: 100%;
    appearance: none;
    border: none;
  }

  progress::-webkit-progress-bar {
    background: transparent;
  }

  progress::-webkit-progress-value {
    background: var(--gradient-accent);
    box-shadow: 0 0 8px var(--color-accent-glow);
  }

  progress::-moz-progress-bar {
    background: var(--color-accent-solid);
  }

  .card-info {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .card-header-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .card-kind-badge {
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--color-accent);
  }

  .card-time-remaining {
    font-size: 11px;
    color: var(--color-muted);
  }

  strong {
    font-size: 14px;
    font-weight: 600;
    line-height: 1.35;
    color: var(--color-text-bright);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-position-text {
    font-size: 11.5px;
    color: var(--color-subtle);
  }

</style>
