<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { loadArtworkRotationPolicy } from './artwork-rotation-policy.svelte';
  import { provideQuickPlay } from './quick-play';
  import { serverPresentation } from '../config/presentation';
  import { ensurePreferences, savePreferences } from '../preferences';
  import { page } from '$app/state';
  import { goto, beforeNavigate, afterNavigate } from '$app/navigation';
  import { cachedCatalog, loadCatalog, invalidateCatalog, catalogRevision, saveCatalogScroll, catalogScroll } from './page-cache';
  import { resolve } from '$app/paths';
  import { DEFAULT_NAME, fetchBrandName } from '../config/brand';
  import type ImmersivePlayer from '../player/ImmersivePlayer.svelte';
  import { remote } from '../remote/remote.svelte';
  import type { Media, PlayerFactory } from '../player/types';
  import Navbar from '../ui/Navbar.svelte';
  import { bestVersion } from '../player/version-choice';
  import CatalogHome from './CatalogHome.svelte';
  import { selectArtwork, renewArtworkVisit } from './artwork-selection';
  import { imagePreviewUrl } from './image-url';
  import CatalogGrid from './CatalogGrid.svelte';
  import CatalogSkeleton from './CatalogSkeleton.svelte';
  import Icon from '../ui/Icon.svelte';
  import {
    request,
    playbackIds,
    fetchMediaDescriptors,
    revalidateMediaDescriptor,
    getCachedMedia,
    isAbortError,
    isCatalogAccessError,
    type CatalogPage,
    type Work
  } from './api';
  import { preloadProgress } from '../player/progress';
  import { session, getSessionGeneration } from '../auth/session';
  import { START_WATCH_PARTY, partyFromLocation, takePendingParty, type StartWatchPartyDetail } from '../syncplay/launch';
  import { joinParty as joinPartyRequest } from '../syncplay/party-api';
  import { t } from '../i18n';

  let { section = 'home' }: { section?: 'home' | 'films' | 'series' | 'search' | 'settings' } = $props();
  let name = $state(DEFAULT_NAME);
  let media = $state<Media[]>([]);
  let mediaError = $state('');
  let busy = $state(true);
  let pendingCatalogKinds = $state(0);
  let partialCatalogError = $state('');
  let error = $state('');
  let films = $state<Work[]>([]);
  let series = $state<Work[]>([]);
  let totalFilms = $state<number | null>(null);
  let totalSeries = $state<number | null>(null);
  let moreFilms = $state(false);
  let moreSeries = $state(false);
  let detail = $state<Work | null>(null);
  let selected = $state('');
  let playingEpisode = $state<Work | null>(null);
  let followingEpisode = $state<Work | undefined>();
  let season = $state(1);
  let search = $state('');
  let sort = $state('title-asc');
  let selectedKindFilter = $state<'all' | 'film' | 'series'>('all');
  let immersive = $state<ImmersivePlayer>();
  let ImmersivePlayerComponent = $state<typeof ImmersivePlayer>();
  let createPlayer = $state<PlayerFactory>();
  let playerLoading: Promise<void> | undefined;
  let launchingEpisodeId = $state<string | null>(null);
  let launching = $state(false);
  let episodeLaunchController: AbortController | null = null;

  // Account Preferences (saved in localStorage for persistence)
  let prefAudio = $state('fr');
  let prefSubtitles = $state('foreign');

  async function launchEpisode(episode: Work, versionId: string): Promise<void> {
    quickWork = null;
    if (episodeLaunchController) {
      episodeLaunchController.abort();
      episodeLaunchController = null;
    }
    if (!episode.synthetic) preloadProgress(episode.id);
    const controller = new AbortController();
    episodeLaunchController = controller;
    const launchGen = getSessionGeneration();
    launchingEpisodeId = episode.id;
    mediaError = '';

    try {
      const descriptor = (async () => {
        if (versionId !== 'auto') return revalidateMediaDescriptor(versionId, controller.signal);
        const ids = playbackIds(episode);
        const known = ids.map(getCachedMedia).filter((item): item is Media => item !== undefined);
        const probe = document.createElement('video');
        if (known.length === ids.length) {
          const candidate = bestVersion(known, mime => probe.canPlayType(mime));
          if (!candidate) throw new Error($t('no_version_available'));
          return revalidateMediaDescriptor(candidate.id, controller.signal);
        }
        const fresh = await fetchMediaDescriptors(ids, controller.signal, { revalidate: true });
        const candidate = bestVersion(fresh, mime => probe.canPlayType(mime));
        if (!candidate) throw new Error($t('no_version_available'));
        return candidate;
      })();
      void descriptor.catch(() => {});
      await loadPlayer();
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      const openedHere = immersive?.open(descriptor);
      const item = await descriptor;
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      const cachedForEpisode = playbackIds(episode)
        .map(eid => (eid === item.id ? item : getCachedMedia(eid)))
        .filter((m): m is Media => m !== undefined);
      const otherMedia = media.filter(m => !cachedForEpisode.some(c => c.id === m.id));
      media = [...otherMedia, ...cachedForEpisode];
      playingEpisode = episode;
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      if (!openedHere) immersive?.changeMedia(item, true);
    } catch (err) {
      if (!controller.signal.aborted && launchGen === getSessionGeneration()) {
        media = media.filter(m => m.id !== versionId && getCachedMedia(m.id) !== undefined);
        mediaError = err instanceof Error ? err.message : $t('episode_load_failed');
        // Revalidation failed (e.g. HTTP 403): do not leave the player open on an unusable source.
        immersive?.close();
      }
    } finally {
      if (episodeLaunchController === controller) {
        launchingEpisodeId = null;
        episodeLaunchController = null;
      }
    }
  }

  async function loadPlayer(): Promise<void> {
    if (ImmersivePlayerComponent && createPlayer) return;
    if (playerLoading) return playerLoading;
    playerLoading = (async () => {
      const [{ default: comp }, { NativePlayer }] = await Promise.all([
        import('../player/ImmersivePlayer.svelte'),
        import('../player/native')
      ]);
      ImmersivePlayerComponent = comp;
      createPlayer = (video, onError) => new NativePlayer(video, onError);
    })();
    try { await playerLoading; }
    finally { playerLoading = undefined; }
  }

  beforeNavigate(({ to }) => {
    // Opening a work keeps the artwork just clicked; any other page draws new artwork.
    if (!to?.url.searchParams.get('work')) renewArtworkVisit();
    if (!workId) saveCatalogScroll(page.url.pathname + page.url.search, window.scrollY);
  });
  afterNavigate(() => {
    if (workId) return;
    const key = page.url.pathname + page.url.search;
    const generation = getSessionGeneration();
    const y = catalogScroll(key);
    if (y !== undefined) void tick().then(() => requestAnimationFrame(() => {
      if (key === page.url.pathname + page.url.search && generation === getSessionGeneration()) window.scrollTo({top: y, behavior: 'instant'});
    }));
  });

  const workId = $derived(page.url.searchParams.get('work') ?? '');
  const libraryId = $derived(page.url.searchParams.get('library') ?? '');
  const query = $derived(page.url.searchParams.get('q') ?? '');
  const order = $derived(
    ['title-asc', 'title-desc', 'year-asc', 'year-desc', 'added-asc', 'added-desc'].includes(
      page.url.searchParams.get('sort') ?? ''
    )
      ? page.url.searchParams.get('sort')!
      : section === 'home'
        ? 'added-desc'
        : 'title-asc'
  );
  const offset = $derived(Math.max(0, Number(page.url.searchParams.get('offset')) || 0));
  const title = $derived(
    section === 'settings'
      ? $t('settings_title')
      : section === 'films'
        ? $t('films')
        : section === 'series'
          ? $t('series')
          : section === 'search'
            ? $t('search')
            : $t('home')
  );
  /** A film started from a poster's quick action, without leaving the current page. */
  let quickWork = $state<Work | null>(null);
  const playbackSource = $derived(quickWork ?? detail);
  const allowedIds = $derived(playbackSource ? playbackIds(playbackSource) : []);
  const playableMedia = $derived(media.filter(item => allowedIds.includes(item.id)));
  const episodeIds = $derived(playingEpisode ? playbackIds(playingEpisode) : []);
  const playerMedia = $derived(!quickWork && detail?.kind === 'series' ? media.filter(item => episodeIds.includes(item.id)) : playableMedia);

  const nextEpisode = $derived.by(() => {
    if (!playingEpisode || detail?.kind !== 'series') return undefined;
    if (detail.episodesPage) return followingEpisode;
    const episodes = (detail.seasons ?? []).flatMap(s => s.episodes);
    const index = episodes.findIndex(episode => episode.id === playingEpisode?.id);
    return index >= 0 ? episodes[index + 1] : undefined;
  });

  // Carte « À suivre » du lecteur : vignette de l'épisode, sinon images de la série.
  const nextImage = $derived.by(() => {
    if (!nextEpisode) return '';
    const episode = selectArtwork(nextEpisode, $session);
    const show = selectArtwork(detail, $session);
    return imagePreviewUrl(episode.thumb || episode.landscape || episode.backdrop || show.landscape || show.backdrop || show.poster, 640);
  });
  const nextLabel = $derived(nextEpisode ? [nextEpisode.seasonNumber ? `Saison ${nextEpisode.seasonNumber}` : '', nextEpisode.number ? `Épisode ${nextEpisode.number}` : ''].filter(Boolean).join(' · ') : '');
  $effect(() => {
    const work = detail;
    const episode = playingEpisode;
    followingEpisode = undefined;
    if (!work?.episodesPage || !episode || work.kind !== 'series') return;
    const controller = new AbortController();
    const generation = getSessionGeneration();
    const params = new URLSearchParams({ episodeAfter: episode.id, episodeLimit: '1' });
    void request<Work>(`/api/v1/catalog/${encodeURIComponent(work.id)}?${params}`, controller.signal).then(result => {
      if (!controller.signal.aborted && generation === getSessionGeneration()) {
        followingEpisode = result.seasons?.flatMap(season => season.episodes)[0];
      }
    }).catch(() => {});
    return () => controller.abort();
  });

  function playNext() {
    if (!nextEpisode) return;
    const next = nextEpisode;
    const id = playbackIds(next)[0];
    if (!id) {
      mediaError = $t('next_episode_unavailable');
      return;
    }
    void launchEpisode(next, id);
  }

  function detailUrl(work: Work) {
    return work.kind === 'series'
      ? resolve(`series/?work=${encodeURIComponent(work.id)}`)
      : resolve(`films/?work=${encodeURIComponent(work.id)}`);
  }

  async function joinFromLink(partyId: string, mediaId?: string) {
    const url = new URL(window.location.href);
    url.searchParams.delete('party'); url.searchParams.delete('media');
    history.replaceState(history.state, '', url);
    try {
      await joinPartyRequest(partyId, $session?.csrfToken ?? '');
      let target = mediaId;
      if (!target) {
        const response = await fetch(`/api/v1/parties/${encodeURIComponent(partyId)}`, { credentials: 'same-origin', cache: 'no-store' });
        if (!response.ok) throw new Error($t('party_join_failed'));
        target = ((await response.json()) as { party?: { mediaId?: string } }).party?.mediaId;
      }
      await launchWatchParty(partyId, target);
    } catch (cause) {
      mediaError = cause instanceof Error ? cause.message : $t('party_join_failed');
    }
  }

  async function launchWatchParty(partyId: string, targetMediaId?: string) {
    const current = playableMedia.find(item => item.id === selected) ?? playableMedia[0];
    // A session points at the work; the open page plays the version chosen on it, other
    // pages let the server pick the work's first playable file.
    const chosen = targetMediaId && targetMediaId === detail?.id && current ? current.id : targetMediaId;
    const targetId = chosen || current?.id || (allowedIds.includes(selected) ? selected : allowedIds[0]);
    if (!targetId) return;
    const controller = new AbortController();
    episodeLaunchController = controller;
    launching = true;
    mediaError = '';

    try {
      const descriptor = revalidateMediaDescriptor(targetId, controller.signal);
      void descriptor.catch(() => {});
      await loadPlayer();
      await tick();
      immersive?.joinParty(partyId, descriptor);
      const item = await descriptor;
      media = [...media.filter(m => m.id !== item.id), item];
      selected = item.id;
    } catch (err) {
      mediaError = err instanceof Error ? err.message : $t('party_join_failed');
      immersive?.close();
    } finally {
      launching = false;
    }
  }

  async function openPlayer(startAt?: number): Promise<void> {
    const current = playableMedia.find(item => item.id === selected) ?? playableMedia[0];
    const targetId = current?.id ?? (allowedIds.includes(selected) ? selected : allowedIds[0]);
    if (!targetId) return;
    if (episodeLaunchController) {
      episodeLaunchController.abort();
      episodeLaunchController = null;
    }
    const controller = new AbortController();
    episodeLaunchController = controller;
    if (playbackSource && !playbackSource.synthetic) preloadProgress(playbackSource.id);
    const launchGen = getSessionGeneration();
    launching = true;
    mediaError = '';

    try {
      const descriptor = revalidateMediaDescriptor(targetId, controller.signal);
      // Attach rejection immediately while the optional module download completes.
      void descriptor.catch(() => {});
      await loadPlayer();
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      immersive?.open(descriptor, startAt);
      const item = await descriptor;
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      media = [...media.filter(m => m.id !== item.id), item];
      selected = item.id;
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
    } catch (err) {
      if (!controller.signal.aborted && launchGen === getSessionGeneration()) {
        media = media.filter(m => m.id !== targetId && getCachedMedia(m.id) !== undefined);
        mediaError = err instanceof Error ? err.message : $t('sources_load_failed');
        immersive?.close();
      }
    } finally {
      if (episodeLaunchController === controller) {
        episodeLaunchController = null;
        launching = false;
      }
    }
  }

  provideQuickPlay(work => handleDirectPlay(work));
  // "Open" from another device, or "Resume here": plays at the handed-over position.
  onMount(() => { remote.catalogs++; return () => { remote.catalogs--; }; });
  $effect(() => {
    const pending = remote.pendingOpen;
    if (!pending) return;
    remote.pendingOpen = null;
    immersive?.close();
    void handleDirectPlay({ id: pending.workId } as Work, pending.position);
  });
  async function handleDirectPlay(work: Work, startAt?: number): Promise<void> {
    if (work.kind === 'series') {
      // detailUrl already applies SvelteKit resolve to the route and base.
      // eslint-disable-next-line svelte/no-navigation-without-resolve
      await goto(detailUrl(work));
      return;
    }
    // Cards and shelves carry no playable versions: read the full work first.
    let full = work;
    if (!playbackIds(work).length) {
      try { full = await request<Work>(`/api/v1/catalog/${encodeURIComponent(work.id)}?episodeLimit=1`, AbortSignal.timeout(10000)); } catch { full = work; }
    }
    const targetId = playbackIds(full)[0];
    if (!targetId) {
      // detailUrl already applies SvelteKit resolve to the route and base.
      // eslint-disable-next-line svelte/no-navigation-without-resolve
      await goto(detailUrl(work));
      return;
    }
    try {
      // The player opens over the current page; the page itself does not change.
      quickWork = detail?.id === full.id ? null : full;
      selected = targetId;
      await openPlayer(startAt);
    } catch {
      // detailUrl already applies SvelteKit resolve to the route and base.
      // eslint-disable-next-line svelte/no-navigation-without-resolve
      await goto(detailUrl(work));
    }
  }

  onMount(() => {
    const controller = new AbortController();
    void loadArtworkRotationPolicy(controller.signal);
    fetchBrandName(controller.signal)
      .then(brandName => {
        name = brandName;
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          mediaError = $t('source_unavailable');
        }
      });

    void ensurePreferences().then(saved => {
      if (controller.signal.aborted || !saved) return;
      const savedAudio = saved.audioLanguage;
      if (savedAudio) prefAudio = savedAudio;
      const savedSub = saved.subtitles;
      if (savedSub) prefSubtitles = savedSub;
    }).catch(() => {});

    const artworkChanged=(event:Event)=>{
      const id=(event as CustomEvent<{id:string}>).detail.id;
      invalidateCatalog(detail?.libraryId);
      if(detail?.id===id)void request<Work>(`/api/v1/catalog/${encodeURIComponent(id)}?episodeLimit=24`,controller.signal).then(work=>{if(!controller.signal.aborted&&detail?.id===id)detail=work;}).catch(()=>{});
    };
    window.addEventListener('media-artwork-updated',artworkChanged);
    const onStartParty = (e: Event) => {
      // Taken here: the launcher does not fall back to the home page.
      e.preventDefault();
      const d = (e as CustomEvent<StartWatchPartyDetail>).detail;
      void launchWatchParty(d.partyId, d.mediaId || d.workId);
    };
    window.addEventListener(START_WATCH_PARTY, onStartParty);
    // Invitation link or a session started from a page without the player: `?party=`,
    // or the invitation link opened before signing in.
    const requested = partyFromLocation(window.location.search) ?? takePendingParty();
    if (requested) void joinFromLink(requested.partyId, requested.mediaId);
    return () => {controller.abort();window.removeEventListener('media-artwork-updated',artworkChanged);window.removeEventListener(START_WATCH_PARTY, onStartParty);};
  });

  function savePreference(key: string, value: string) {
    if (key === 'media_pref_audio') savePreferences({ audioLanguage: value });
    if (key === 'media_pref_subtitles' && (value === 'forced' || value === 'foreign' || value === 'always' || value === 'off')) savePreferences({ subtitles: value });
    try {
      localStorage.setItem(key, value);
    } catch {
      // Ignore
    }
  }

  $effect.pre(() => {
    void $catalogRevision;
    const id = workId;
    const q = query;
    const currentLibrary = libraryId;
    const sorting = order;
    const start = offset;
    const filters=Object.fromEntries(['genre','year','quality','codec','language'].flatMap(key=>{const value=page.url.searchParams.get(key);return value?[[key,value]]:[];}));
    const currentSection = section;
    const activeSessionKey = $session ? `${$session.user.id}:${$session.csrfToken}` : 'anonymous';
    void activeSessionKey;
    const effectSessionGen = getSessionGeneration();
    const controller = new AbortController();

    search = q;
    sort = sorting;

    launching = false;
    pendingCatalogKinds = 0;
    partialCatalogError = '';
    error = '';
    mediaError = '';
    detail = null;
    media = [];
    totalFilms = null;
    totalSeries = null;
    moreFilms = false;
    moreSeries = false;
    playingEpisode = null;

    const kinds = currentSection === 'films' ? ['film'] : currentSection === 'series' ? ['series'] : ['film', 'series'];
    const requests = kinds.map(kind => {
      const url = `/api/v1/catalog?${new URLSearchParams({ ...filters, ...(currentLibrary ? {library: currentLibrary} : {}), ...(currentSection === 'home' ? { scope: 'latest' } : {}), kind, q, sort: currentSection === 'home' ? 'added-desc' : sorting, offset: String(start), limit: '24', count: 'false' })}`;
      return { kind, url, key: url };
    });
    let cachedKinds = 0;
    if (!id) for (const {kind, key} of requests) {
      const cached = cachedCatalog(key);
      if (!cached) continue;
      cachedKinds++;
      if (kind === 'film') { films = cached.items; totalFilms = cached.total; moreFilms = cached.hasMore ?? (cached.total ?? 0) > start + 24; }
      else { series = cached.items; totalSeries = cached.total; moreSeries = cached.hasMore ?? (cached.total ?? 0) > start + 24; }
    }
    if (cachedKinds === 0) {
      films = [];
      series = [];
    }
    busy = Boolean(id) || cachedKinds === 0;

    async function load() {
      try {
        if (currentSection === 'settings') return;
        if (id) {
          if (currentSection !== 'films' && currentSection !== 'series') {
            throw new Error($t('open_media_from_library'));
          }
          void loadPlayer().catch(() => { /* A playback attempt reports the import error and can retry. */ });
          const work = await request<Work>(`/api/v1/catalog/${encodeURIComponent(id)}?episodeLimit=24&episodeOffset=0`, controller.signal);
          if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
          if (
            (currentSection === 'films' && work.kind !== 'film') ||
            (currentSection === 'series' && work.kind !== 'series')
          ) {
            throw new Error($t('wrong_library_media'));
          }
          detail = work;
          busy = false;
          season = work.seasons?.[0]?.number ?? 1;

          if (work.kind === 'film') {
            const ids = playbackIds(work);
            selected = ids[0] ?? '';
            try {
              const items = await fetchMediaDescriptors(ids, controller.signal);
              if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) {
                media = items;
                const probe=document.createElement('video');
                selected=bestVersion(items,mime=>probe.canPlayType(mime))?.id??selected;
              }
            } catch (failure) {
              if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) {
                mediaError = failure instanceof Error ? failure.message : $t('sources_load_failed');
              }
            }
          } else {
            selected = '';
            const allEpisodeIds = (work.seasons ?? []).flatMap(s => s.episodes.flatMap(playbackIds));
            media = allEpisodeIds.map(eid => getCachedMedia(eid)).filter((m): m is Media => m !== undefined);
          }
        } else {
          pendingCatalogKinds = kinds.length;
          let completedKinds = 0;
          const results = await Promise.allSettled(
            requests.map(async ({kind, key, url}) => {
              try {
                let result: CatalogPage;
                try { result = await loadCatalog(key, url); }
                catch (cause) {
                  // A shared request can be cancelled under us (another shelf got a 403, the cache was
                  // invalidated): that is not this page's answer. Ask once more for our own.
                  if (!isAbortError(cause) || controller.signal.aborted || effectSessionGen !== getSessionGeneration()) throw cause;
                  result = await loadCatalog(key, url);
                }
                if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
                if (kind === 'film') {
                  films = result.items;
                  totalFilms = result.total;
                  moreFilms = result.hasMore ?? (result.total ?? 0) > start + 24;
                } else {
                  series = result.items;
                  totalSeries = result.total;
                  moreSeries = result.hasMore ?? (result.total ?? 0) > start + 24;
                }
                completedKinds += 1;
                // Render the ready shelf and start its images immediately.
                busy = false;
              } finally {
                if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) pendingCatalogKinds -= 1;
              }
            })
          );
          if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
          void loadPlayer().catch(() => {});
          const failures = results.flatMap(result => result.status === 'rejected' ? [result.reason as unknown] : []);
          // An account-level answer explains every other failure (a 403 cancels the shelves still loading).
          const failure = failures.find(isCatalogAccessError) ?? failures.find(cause => !isAbortError(cause)) ?? failures[0];
          if (failure !== undefined) {
            if (isCatalogAccessError(failure)) { films = []; series = []; throw failure; }
            if (completedKinds === 0 && cachedKinds === 0) throw isAbortError(failure) ? new Error($t('catalog_connection_failed')) : failure;
            partialCatalogError = $t('partial_catalog_unavailable');
          }
        }
      } catch (e) {
        if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) {
          error = e instanceof Error ? e.message : $t('catalog_connection_failed');
        }
      } finally {
        if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) {
          busy = false;
        }
      }
    }

    void load();
    return () => {
      controller.abort();
      if (episodeLaunchController) {
        episodeLaunchController.abort();
        episodeLaunchController = null;
      }
    };
  });
</script>

<svelte:head>
  <title>{detail?.title ?? title} · {name}</title>
  <meta name="description" content="Films et séries en haute fidélité. Bibliothèques locales et lecture directe." />
</svelte:head>

<a class="skip-link" href="#main-content">{$t('skip_to_content')}</a>

<Navbar {section} {name} />

<main id="main-content" tabindex="-1">
  {#if launching}<p role="status">{$t('connecting_source')} <button onclick={()=>{episodeLaunchController?.abort();episodeLaunchController=null;launching=false;}}>{$t('cancel')}</button></p>{/if}
  {#if !busy && pendingCatalogKinds > 0}<p role="status">{$t('loading_catalog_remainder')}</p>{/if}
  {#if partialCatalogError}<p role="alert">{partialCatalogError}</p>{/if}
  {#if section === 'home' && $serverPresentation.welcomeMessage}<p class="server-welcome">{$serverPresentation.welcomeMessage}</p>{/if}
  {#if section === 'settings'}
    {#await import('./SettingsView.svelte')}
      <CatalogSkeleton label={$t('loading_settings')} />
    {:then { default: SettingsView }}
    <SettingsView
      bind:prefAudio
      bind:prefSubtitles
      onSavePreference={savePreference}
    />
    {:catch}<p role="alert">{$t('settings_load_failed')}</p>{/await}

  {:else if busy}
    <div class="page-heading">
      <span class="eyebrow">{title}</span>
      <h1>{$t('all_media_library')}</h1>
    </div>
    <CatalogSkeleton />

  {:else if error}
    <div class="page-heading">
      <h1>{title}</h1>
    </div>
    <div class="empty error" role="alert">
      <Icon name="alert" size={32} />
      <h2>{$t('catalog_unavailable')}</h2>
      <p>{error}</p>
      <a
        class="retry-action-btn"
        href={resolve(
          section === 'series'
            ? `series/?${page.url.searchParams}`
            : section === 'films'
              ? `films/?${page.url.searchParams}`
              : section === 'search'
                ? `search/?${page.url.searchParams}`
                : '/'
        )}
        data-sveltekit-reload
      >
        {$t('retry')}
      </a>
    </div>

  {:else if detail}
    {#if detail.kind === 'film'}
      {#await import('./FilmDetail.svelte')}
        <CatalogSkeleton label={$t('loading_movie')} />
      {:then { default: FilmDetail }}
      <FilmDetail
        work={detail}
        {playableMedia}
        {allowedIds}
        bind:selected
        {mediaError}
        similarCandidates={films}
        onPlay={() => { quickWork = null; return openPlayer(); }}
      />
      {:catch}<p role="alert">{$t('movie_load_failed')}</p>{/await}
    {:else}
      {#await import('./SeriesDetail.svelte')}
        <CatalogSkeleton label={$t('loading_episodes')} />
      {:then { default: SeriesDetail }}
      <SeriesDetail
        work={detail}
        bind:season
        {media}
        {mediaError}
        similarCandidates={series}
        {immersive}
        {launchingEpisodeId}
        onLaunchEpisode={launchEpisode}
        onSelectEpisode={episode => (playingEpisode = episode)}
      />
      {:catch}<p role="alert">{$t('episodes_load_failed')}</p>{/await}
    {/if}

  {:else if section === 'home'}
    <!-- Refined, Immersive Home View -->
    <CatalogHome
      {films}
      {series}
      featured={films[0] ?? series[0] ?? null}
      catalogReady={pendingCatalogKinds === 0}
      onPlay={handleDirectPlay}
      {detailUrl}
    />

  {:else}
    <!-- Films, Series and Search Grid View -->
    <CatalogGrid
      {section}
      {films}
      {series}
      {totalFilms}
      {totalSeries}
      {moreFilms}
      {moreSeries}
      bind:search
      bind:sort
      bind:selectedKindFilter
      {query}
      {offset}
      {title}
      {detailUrl}
    />
  {/if}
</main>

{#if (detail || quickWork) && ImmersivePlayerComponent && createPlayer}
  <ImmersivePlayerComponent
    bind:this={immersive}
    catalogue={playerMedia}
    {createPlayer}
    nextTitle={nextEpisode?.title}
    {nextLabel}
    {nextImage}
    onNext={nextEpisode ? playNext : undefined}
  />
{/if}

<style>
  .server-welcome { margin: 0; padding: 16px 0; color: var(--mu); font-size: 15px; line-height: 1.6; white-space: pre-wrap; overflow-wrap: anywhere; }
  .skip-link {
    position: fixed;
    top: -80px;
    left: 20px;
    z-index: 50;
    background: var(--btn);
    color: var(--btn-tx);
    padding: 12px 18px;
    border-radius: var(--r-md);
    font-weight: 700;
    transition: top var(--t-fast) ease;
  }

  .skip-link:focus {
    top: 14px;
  }

  .skip-link:focus-visible,
  .retry-action-btn:focus-visible,
  main button:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  main {
    width: 100%;
    max-width: var(--maxw);
    margin: 0 auto;
    padding: 0 var(--gutter) 80px;
    min-height: 75vh;
    min-width: 0;
  }

  main:focus { outline: none; }

  main > [role='status'],
  main > [role='alert'] {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    margin: 12px 0 0;
    padding: 12px 16px;
    border-radius: var(--r-md);
    background: var(--pn);
    color: var(--mu);
    font-size: 14px;
    font-weight: 600;
  }

  main > [role='alert'] {
    background: var(--err-soft);
    color: var(--err);
  }

  main > [role='status'] button {
    min-height: 40px;
    padding: 0 16px;
    border: 0;
    border-radius: var(--r-sm);
    background: var(--pn-2);
    color: var(--tx);
    font: inherit;
    font-weight: 700;
    cursor: pointer;
  }

  main > [role='status'] button:hover { background: var(--pn-3); }

  .page-heading {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-top: 20px;
  }

  .page-heading .eyebrow {
    margin: 0;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    color: var(--mu);
  }

  h1 {
    font-size: clamp(32px, 4vw, calc(44px * var(--ui-scale, 1)));
    font-weight: 800;
    line-height: 1.02;
    letter-spacing: -0.03em;
    color: var(--tx);
    margin: 4px 0 16px;
  }

  /* États vide / erreur */
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    max-width: 640px;
    margin: 32px auto;
    padding: 48px 24px;
    border-radius: var(--r-lg);
    background: var(--bg-2);
    border: 1px solid var(--line);
    text-align: center;
  }

  .error {
    color: var(--err);
  }

  .error h2 {
    font-size: 24px;
    font-weight: 800;
    letter-spacing: -0.02em;
    margin: 0;
    color: var(--tx);
  }

  .error p {
    margin: 0;
    color: var(--mu);
    font-size: 15px;
    line-height: 1.6;
    max-width: 500px;
  }

  .retry-action-btn {
    display: inline-flex;
    align-items: center;
    height: 48px;
    padding: 0 22px;
    border-radius: var(--r-md);
    background: var(--btn);
    color: var(--btn-tx);
    font-size: 15px;
    font-weight: 700;
    margin-top: 8px;
    transition: opacity var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }

  .retry-action-btn:hover {
    opacity: 0.9;
  }

  .retry-action-btn:active {
    transform: scale(0.98);
  }
</style>
