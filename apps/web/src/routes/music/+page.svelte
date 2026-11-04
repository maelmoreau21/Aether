<script lang="ts">
  import { page } from '$app/state';
  import { onDestroy, untrack, tick } from 'svelte';
  import { accessibleLibraries } from '../../lib/catalog/libraries';
  import Navbar from '../../lib/ui/Navbar.svelte';
  import { DEFAULT_NAME } from '../../lib/config/brand';
  import { playMusic, enqueueMusic } from '../../lib/music/queue';
  import { musicQueue } from '../../lib/music/queue';
  import AddToPlaylist from '../../lib/music/AddToPlaylist.svelte';
  import MusicArtwork from '../../lib/music/MusicArtwork.svelte';
  import Icon from '../../lib/ui/Icon.svelte';

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
    const map: Record<string, { id: string; title: string; artist: string; items: Track[] }> = Object.create(null);
    for (const track of tracks) {
      const artist = track.albumArtist || track.artist;
      const title = view === 'artists' ? artist : track.album;
      if (!title) continue;
      const id = JSON.stringify(view === 'artists' ? [artist] : [artist, track.album]);
      const group = map[id] ?? { id, title, artist, items: [] };
      group.items.push(track); map[id] = group;
    }
    return Object.values(map);
  });
  const group = $derived(groups.find(item => item.id === selectedGroup));
  const visibleTracks = $derived(
    view === 'tracks' || !selectedGroup
      ? tracks
      : group?.items || []
  );
  let groupHeading = $state<HTMLHeadingElement>();
  let covers = $state<HTMLDivElement>();
  async function selectGroup(id: string) { selectedGroup = id; await tick(); groupHeading?.focus(); }
  async function leaveGroup() { const id = selectedGroup; selectedGroup = ''; await tick(); [...(covers?.querySelectorAll<HTMLButtonElement>('button') ?? [])].find(button => button.dataset.group === id)?.focus(); }

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
      selectedGroup = '';
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
  <header class="page-heading"><div><p class="eyebrow">Votre médiathèque</p><h1>{$accessibleLibraries.find(item => item.id === page.url.searchParams.get('library'))?.name ?? 'Musique'}</h1></div></header>
  <form class="search" onsubmit={event => { event.preventDefault(); void load(); }}><label for="music-search">Rechercher un titre, artiste ou album</label><div><input id="music-search" type="search" bind:value={query} maxlength="256" placeholder="Titre, artiste, album…" /><button disabled={busy} aria-label="Rechercher"><Icon name="search" size={18} /><span>Rechercher</span></button></div></form>
  {#if error}<div class="error" role="alert"><p>{error}</p><button disabled={busy} onclick={() => load(offset)}>Réessayer</button></div>{/if}
  <nav class="music-tabs" aria-label="Présentation de la musique">
    {#each [['tracks', 'Morceaux'], ['albums', 'Albums'], ['artists', 'Artistes']] as option (option[0])}<button aria-pressed={view === option[0]} onclick={() => { view = option[0] as typeof view; selectedGroup = ''; }}>{option[1]}</button>{/each}
  </nav>
  {#if busy}<div class="loading" role="status"><span>Chargement de la musique…</span><div class="skeletons" aria-hidden="true">{#each [1,2,3,4] as item (item)}<span></span>{/each}</div></div>{/if}
  <div aria-busy={busy}>
    {#if !busy || tracks.length}
      {#if view !== 'tracks' && !group}
        <div class="section-heading"><h2>{view === 'albums' ? 'Albums' : 'Artistes'}</h2><p>{groups.length} sur cette page</p></div>
        <p class="page-note">Regroupements des {tracks.length} morceau{tracks.length > 1 ? 'x' : ''} chargé{tracks.length > 1 ? 's' : ''}{total > tracks.length ? ` sur ${total}` : ''}. Les autres pages peuvent compléter un album ou un artiste.</p>
        <div class="music-covers" bind:this={covers}>
          {#each groups as item (item.id)}<button data-group={item.id} onclick={() => selectGroup(item.id)} aria-label={`Ouvrir ${view === 'albums' ? 'l’album' : 'l’artiste'} ${item.title}`}><span class="cover"><MusicArtwork src={view === 'albums' ? groupCover(item.items) : null} label={item.title} circular={view === 'artists'} /></span><strong>{item.title}</strong>{#if view === 'albums'}<span class="artist-name">{item.artist || 'Artiste inconnu'}</span>{/if}<small>{item.items.length} morceau{item.items.length > 1 ? 'x' : ''} sur cette page</small></button>{/each}
        </div>
        {#if !groups.length && tracks.length}<p class="empty">Aucun {view === 'albums' ? 'album' : 'artiste'} renseigné sur cette page. Retrouvez ces titres dans Morceaux.</p>{/if}
      {:else}
        {#if group}<button class="back" onclick={leaveGroup}><Icon name="back" size={18} />{view === 'albums' ? 'Tous les albums' : 'Tous les artistes'}</button><div class="group-heading"><div class="group-cover"><MusicArtwork src={view === 'albums' ? groupCover(group.items) : null} label={group.title} circular={view === 'artists'} /></div><div><p class="eyebrow">{view === 'albums' ? 'Album' : 'Artiste'}</p><h2 tabindex="-1" bind:this={groupHeading}>{group.title}</h2>{#if view === 'albums'}<p>{group.artist}</p>{/if}<p class="page-note">{group.items.length} morceau{group.items.length > 1 ? 'x' : ''} de cette page</p><button class="primary" aria-label="Lire cette sélection" disabled={!visibleTracks.length || busy} onclick={() => playMusic(visibleTracks)}><Icon name="play" size={18} />Lire</button></div></div>{/if}
        {#if !group}<div class="section-heading"><div><h2>Morceaux</h2><p>{total} morceau{total > 1 ? 'x' : ''}{total > tracks.length ? ` · ${tracks.length} sur cette page` : ''}</p></div><button class="primary" disabled={!visibleTracks.length || busy} onclick={() => playMusic(visibleTracks)}><Icon name="play" size={18} />Lire cette sélection</button></div>{/if}
        <div class="tracks">
          {#each visibleTracks as track, index (track.id)}
            <article class:playing={$musicQueue.tracks[$musicQueue.index]?.id === track.id} aria-label={track.title}>
              <span class="track-number">{track.trackNumber ?? index + 1}</span><span class="track-art"><MusicArtwork src={track.coverUrl} label={track.album || track.title} /></span>
              <div class="track-title"><strong>{track.title}</strong><p>{track.artist || 'Artiste inconnu'}{#if !group && track.album}<span> · {track.album}</span>{/if}</p></div>
              <span class="duration">{time(track.duration)}</span><button class="track-play" onclick={() => play(track)} aria-label={`Lire ${track.title}`}><Icon name="play" size={20} /></button>
              <details class="track-actions"><summary aria-label={`Options pour ${track.title}`}><Icon name="more" size={20} /></summary><div class="track-menu"><button onclick={() => enqueueMusic(track)} aria-label={`Ajouter ${track.title} à la file`}>Ajouter à la file</button><AddToPlaylist id={track.id} inline /><p>{time(track.duration)} · {track.codec}</p></div></details>
            </article>
          {/each}
        </div>
      {/if}
      {#if !tracks.length && !busy && !error}<div class="empty"><h2>{query ? 'Aucun résultat' : 'Votre musique apparaîtra ici'}</h2><p>{query ? 'Essayez un autre titre, artiste ou album.' : 'Ajoutez une bibliothèque Musique dans l’administration, puis lancez son indexation.'}</p>{#if query}<button onclick={() => { query = ''; void load(); }}>Effacer la recherche</button>{/if}</div>{/if}
    {/if}
  </div>
  <nav class="pagination" aria-label="Pagination musique"><button disabled={busy || offset === 0} onclick={() => load(Math.max(0, offset - 100))}>Précédent</button><span>Page {Math.floor(offset / 100) + 1}</span><button disabled={busy || offset + tracks.length >= total} onclick={() => load(offset + 100)}>Suivant</button></nav>
</main>
<style>
  main { width: 100%; max-width: var(--maxw); margin: 0 auto; padding: 20px var(--gutter) 80px; min-width: 0; }
  .page-heading { margin-bottom: 20px; }
  h1 { margin: 4px 0 0; font-size: clamp(32px, 4vw, 44px); font-weight: 800; letter-spacing: -0.03em; line-height: 1.02; overflow-wrap: anywhere; }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; overflow-wrap: anywhere; }
  .eyebrow { margin: 0; font-size: 12px; font-weight: 700; letter-spacing: 0.07em; text-transform: uppercase; color: var(--mu); }
  p { color: var(--mu); line-height: 1.6; }
  button, input { min-height: 44px; font: inherit; padding: 0 16px; background: var(--pn); color: var(--tx); border: 1px solid transparent; border-radius: var(--r-sm); }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; cursor: pointer; font-weight: 700; transition: background-color var(--t-fast) ease; }
  button:hover:not(:disabled) { background: var(--pn-2); }
  button:disabled { cursor: default; opacity: 0.45; }
  .search { max-width: 640px; margin-bottom: 20px; }
  .search label { display: block; margin-bottom: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  .search > div { display: flex; gap: 8px; }
  input { flex: 1; min-width: 0; width: 100%; font-size: 15px; font-weight: 500; }
  input:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  .music-tabs { display: inline-flex; gap: 4px; padding: 4px; border-radius: var(--r-pill); background: var(--pn); margin-bottom: 24px; }
  .music-tabs button { min-height: 38px; padding: 0 18px; border: 0; border-radius: var(--r-pill); background: none; color: var(--mu); font-size: 14px; }
  .music-tabs button:hover:not(:disabled) { background: none; color: var(--tx); }
  .music-tabs [aria-pressed=true], .music-tabs [aria-pressed=true]:hover:not(:disabled) { background: var(--tx); color: var(--bg); }
  .section-heading { display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 16px; margin: 8px 0 16px; }
  .section-heading p { font-size: 14px; margin: 4px 0 0; }
  .primary, .primary:hover:not(:disabled) { border: 0; background: var(--btn); color: var(--btn-tx); }
  .primary:hover:not(:disabled) { opacity: 0.9; }
  .page-note { font-size: 13px; margin: 4px 0 20px; max-width: 75ch; }
  .music-covers { display: grid; grid-template-columns: repeat(auto-fill, minmax(var(--poster-base, 170px), 1fr)); gap: 28px 16px; margin: 16px 0; }
  .music-covers > button, .music-covers > button:hover:not(:disabled) { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; min-height: 0; border: 0; background: transparent; text-align: left; padding: 0; min-width: 0; font-weight: 500; }
  .cover { display: block; width: 100%; aspect-ratio: 1; margin-bottom: 10px; border-radius: var(--r-md); overflow: hidden; transition: transform var(--t-slow) var(--ease), box-shadow var(--t-slow) ease; }
  .music-covers > button:hover .cover { transform: translateY(-4px); box-shadow: var(--shadow); }
  .music-covers strong, .artist-name { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; line-height: 1.35; }
  .music-covers strong { font-size: 14px; font-weight: 700; color: var(--tx); }
  .artist-name, small { font-size: 13px; color: var(--mu); }
  small { display: block; margin-top: 2px; }
  .back, .back:hover:not(:disabled) { padding-left: 4px; background: none; border: 0; color: var(--mu); margin-bottom: 16px; }
  .back:hover:not(:disabled) { color: var(--tx); }
  .group-heading { display: flex; gap: 32px; align-items: flex-end; margin-bottom: 28px; }
  .group-heading > div:last-child { min-width: 0; display: flex; flex-direction: column; align-items: flex-start; gap: 6px; }
  .group-heading h2 { font-size: clamp(28px, 4vw, 44px); }
  .group-heading p { margin: 0; }
  .group-cover { width: 220px; height: 220px; flex: none; border-radius: var(--r-md); overflow: hidden; box-shadow: var(--shadow); }
  .tracks { border-radius: var(--r-lg); background: var(--bg-2); border: 1px solid var(--line); overflow: hidden; }
  article { display: grid; grid-template-columns: 28px 44px minmax(0, 1fr) 52px 44px 44px; gap: 16px; align-items: center; padding: 8px 16px; border-top: 1px solid var(--line); min-width: 0; transition: background-color var(--t-fast) ease; }
  article:first-child { border-top: 0; }
  article:hover { background: var(--pn); }
  .playing, .playing:hover { background: var(--ac-soft); }
  .playing .track-title strong { color: var(--ac); }
  .track-art { width: 44px; height: 44px; border-radius: 10px; overflow: hidden; }
  .track-title { min-width: 0; }
  .track-title strong { font-size: 15px; line-height: 1.4; font-weight: 700; overflow-wrap: anywhere; color: var(--tx); }
  .track-title p { font-size: 13px; margin: 2px 0 0; overflow-wrap: anywhere; }
  .track-number, .duration { font-size: 13px; font-variant-numeric: tabular-nums; color: var(--mu); }
  .track-play, .track-play:hover:not(:disabled) { width: 44px; padding: 0; border: 0; border-radius: 14px; background: transparent; }
  .track-play:hover:not(:disabled) { background: var(--pn-2); }
  .track-actions { position: relative; }
  .track-actions > summary { list-style: none; display: grid; place-items: center; min-height: 44px; width: 44px; cursor: pointer; border-radius: 14px; }
  .track-actions > summary:hover { background: var(--pn-2); }
  summary::-webkit-details-marker { display: none; }
  .track-actions[open] > summary { background: var(--pn-2); }
  .track-menu { position: absolute; right: 0; top: 100%; width: min(260px, calc(100vw - 48px)); z-index: 10; background: var(--bg-3); border: 1px solid var(--line); border-radius: var(--r-md); box-shadow: var(--shadow); padding: 6px; }
  .track-menu > button { justify-content: start; width: 100%; border: 0; background: none; font-weight: 600; }
  .track-menu p { font-size: 12px; margin: 10px 12px; }
  .pagination { display: flex; align-items: center; justify-content: center; flex-wrap: wrap; gap: 12px; margin: 28px 0 8px; }
  .pagination span { font-size: 14px; font-weight: 600; color: var(--mu); }
  .empty { padding: 40px 24px; max-width: 65ch; margin: 0 auto; text-align: center; }
  .error { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; padding: 14px 16px; margin-bottom: 20px; border-radius: var(--r-md); background: var(--err-soft); }
  .error p { margin: 0; color: var(--err); font-weight: 600; }
  .loading { margin-bottom: 24px; color: var(--mu); font-size: 14px; }
  .skeletons { display: grid; gap: 8px; margin-top: 12px; }
  .skeletons span { height: 60px; background: var(--pn-2); border-radius: var(--r-sm); }
  @media (max-width: 600px) {
    main { padding-top: 12px; }
    .search button { width: 44px; padding: 0; flex: none; }
    .search button span { display: none; }
    .music-covers { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 24px 12px; }
    article { grid-template-columns: 40px minmax(0, 1fr) 44px 44px; gap: 8px; padding: 8px 10px; }
    .track-number, .duration { display: none; }
    .track-art { width: 40px; height: 40px; }
    .track-title strong { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
    .track-title p { white-space: nowrap; text-overflow: ellipsis; overflow: hidden; }
    .group-heading { flex-direction: column; align-items: flex-start; gap: 16px; }
    .group-cover { width: 160px; height: 160px; }
  }
</style>
