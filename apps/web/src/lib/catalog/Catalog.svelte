<script module lang="ts">
  const links = [{ key:'home', path:'/', label:'Accueil', symbol:'⌂' },{ key:'films', path:'films/', label:'Films', symbol:'▤' },{ key:'series', path:'series/', label:'Séries', symbol:'▥' },{ key:'search', path:'search/', label:'Recherche', symbol:'⌕' }] as const;
</script>
<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { page } from '$app/state';
  import { resolve } from '$app/paths';
  import { DEFAULT_NAME, fetchBrandName } from '../config/brand';
  import type ImmersivePlayer from '../player/ImmersivePlayer.svelte';
  import type { Media, PlayerFactory } from '../player/types';
  import Icon from '../ui/Icon.svelte';
  import ProfileMenu from '../ui/ProfileMenu.svelte';
  import Poster from './Poster.svelte';
  import EpisodePlayer from './EpisodePlayer.svelte';
  import {
    request,
    playbackIds,
    sourceLabel,
    fetchMediaDescriptors,
    revalidateMediaDescriptor,
    getCachedMedia,
    type Work,
    type CatalogPage
  } from './api';
  import { session, getSessionGeneration } from '../auth/session';
  let { section = 'home' }: { section?: 'home' | 'films' | 'series' | 'search' | 'settings' } = $props();
  let name = $state(DEFAULT_NAME);
  let media = $state<Media[]>([]);
  let mediaError = $state('');
  let catalogueNotice = $state('');
  let busy = $state(true);
  let error = $state('');
  let films = $state<Work[]>([]);
  let series = $state<Work[]>([]);
  let totalFilms = $state(0);
  let totalSeries = $state(0);
  let detail = $state<Work | null>(null);
  let selected = $state('');
  let playingEpisode = $state<Work | null>(null);
  let season = $state(1);
  let search = $state('');
  let sort = $state('title-asc');
  let immersive = $state<ImmersivePlayer>();
  let ImmersivePlayerComponent = $state<typeof ImmersivePlayer>();
  let createPlayer = $state<PlayerFactory>();
  let playerLoading = false;
  let launchingEpisodeId = $state<string | null>(null);
  let episodeLaunchController: AbortController | null = null;

  async function launchEpisode(episode: Work, versionId: string): Promise<void> {
    if (episodeLaunchController) {
      episodeLaunchController.abort();
      episodeLaunchController = null;
    }
    const controller = new AbortController();
    episodeLaunchController = controller;
    const launchGen = getSessionGeneration();
    launchingEpisodeId = episode.id;
    mediaError = '';

    try {
      const item = await revalidateMediaDescriptor(versionId, controller.signal);
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      const cachedForEpisode = playbackIds(episode)
        .map(eid => (eid === item.id ? item : getCachedMedia(eid)))
        .filter((m): m is Media => m !== undefined);
      const otherMedia = media.filter(m => !cachedForEpisode.some(c => c.id === m.id));
      media = [...otherMedia, ...cachedForEpisode];
      playingEpisode = episode;
      await tick();
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      immersive?.open(item);
    } catch (err) {
      if (!controller.signal.aborted && launchGen === getSessionGeneration()) {
        media = media.filter(m => m.id !== versionId && getCachedMedia(m.id) !== undefined);
        mediaError = err instanceof Error ? err.message : 'Chargement de l’épisode impossible.';
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
    if (playerLoading) return;
    playerLoading = true;
    try {
      const [{ default: comp }, { NativePlayer }] = await Promise.all([
        import('../player/ImmersivePlayer.svelte'),
        import('../player/native')
      ]);
      ImmersivePlayerComponent = comp;
      createPlayer = (video, onError) => new NativePlayer(video, onError);
    } finally {
      playerLoading = false;
    }
  }

  const base = $derived(resolve('/').replace(/\/$/, ''));
  const workId = $derived(page.url.searchParams.get('work') ?? '');
  const query = $derived(page.url.searchParams.get('q') ?? '');
  const order = $derived(page.url.searchParams.get('sort') === 'title-desc' ? 'title-desc' : 'title-asc');
  const offset = $derived(Math.max(0, Number(page.url.searchParams.get('offset')) || 0));
  const title = $derived(section === 'settings' ? 'Paramètres' : links.find(link => link.key === section)?.label ?? 'Accueil');
  const allowedIds = $derived(detail ? playbackIds(detail) : []);
  const playableMedia = $derived(media.filter(item => allowedIds.includes(item.id)));
  const episodeIds = $derived(playingEpisode ? playbackIds(playingEpisode) : []);
  const playerMedia = $derived(detail?.kind === 'series' ? media.filter(item => episodeIds.includes(item.id)) : playableMedia);
  const activeSeason = $derived(detail?.seasons?.find(item => item.number === season));
  function detailUrl(work: Work) { return work.kind === 'series' ? resolve(`series/?work=${encodeURIComponent(work.id)}`) : resolve(`films/?work=${encodeURIComponent(work.id)}`); }
  async function openPlayer(): Promise<void> {
    const current = playableMedia.find(item => item.id === selected) ?? playableMedia[0];
    const targetId = current?.id ?? (allowedIds.includes(selected) ? selected : allowedIds[0]);
    if (!targetId) return;
    if (episodeLaunchController) {
      episodeLaunchController.abort();
      episodeLaunchController = null;
    }
    const controller = new AbortController();
    episodeLaunchController = controller;
    const launchGen = getSessionGeneration();
    mediaError = '';

    try {
      const item = await revalidateMediaDescriptor(targetId, controller.signal);
      if (controller.signal.aborted || launchGen !== getSessionGeneration()) return;
      media = [...media.filter(m => m.id !== item.id), item];
      selected = item.id;
      immersive?.open(item);
    } catch (err) {
      if (!controller.signal.aborted && launchGen === getSessionGeneration()) {
        media = media.filter(m => m.id !== targetId && getCachedMedia(m.id) !== undefined);
        mediaError = err instanceof Error ? err.message : 'Chargement des sources impossible.';
      }
    } finally {
      if (episodeLaunchController === controller) {
        episodeLaunchController = null;
      }
    }
  }
  onMount(() => {
    const controller = new AbortController();
    fetchBrandName(controller.signal)
      .then(brandName => { name = brandName; })
      .catch(() => { if (!controller.signal.aborted) mediaError = 'La lecture est temporairement indisponible. Rechargez la page pour réessayer.'; });
    return () => controller.abort();
  });
  $effect(() => {
    const id = workId, q = query, sorting = order, start = offset, currentSection = section;
    const activeSessionKey = $session ? `${$session.user.id}:${$session.csrfToken}` : 'anonymous';
    void activeSessionKey;
    const effectSessionGen = getSessionGeneration();
    const controller = new AbortController();
    search = q; sort = sorting; busy = true; error = ''; mediaError = ''; detail = null; media = []; films = []; series = []; totalFilms = 0; totalSeries = 0; playingEpisode = null;
    async function load() {
      try {
        if (currentSection === 'settings') return;
        if (id) {
          if (currentSection !== 'films' && currentSection !== 'series') throw new Error('Ouvrez ce média depuis sa bibliothèque Films ou Séries.');
          const playerPromise = loadPlayer();
          const work = await request<Work>(`/api/v1/catalog/${encodeURIComponent(id)}`, controller.signal);
          if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
          if ((currentSection === 'films' && work.kind !== 'film') || (currentSection === 'series' && work.kind !== 'series')) throw new Error('Ce média appartient à une autre bibliothèque.');
          await playerPromise;
          if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
          detail = work; season = work.seasons?.[0]?.number ?? 1;
          if (work.kind === 'film') {
            const ids = playbackIds(work);
            selected = ids[0] ?? '';
            try {
              const items = await fetchMediaDescriptors(ids, controller.signal);
              if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) media = items;
            } catch (failure) {
              if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) {
                mediaError = failure instanceof Error ? failure.message : 'Chargement des sources impossible.';
              }
            }
          } else {
            selected = '';
            const allEpisodeIds = (work.seasons ?? []).flatMap(s => s.episodes.flatMap(playbackIds));
            media = allEpisodeIds.map(eid => getCachedMedia(eid)).filter((m): m is Media => m !== undefined);
          }
        } else {
          const kinds = currentSection === 'films' ? ['film'] : currentSection === 'series' ? ['series'] : ['film','series'];
          const results = await Promise.all(kinds.map(kind => request<CatalogPage>(`/api/v1/catalog?${new URLSearchParams({ kind, q, sort:sorting, offset:String(start), limit:'24' })}`, controller.signal)));
          if (controller.signal.aborted || effectSessionGen !== getSessionGeneration()) return;
          films = []; series = []; totalFilms = 0; totalSeries = 0;
          catalogueNotice = [...new Set(results.map(result => result.notice).filter(Boolean))].join(' ');
          results.forEach((result, index) => { if (kinds[index] === 'film') { films = result.items; totalFilms = result.total; } else { series = result.items; totalSeries = result.total; } });
        }
      } catch (e) { if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) error = e instanceof Error ? e.message : 'Connexion au catalogue impossible.'; }
      finally { if (!controller.signal.aborted && effectSessionGen === getSessionGeneration()) busy = false; }
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
<svelte:head><title>{detail?.title ?? title} · {name}</title><meta name="description" content="Films et séries, chacun à leur place. Bibliothèques locales." /></svelte:head>
<a class="skip-link" href="#main-content">Aller au contenu</a>
<header class="site-header">
  <a class="brand" href={resolve('/')} aria-label={`${name}, accueil`}><img src={`${base}/images/brand-mark.svg`} width="25" height="30" alt="" /><span>{name}</span></a>
  <nav aria-label="Navigation principale">{#each links as link (link.key)}<a href={resolve(link.path)} aria-current={section === link.key ? 'page' : undefined}><span class:search-icon={link.key === 'search'} class:nav-symbol={link.key !== 'search'} aria-hidden="true">{#if link.key === 'search'}<Icon name="search" size={19} />{:else}{link.symbol}{/if}</span>{link.label}</a>{/each}</nav>
  <ProfileMenu />
</header>
<main id="main-content" tabindex="-1">
  {#if section === 'settings'}
    <div class="page-heading"><p class="eyebrow">VOTRE ESPACE</p><h1>Paramètres</h1><p>Vos réglages personnels de l’application.</p></div>
    <section class="settings-panel"><h2>Réglages personnels</h2><p>Les réglages personnels de l’application seront disponibles prochainement.</p><button disabled>Réglages personnels · Bientôt</button></section>
  {:else if busy}
    <div class="page-heading"><p class="eyebrow">{title}</p><h1>Votre médiathèque</h1></div><div class="empty" role="status">Chargement du catalogue…</div>
  {:else if error}
    <div class="page-heading"><h1>{title}</h1></div><div class="empty error" role="alert"><h2>Le catalogue ne répond pas</h2><p>{error}</p><a class="secondary" href={resolve(section === 'series' ? `series/?${page.url.searchParams}` : section === 'films' ? `films/?${page.url.searchParams}` : section === 'search' ? `search/?${page.url.searchParams}` : '/')} data-sveltekit-reload>Réessayer</a></div>
  {:else if detail}
    <a class="back-link" href={resolve(section === 'series' ? 'series/' : 'films/')}>← {section === 'series' ? 'Toutes les séries' : 'Tous les films'}</a>
    <section class="detail">
      <div class="detail-poster"><Poster work={detail} showMetadata={false} eager /></div>
      <div class="detail-copy"><p class="eyebrow">{detail.kind === 'series' ? 'SÉRIE' : 'FILM'}{detail.synthetic ? ' · DÉMONSTRATION' : ''}</p><h1>{detail.title}</h1><p class="metadata">{detail.year ?? 'Année non renseignée'}{detail.kind === 'series' ? ` · ${detail.seasonCount ?? 0} ${(detail.seasonCount ?? 0) > 1 ? 'saisons' : 'saison'} · ${detail.episodeCount ?? 0} ${(detail.episodeCount ?? 0) > 1 ? 'épisodes' : 'épisode'}` : ''}</p>{#if detail.genres?.length}<p class="metadata genres">{detail.genres.join(' · ')}</p>{/if}<p class="synopsis">{detail.description || 'Synopsis non renseigné.'}</p>
      {#if detail.kind === 'film'}
        <div class="versions" aria-label="Versions disponibles">{#each playableMedia as item (item.id)}<button class="version-choice" aria-pressed={selected === item.id} aria-label={`Sélectionner la version ${sourceLabel(item)}`} onclick={() => selected = item.id}>{sourceLabel(item)}</button>{/each}</div>
        <button class="hero-cta" disabled={!playableMedia.length && !allowedIds.length} onclick={openPlayer}><Icon name="play" size={18} />Lecture</button>
        {#if !playableMedia.length && !allowedIds.length}<p class="unavailable">Aucune version disponible pour la lecture.</p>{/if}
      {/if}
      {#if mediaError}<p class="error" role="alert">{mediaError}</p>{/if}
      {#if detail.synthetic}<p class="detail-note">Fiche de démonstration.</p>{/if}
      </div>
    </section>
    {#if detail.kind === 'series'}
      <section class="shelf"><div class="section-heading"><h2>Épisodes</h2><label>Saison <select aria-label="Saison" bind:value={season}>{#each detail.seasons ?? [] as item (item.id)}<option value={item.number}>{item.title || (item.number === 0 ? 'Hors-série' : `Saison ${item.number}`)}</option>{/each}</select></label></div>
        {#each activeSeason?.episodes ?? [] as episode (episode.id)}
          <article class="episode"><span class="episode-number">{String(episode.number).padStart(2,'0')}</span><div><h3>{episode.title}</h3><p>{episode.description}</p><EpisodePlayer {episode} {media} {immersive} isLaunching={launchingEpisodeId === episode.id} onlaunch={launchEpisode} onselect={() => playingEpisode = episode} /></div></article>
        {:else}<div class="empty">Aucun épisode disponible pour cette saison.</div>{/each}
      </section>
    {/if}
  {:else}
    {#if section === 'home'}
      <section class="home-intro"><div><p class="eyebrow">LE CINÉMA, CHEZ VOUS</p><h1>Une envie de cinéma.<br /><em>Une place pour chaque histoire.</em></h1><p>Retrouvez vos films. Explorez vos séries.<br />Votre prochaine séance commence ici.</p><div class="home-actions"><a class="primary" href={resolve('films/')}>Explorer les films <Icon name="arrow" size={16} /></a><a class="secondary" href={resolve('series/')}>Voir les séries</a></div></div><div class="intro-mark" aria-hidden="true">a<span>VOTRE MÉDIATHÈQUE</span></div></section>
    {:else}
      <div class="page-heading"><p class="eyebrow">VOTRE MÉDIATHÈQUE</p><h1>{title}</h1><p>{section === 'films' ? 'Chaque film, toutes ses versions.' : section === 'series' ? 'Vos histoires, saison après saison.' : 'Trouvez votre prochaine séance.'}</p></div>
      <form class="filters" action={`${base}/${section}/`}><label class="search-field">Rechercher {section === 'series' ? 'une série' : section === 'films' ? 'un film' : 'dans le catalogue'}<input type="search" name="q" bind:value={search} placeholder="Un titre…" maxlength="200" /></label><label>Trier<select name="sort" bind:value={sort}><option value="title-asc">Titre · A à Z</option><option value="title-desc">Titre · Z à A</option></select></label><button class="secondary" type="submit">Appliquer</button><details class="future-filters"><summary>Filtres</summary><p>Les filtres par année, genre et langue seront disponibles prochainement.</p></details></form>
    {/if}
    {#if section === 'home'}<section class="shelf"><h2>Reprendre la lecture</h2><div class="empty compact">Votre historique de lecture n’est pas encore disponible.</div></section>{/if}
    {#if section !== 'series'}<section class="shelf"><div class="section-heading"><h2>{section === 'home' ? 'Films récemment ajoutés' : section === 'search' ? 'Films' : 'Tous les films'}</h2>{#if section === 'home'}<a href={resolve('films/')}>Tout voir →</a>{:else}<span>{totalFilms} film{totalFilms > 1 ? 's' : ''}</span>{/if}</div>{#if section === 'home'}<div class="empty compact">Les ajouts récents apparaîtront ici prochainement.</div><h3 class="library-heading">Catalogue par titre</h3>{/if}<div class="media-grid">{#each films as work (work.id)}<Poster {work} href={detailUrl(work)} />{:else}<div class="empty">{query ? 'Aucun film ne correspond à votre recherche.' : 'Aucun film dans cette bibliothèque.'}</div>{/each}</div></section>{/if}
    {#if section !== 'films'}<section class="shelf"><div class="section-heading"><h2>{section === 'home' ? 'Séries récemment ajoutées' : section === 'search' ? 'Séries' : 'Toutes les séries'}</h2>{#if section === 'home'}<a href={resolve('series/')}>Tout voir →</a>{:else}<span>{totalSeries} série{totalSeries > 1 ? 's' : ''}</span>{/if}</div>{#if section === 'home'}<div class="empty compact">Les ajouts récents apparaîtront ici prochainement.</div><h3 class="library-heading">Catalogue par titre</h3>{/if}<div class="media-grid">{#each series as work (work.id)}<Poster {work} href={detailUrl(work)} />{:else}<div class="empty">{query ? 'Aucune série ne correspond à votre recherche.' : 'Aucune série dans cette bibliothèque.'}</div>{/each}</div></section>{/if}
    {#if section === 'home'}<section class="shelf"><h2>Continuer les séries en cours</h2><div class="empty compact">Vos épisodes en cours apparaîtront ici lors de vos prochaines lectures.</div></section><section class="shelf"><h2>Films à découvrir</h2><div class="empty compact">De nouvelles suggestions apparaîtront ici prochainement.</div></section>{/if}
    {#if section !== 'home' && (offset > 0 || totalFilms > offset + 24 || totalSeries > offset + 24)}<nav class="pagination" aria-label="Pages du catalogue">{#if offset > 0}<a class="secondary" href={resolve(`${section === 'films' ? 'films/' : section === 'series' ? 'series/' : 'search/'}?${new URLSearchParams({ q:query, sort:order, offset:String(Math.max(0,offset-24)) })}`)}>← Précédent</a>{/if}<span>Page {Math.floor(offset / 24) + 1}</span>{#if totalFilms > offset + 24 || totalSeries > offset + 24}<a class="secondary" href={resolve(`${section === 'films' ? 'films/' : section === 'series' ? 'series/' : 'search/'}?${new URLSearchParams({ q:query, sort:order, offset:String(offset+24) })}`)}>Suivant →</a>{/if}</nav>{/if}
  {/if}
</main>
<footer><span>{name} <span class="footer-dot">·</span> Votre cinéma, autrement.</span><p>{detail?.synthetic ? 'Fiche de démonstration · médias de test.' : catalogueNotice || 'Bibliothèques locales · fichiers en lecture seule.'}</p></footer>
{#if detail && ImmersivePlayerComponent && createPlayer}
  <ImmersivePlayerComponent bind:this={immersive} catalogue={playerMedia} {createPlayer} />
{/if}

<style>
  .skip-link { position:fixed; top:-80px; left:20px; z-index:50; background:var(--color-text); color:var(--color-canvas); padding:14px; }.skip-link:focus { top:10px; }.site-header { display:flex; align-items:center; gap:60px; padding:0 var(--page-gutter); min-height:88px; border-bottom:1px solid var(--color-line); }.brand { display:flex; gap:9px; align-items:center; }.brand span { font-size:30px; letter-spacing:-.065em; font-weight:650; }nav { display:flex; gap:30px; }nav a { display:flex; align-items:center; gap:7px; font-size:12px; color:var(--color-muted); padding:32px 0; border-bottom:2px solid transparent; }nav a[aria-current='page'] { color:var(--color-text); border-color:var(--color-accent); }nav a:hover { color:white; }.nav-symbol { display:none; }.search-icon { display:inline-flex; align-items:center; justify-content:center; }main { max-width:1600px; margin:auto; padding:0 var(--page-gutter) 70px; min-height:70vh; }h1 { font-size:clamp(34px,4.5vw,66px); line-height:1.12; letter-spacing:-.055em; font-weight:500; margin:16px 0 22px; }h2 { font-size:clamp(21px,2vw,28px); font-weight:500; letter-spacing:-.035em; margin:0; }h3 { font-size:16px; font-weight:550; }p { line-height:1.8; color:var(--color-muted); }.page-heading { padding:55px 0 26px; }.page-heading p { margin:0; }.home-intro { display:flex; justify-content:space-between; align-items:center; gap:30px; padding:70px 0 60px; border-bottom:1px solid var(--color-line); }.home-intro h1 { font-size:clamp(36px,4.5vw,66px); }.home-intro h1 em { color:#c9b8ef; font-family:Georgia,serif; font-weight:400; }.home-intro p { font-size:13px; }.intro-mark { color:#b4a1f83b; font-size:180px; letter-spacing:-.12em; padding:0 50px; font-family:Georgia,serif; }.intro-mark span { display:block; font-family:var(--font-sans); font-size:8px; color:var(--color-subtle); letter-spacing:.2em; white-space:nowrap; }.home-actions { display:flex; gap:12px; margin-top:28px; }.primary,.hero-cta,.secondary { display:inline-flex; gap:12px; align-items:center; justify-content:center; padding:13px 20px; border-radius:6px; font-size:12px; min-height:44px; border:1px solid var(--color-line); }.primary,.hero-cta { background:var(--color-text); color:#16121d; font-weight:600; }.primary:hover,.hero-cta:hover { background:white; transform:translateY(-2px); }.secondary { background:var(--color-surface); color:var(--color-text); }.secondary:hover { border-color:var(--color-accent); }.shelf { margin-top:42px; }.section-heading { display:flex; align-items:center; justify-content:space-between; gap:16px; margin-bottom:20px; }.section-heading>a,.section-heading>span { font-size:11px; color:var(--color-muted); }.library-heading { margin:24px 0 18px; color:var(--color-muted); font-size:13px; }.media-grid { display:grid; grid-template-columns:repeat(6,minmax(0,1fr)); gap:22px; }.empty { grid-column:1/-1; padding:32px; background:var(--color-surface); border:1px solid var(--color-line); border-radius:10px; color:var(--color-muted); font-size:13px; line-height:1.8; }.empty h2 { color:var(--color-text); }.empty.compact { margin-top:18px; padding:22px 26px; }.filters { display:flex; gap:18px; align-items:end; flex-wrap:wrap; padding:22px; border:1px solid var(--color-line); border-radius:10px; }label { display:grid; gap:8px; font-size:11px; color:var(--color-muted); }.search-field { flex:1; min-width:180px; }input { background:var(--color-raised); color:var(--color-text); border:1px solid var(--color-line); border-radius:6px; min-height:46px; padding:12px 14px; width:100%; }.future-filters { width:100%; font-size:12px; color:var(--color-muted); }.future-filters summary { cursor:pointer; }.future-filters p { margin-bottom:0; }.pagination { margin-top:32px; align-items:center; }.pagination span { font-size:12px; }.back-link { display:inline-block; margin-top:35px; color:var(--color-muted); font-size:12px; }.detail { display:grid; grid-template-columns:220px minmax(0,1fr); gap:60px; padding:36px 0; border-bottom:1px solid var(--color-line); }.detail-copy { align-self:center; max-width:700px; }.detail h1 { font-size:clamp(32px,4vw,54px); }.metadata { font-size:12px; }.synopsis { font-size:14px; }.detail-note { font-size:11px; max-width:520px; }.versions { display:flex; gap:8px; margin:25px 0 16px; }.version-choice { background:var(--color-surface); color:var(--color-muted); border:1px solid var(--color-line); border-radius:6px; min-height:40px; padding:8px 18px; font-size:12px; }.version-choice[aria-pressed='true'] { color:var(--color-accent); border-color:var(--color-accent); background:var(--color-accent-soft); }.episode { display:flex; gap:26px; padding:26px; background:var(--color-surface); border:1px solid var(--color-line); border-radius:10px; }.episode>div { flex:1; min-width:0; }.episode-number { color:var(--color-accent); font-size:28px; margin-top:12px; }.episode p { font-size:12px; }.error { color:var(--color-danger); }.unavailable { font-size:12px; }.settings-panel { max-width:700px; background:var(--color-surface); padding:30px; border:1px solid var(--color-line); border-radius:12px; }.settings-panel p { font-size:14px; }.settings-panel button { padding:12px; background:var(--color-raised); color:var(--color-muted); border:0; border-radius:6px; }footer { border-top:1px solid var(--color-line); padding:26px var(--page-gutter); display:flex; justify-content:space-between; align-items:center; gap:24px; font-size:12px; color:#c4becf; }footer p { margin:0; text-align:right; font-size:10px; }.footer-dot { color:var(--color-accent); padding:0 8px; }
  @media(min-width:1600px) { .media-grid { grid-template-columns:repeat(7,minmax(0,1fr)); } }@media(max-width:1100px) { .site-header { gap:32px; }nav { gap:22px; }.media-grid { grid-template-columns:repeat(4,minmax(0,1fr)); }.intro-mark { padding:0 10px; font-size:140px; } }@media(max-width:700px) { .site-header { min-height:70px; }.site-header nav { position:fixed; bottom:0; left:0; right:0; z-index:10; background:#101017; border-top:1px solid var(--color-line); justify-content:space-around; gap:0; padding:0 6px env(safe-area-inset-bottom); }.site-header nav a { padding:10px 2px; display:flex; flex-direction:column; align-items:center; gap:4px; font-size:10px; min-width:56px; border-bottom:0; border-top:2px solid transparent; }.site-header nav a[aria-current='page'] { border-top-color:var(--color-accent); color:var(--color-accent); }.nav-symbol { display:block; font-size:21px; height:24px; }.search-icon { height:24px; }.home-intro { padding:44px 0 36px; }.intro-mark { display:none; }.media-grid { grid-template-columns:repeat(2,minmax(0,1fr)); gap:20px 14px; }.home-actions { flex-wrap:wrap; }.home-intro h1 { font-size:38px; }.shelf { margin-top:32px; }.page-heading { padding-top:36px; }.filters { gap:14px; padding:16px; }.search-field { width:100%; flex-basis:100%; }.detail { grid-template-columns:1fr; gap:20px; }.detail-poster { width:140px; }.detail-copy h1 { margin-top:12px; }.episode { padding:18px; gap:16px; }footer { flex-direction:column; align-items:flex-start; padding-bottom:calc(110px + env(safe-area-inset-bottom, 0px)); }footer p { text-align:left; }.section-heading { align-items:start; }main { padding-bottom:calc(64px + env(safe-area-inset-bottom, 0px)); scroll-padding-bottom:calc(84px + env(safe-area-inset-bottom, 0px)); } }
</style>
