<script lang="ts">
  import { page } from '$app/state';
  import { onDestroy, untrack } from 'svelte';
  import { accessibleLibraries } from '../../lib/catalog/libraries';
  import Navbar from '../../lib/ui/Navbar.svelte';
  import { DEFAULT_NAME } from '../../lib/config/brand';
  import { playMusic, enqueueMusic } from '../../lib/music/queue';
  import { musicQueue } from '../../lib/music/queue';
  import AddToPlaylist from '../../lib/music/AddToPlaylist.svelte';

  interface Track {
    id: string;
    title: string;
    artist: string;
    album: string;
    albumArtist: string;
    mime: string;
    codec: string;
    duration: number | null;
    trackNumber?: number | null;
    coverUrl?: string | null;
  }

  let tracks = $state<Track[]>([]);
  let query = $state('');
  let error = $state('');
  let busy = $state(false);
  let offset = $state(0);
  let total = $state(0);
  let view = $state<'tracks' | 'albums' | 'artists'>('tracks');
  let selectedGroup = $state('');

  const groups = $derived.by(() => {
    const map: Record<string, Track[]> = Object.create(null);
    for (const track of tracks) {
      const key = view === 'artists'
        ? track.albumArtist || track.artist
        : track.album
          ? `${track.albumArtist || track.artist} · ${track.album}`
          : '';
      if (key) map[key] = [...(map[key] || []), track];
    }
    return Object.entries(map);
  });

  const visibleTracks = $derived(
    view === 'tracks' || !selectedGroup
      ? tracks
      : groups.find(([name]) => name === selectedGroup)?.[1] || []
  );

  function groupCover(items: Track[]): string | null {
    return items.find(t => t.coverUrl)?.coverUrl ?? null;
  }

  function time(value: number | null) {
    return value === null
      ? '—'
      : `${Math.floor(value / 60)}:${String(Math.floor(value % 60)).padStart(2, '0')}`;
  }

  let pending: AbortController | undefined;
  async function load(next = 0) {
    pending?.abort();
    const controller = new AbortController();
    pending = controller;
    const timer = setTimeout(() => controller.abort(), 8000);
    busy = true;
    error = '';
    try {
      const response = await fetch(
        '/api/v1/music?' +
          new URLSearchParams({
            q: query,
            offset: String(next),
            limit: '100',
            library: page.url.searchParams.get('library') ?? ''
          }),
        { signal: controller.signal }
      );
      if (!response.ok) throw new Error('Impossible de charger votre musique.');
      const data = await response.json();
      if (pending !== controller) return;
      tracks = data.items;
      total = data.total;
      offset = next;
    } catch (cause) {
      if (pending === controller) {
        error = controller.signal.aborted
          ? 'Le chargement prend trop de temps. Réessayez.'
          : cause instanceof Error
            ? cause.message
            : 'Chargement impossible.';
      }
    } finally {
      clearTimeout(timer);
      if (pending === controller) busy = false;
    }
  }

  onDestroy(() => {
    pending?.abort();
    pending = undefined;
  });

  function play(track: Track) {
    playMusic(visibleTracks, visibleTracks.findIndex(item => item.id === track.id));
    error = '';
  }

  $effect(() => {
    const id = page.url.searchParams.get('library');
    void id;
    untrack(() => void load());
  });
</script>

<svelte:head><title>Musique · {DEFAULT_NAME}</title></svelte:head>
<Navbar section="music" />

<main>
  <h1>{$accessibleLibraries.find(item => item.id === page.url.searchParams.get('library'))?.name ?? 'Musique'}</h1>
  <form onsubmit={event => { event.preventDefault(); void load(); }}>
    <label>
      Rechercher un titre, artiste ou album
      <input type="search" bind:value={query} maxlength="256" />
    </label>
    <button disabled={busy}>Rechercher</button>
  </form>

  {#if error}<p role="alert">{error}</p>{/if}
  {#if busy}<p role="status">Chargement…</p>{/if}

  <div class="music-heading">
    <p>{total} piste(s)</p>
    <button disabled={!tracks.length} onclick={() => playMusic(visibleTracks)}>Lire cette sélection</button>
  </div>

  <nav class="music-tabs" aria-label="Présentation de la musique">
    {#each [['tracks', 'Morceaux'], ['albums', 'Albums'], ['artists', 'Artistes']] as option (option[0])}
      <button class:active={view === option[0]} onclick={() => { view = option[0] as typeof view; selectedGroup = ''; }}>
        {option[1]}
      </button>
    {/each}
  </nav>

  {#if view !== 'tracks'}
    <div class="music-covers">
      {#each groups as [name, items] (name)}
        {@const cover = groupCover(items)}
        <button class:selected={selectedGroup === name} onclick={() => selectedGroup = selectedGroup === name ? '' : name}>
          {#if cover}
            <img class="cover" class:artist={view === 'artists'} src={cover} alt="" loading="lazy" />
          {:else}
            <span class="cover" class:artist={view === 'artists'} aria-hidden="true">{name.slice(0, 1)}</span>
          {/if}
          <strong>{name}</strong>
          <small>{items.length} titre{items.length > 1 ? 's' : ''}</small>
        </button>
      {/each}
    </div>
  {/if}

  <div class="tracks">
    {#each visibleTracks as track, index (track.id)}
      <article class:playing={$musicQueue.tracks[$musicQueue.index]?.id === track.id}>
        {#if track.coverUrl}
          <img class="track-thumb" src={track.coverUrl} alt="" loading="lazy" />
        {:else}
          <span class="track-thumb-placeholder" aria-hidden="true">♫</span>
        {/if}
        <span class="track-number">{track.trackNumber ?? index + 1}</span>
        <div class="track-title">
          <strong>{track.title}</strong>
          <p>{track.artist} · {track.album || 'Album inconnu'} · {track.codec}</p>
        </div>
        <span class="duration">{time(track.duration)}</span>
        <button onclick={() => play(track)} aria-label={`Lire ${track.title}`}>Lire</button>
        <button onclick={() => enqueueMusic(track)} aria-label={`Ajouter ${track.title} à la file`}>À suivre</button>
        <AddToPlaylist id={track.id} />
      </article>
    {:else}
      {#if !busy}
        <p>Aucune piste musicale. Ajoutez une bibliothèque Musique dans l’administration puis lancez son scan.</p>
      {/if}
    {/each}
  </div>

  <nav aria-label="Pagination musique">
    <button disabled={busy || offset === 0} onclick={() => load(Math.max(0, offset - 100))}>Précédent</button>
    <button disabled={busy || offset + tracks.length >= total} onclick={() => load(offset + 100)}>Suivant</button>
  </nav>
</main>

<style>
  .music-tabs { display: flex; gap: 8px; }
  .music-tabs button { min-height: 44px; }
  .music-tabs .active { background: var(--color-accent-soft); color: var(--color-accent); border-color: transparent; }
  .music-covers { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 16px; margin: 24px 0; }
  .music-covers button { border: 0; background: transparent; text-align: left; padding: 0; min-width: 0; cursor: pointer; }
  .music-covers strong, .music-covers small { display: block; margin-top: 8px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .music-covers .selected { color: var(--color-accent); }
  .cover { display: grid; place-items: center; aspect-ratio: 1; width: 100%; object-fit: cover; background: var(--color-raised); border-radius: var(--radius-md); font-size: 48px; color: var(--color-muted); }
  .cover.artist { border-radius: 50%; }
  .track-thumb { width: 44px; height: 44px; object-fit: cover; border-radius: var(--radius-sm); flex-shrink: 0; }
  .track-thumb-placeholder { display: grid; place-items: center; width: 44px; height: 44px; background: var(--color-raised); color: var(--color-muted); border-radius: var(--radius-sm); flex-shrink: 0; font-size: 18px; }
  .track-title { flex: 1; min-width: 160px; }
  .track-number, .duration { font-size: 13px; color: var(--color-muted); font-variant-numeric: tabular-nums; }
  .playing { background: var(--color-accent-soft); }
  button { min-height: 44px; cursor: pointer; }
  main { max-width: 1440px; margin: auto; padding: 32px var(--page-gutter); }
  form, label { display: flex; gap: 12px; flex-wrap: wrap; align-items: center; }
  label { flex: 1; }
  input { flex: 1; min-width: 160px; }
  button, input { padding: 10px 14px; background: var(--color-surface); color: var(--color-text); border: 1px solid var(--color-line); border-radius: var(--radius-md); }
  .music-heading { display: flex; justify-content: space-between; align-items: center; }
  article { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 16px; align-items: center; padding: 14px 0; border-bottom: 1px solid var(--color-line); }
  p { color: var(--color-muted); }
  nav { display: flex; gap: 12px; margin: 20px 0; }
</style>
