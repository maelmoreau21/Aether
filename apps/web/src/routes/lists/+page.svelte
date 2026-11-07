<script lang="ts">
  import {onMount} from 'svelte';
  import {resolve} from '$app/paths';
  import {page} from '$app/state';
  import Navbar from '../../lib/ui/Navbar.svelte';
  import {session} from '../../lib/auth/session';
  import {playlistRequest,type Playlist} from '../../lib/music/playlists';
  import {playMusic,type MusicTrack} from '../../lib/music/queue';
  import { t } from '../../lib/i18n';
  interface Person{id:string;name:string}
  interface Entry{kind:'work'|'music';id:string;media:{id:string;title:string;kind?:string;artist?:string;album?:string}}
  interface Detail extends Playlist{entries:Entry[];shares:Person[]}
  let lists=$state<Playlist[]>([]),selected=$state<Detail|null>(null),tab=$state('mine'),name=$state(''),error=$state(''),notice=$state(''),busy=$state(false);
  let editName=$state(''),visibility=$state('private'),shares=$state<Person[]>([]),people=$state<Person[]>([]),personQuery=$state('');
  let query=$state(''),results=$state<Entry[]>([]),policy=$state({allowPublic:false,allowSharing:false});let generation=0,searchGeneration=0;
  const pageSize=50;let entryOffset=$state(0),listOffset=$state(0);
  $effect(()=>{void tab;listOffset=0;});
  const visible=$derived(lists.filter(list=>tab==='collections'?list.visibility==='collection':tab==='mine'?list.owned:tab==='public'?list.visibility==='public'&&!list.owned:!list.owned&&list.visibility==='shared'));
  const songs=$derived(selected?.entries.filter(entry=>entry.kind==='music').map(entry=>entry.media as MusicTrack)??[]);
  async function load(){const [data,flags]=await Promise.all([playlistRequest<{lists:Playlist[]}>('/api/v1/lists'),playlistRequest<typeof policy>('/api/v1/playlists/policy')]);lists=data.lists;policy=flags;}
  async function show(id:string){if(selected?.id!==id)entryOffset=0;const current=++generation;error='';try{const detail=await playlistRequest<Detail>(`/api/v1/lists/${encodeURIComponent(id)}?compact=true`);if(current!==generation)return;selected=detail;entryOffset=Math.min(entryOffset,Math.max(0,Math.floor((detail.entries.length-1)/pageSize)*pageSize));editName=detail.name;visibility=detail.visibility;shares=detail.shares;results=[];notice='';}catch(cause){if(current===generation){selected=null;error=(cause as Error).message;}}}
  async function action(operation:()=>Promise<void>){if(busy)return;busy=true;error='';try{await operation();await load();}catch(cause){error=(cause as Error).message;}finally{busy=false;}}
  async function create(event:SubmitEvent){event.preventDefault();await action(async()=>{const item=await playlistRequest<{id:string}>('/api/v1/lists','POST',{name});name='';await show(item.id);tab='mine';});}
  async function save(event:SubmitEvent){event.preventDefault();if(!selected)return;const id=selected.id;await action(async()=>{await playlistRequest(`/api/v1/playlists/${id}/settings`,'PUT',{name:editName,visibility,userIds:shares.map(user=>user.id)});await show(id);notice=$t('playlist_saved');});}
  async function search(event:SubmitEvent){event.preventDefault();const current=++searchGeneration;error='';try{const [works,music]=await Promise.all([playlistRequest<{items:Entry['media'][]}>('/api/v1/catalog?'+new URLSearchParams({q:query,limit:'12'})),playlistRequest<{items:MusicTrack[]}>('/api/v1/music?'+new URLSearchParams({q:query,limit:'12'}))]);if(current===searchGeneration)results=[...works.items.map(media=>({kind:'work' as const,id:media.id,media})),...music.items.map(media=>({kind:'music' as const,id:media.id,media}))];}catch(cause){if(current===searchGeneration)error=(cause as Error).message;}}
  async function findPeople(event:SubmitEvent){event.preventDefault();try{people=(await playlistRequest<{users:Person[]}>('/api/v1/playlists/recipients?q='+encodeURIComponent(personQuery))).users;}catch(cause){error=(cause as Error).message;}}
  async function add(entry:Entry){if(!selected)return;const id=selected.id;await action(async()=>{await playlistRequest(`/api/v1/${entry.kind==='music'?'playlists':'lists'}/${id}/${entry.kind==='music'?'music':'items'}/${encodeURIComponent(entry.id)}`,'PUT');await show(id);});}
  async function remove(entry:Entry){if(!selected)return;const id=selected.id;await action(async()=>{await playlistRequest(`/api/v1/${entry.kind==='music'?'playlists':'lists'}/${id}/${entry.kind==='music'?'music':'items'}/${encodeURIComponent(entry.id)}`,'DELETE');await show(id);});}
  async function move(index:number,delta:number){if(!selected)return;const id=selected.id;const entries=[...selected.entries];[entries[index],entries[index+delta]]=[entries[index+delta],entries[index]];await action(async()=>{await playlistRequest(`/api/v1/playlists/${id}/order`,'PUT',entries.map(({kind,id})=>({kind,id})));await show(id);});}
  onMount(()=>{void load().then(()=>{const id=page.url.searchParams.get('id');if(id)return show(id);}).catch(cause=>error=cause.message);return()=>{generation++;searchGeneration++;};});
</script>
<svelte:head><title>{$t('playlists')}</title></svelte:head>
<Navbar section="lists" />
<main><header><div><p class="eyebrow">{$t('your_collection')}</p><h1>{$t('playlists')}</h1><p>{$t('films_series_and_music_gathered_your')}</p></div><form onsubmit={create}><label>{$t('catalog_new_playlist')}<input required maxlength="100" placeholder={$t('an_evening_a_mood')} bind:value={name} /></label><button disabled={busy}>{$t('create')}</button></form></header>
{#if error}<p role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}
<div class="workspace"><aside><nav aria-label={$t('playlists')}><button class:active={tab==='collections'} onclick={()=>tab='collections'}>{$t('collections')}</button><button class:active={tab==='mine'} onclick={()=>tab='mine'}>{$t('my_playlists')}</button><button class:active={tab==='shared'} onclick={()=>tab='shared'}>{$t('shared_with_me')}</button><button class:active={tab==='public'} onclick={()=>tab='public'}>{$t('public')}</button></nav><div class="collection">{#each visible.slice(listOffset,listOffset+pageSize) as list(list.id)}<button disabled={busy} class:chosen={selected?.id===list.id} onclick={()=>show(list.id)}><span class="tile" aria-hidden="true">♫</span><span><strong>{list.name}</strong><small>{list.visibility==='collection'?$t('automatic_collection'):list.visibility==='private'?'Privée':list.visibility==='shared'?$t('by_invitation'):$t('public_on_the_server')}</small></span></button>{:else}<p>{$t('no_playlists_in_this_category')}</p>{/each}</div>{#if visible.length>pageSize}<nav class="pagination" aria-label={$t('playlist_pagination')}><button disabled={listOffset===0} onclick={()=>listOffset=Math.max(0,listOffset-pageSize)}>{$t('catalog_previous')}</button><span>{listOffset+1}–{Math.min(listOffset+pageSize,visible.length)} / {visible.length}</span><button disabled={listOffset+pageSize>=visible.length} onclick={()=>listOffset+=pageSize}>{$t('catalog_next')}</button></nav>{/if}</aside>
<section class="detail">{#if selected}<div class="detail-heading"><div><span class="badge">{selected.owned?$t('your_playlist'):$t('read_only')}</span><h2>{selected.name}</h2><p>{$t('accessible', { count: selected.entries.length })}</p></div>{#if songs.length}<button disabled={busy} onclick={()=>playMusic(songs)}>{$t('play_the_music_tracks')}</button>{/if}</div>
{#if selected.owned}<details class="settings"><summary>{$t('name_and_sharing')}</summary><form onsubmit={save}><label>{$t('name')}<input bind:value={editName} maxlength="100" required /></label><div class="field"><label for="playlist-visibility">{$t('visibility')}</label><select id="playlist-visibility" bind:value={visibility}><option value="private">{$t('private')}</option><option value="shared" disabled={!policy.allowSharing}>{$t('shared_with_people')}</option><option value="public" disabled={!policy.allowPublic}>{$t('public_on_this_server')}</option></select></div><p>{$t('sharing_grants_no_extra_rights_on')}</p>{#if !policy.allowSharing&&!policy.allowPublic}<p>{$t('sharing_is_disabled_by_the_administrator')}</p>{/if}<button disabled={busy}>{$t('save')}</button></form>{#if visibility==='shared'&&policy.allowSharing}<form onsubmit={findPeople}><label>{$t('search_for_a_person')}<input bind:value={personQuery} minlength="2" maxlength="100" required /></label><button disabled={busy}>{$t('filter_extensions')}</button></form><div class="people">{#each shares as person(person.id)}<button onclick={()=>shares=shares.filter(item=>item.id!==person.id)}>{person.name} ×</button>{/each}{#each people.filter(person=>!shares.some(item=>item.id===person.id)) as person(person.id)}<button onclick={()=>shares=[...shares,person]}>+ {person.name}</button>{/each}</div>{/if}
{#if selected.visibility!=='private'}<label>{$t('playlist_link')}<input readonly value={new URL(resolve(`lists/?id=${selected.id}`),page.url.origin).href} onclick={event=>event.currentTarget.select()} /></label><small>{$t('sign_in_required_only_authorised_people')}</small>{/if}</details>
<form class="search" onsubmit={search}><label>{$t('add_media')}<input type="search" bind:value={query} required maxlength="256" placeholder={$t('title_artist_film_or_series')} /></label><button disabled={busy}>{$t('filter_extensions')}</button></form>{#if results.length}<div class="results">{#each results as entry (`${entry.kind}:${entry.id}`)}<div><span>{entry.media.title} <small>{entry.kind==='music'?'Musique':entry.media.kind==='series'?'Série':'Film'}</small></span><button disabled={busy||selected.entries.some(item=>item.kind===entry.kind&&item.id===entry.id)} onclick={()=>add(entry)}>{$t('sched_add')}</button></div>{/each}</div>{/if}{/if}
<ol class="entries">{#each selected.entries.slice(entryOffset,entryOffset+pageSize) as entry,pageIndex (`${entry.kind}:${entry.id}`)}{@const index=entryOffset+pageIndex}<li><span class="number">{index+1}</span><div class="entry-title"><strong>{entry.media.title}</strong><small>{entry.kind==='music'?`${entry.media.artist} · ${entry.media.album||'Album inconnu'}`:entry.media.kind==='series'?'Série':'Film'}</small></div>{#if entry.kind==='music'}<button onclick={()=>playMusic(songs,songs.findIndex(track=>track.id===entry.id))} aria-label={$t('play_title', { title: entry.media.title })}>{$t('play_action')}</button>{:else}<a href={resolve(`${entry.media.kind==='series'?'series':'films'}/?work=${encodeURIComponent(entry.id)}`)}>{$t('catalog_open')}</a>{/if}{#if selected.owned}<button disabled={busy||index===0} onclick={()=>move(index,-1)} aria-label={$t('catalog_move_up', { title: entry.media.title })}>↑</button><button disabled={busy||index===selected.entries.length-1} onclick={()=>move(index,1)} aria-label={$t('catalog_move_down', { title: entry.media.title })}>↓</button><button disabled={busy} onclick={()=>remove(entry)} aria-label={$t('catalog_remove', { title: entry.media.title })}>×</button>{/if}</li>{:else}<p>{$t('this_playlist_is_empty_or_its')}</p>{/each}</ol>
{#if selected.entries.length>pageSize}<nav class="pagination" aria-label={$t('item_pagination')}><button disabled={busy||entryOffset===0} onclick={()=>entryOffset=Math.max(0,entryOffset-pageSize)}>{$t('previous_page')}</button><span role="status">{$t('range_of_total', { from: entryOffset+1, to: Math.min(entryOffset+pageSize,selected.entries.length), total: selected.entries.length })}</span><button disabled={busy||entryOffset+pageSize>=selected.entries.length} onclick={()=>entryOffset+=pageSize}>{$t('next_page')}</button></nav>{/if}
{#if selected.owned}<details class="delete"><summary>{$t('delete_the_playlist')}</summary><p>{$t('the_media_will_be_kept')}</p><button disabled={busy} onclick={()=>action(async()=>{await playlistRequest(`/api/v1/lists/${selected!.id}`,'DELETE');selected=null;})}>{$t('confirm_deletion')}</button></details>{/if}
{:else}<div class="empty"><span aria-hidden="true">♫</span><h2>{$t('a_collection_that_looks_like_you')}</h2><p>{$t('create_a_playlist_or_choose_one')}</p><a href={resolve('music/')}>{$t('explore_music')}</a></div>{/if}</section></div>
{#if $session?.user.role==='administrator'}<details class="admin-policy"><summary>{$t('playlist_sharing_administration')}</summary><p>{$t('these_options_apply_to_every_account')}</p><label><input type="checkbox" bind:checked={policy.allowSharing} /> {$t('allow_sharing_between_users')}</label><label><input type="checkbox" bind:checked={policy.allowPublic} /> {$t('allow_public_playlists_on_the_server')}</label><button disabled={busy} onclick={()=>action(async()=>{await playlistRequest('/api/v1/admin/playlists/policy','PUT',policy);notice=$t('sharing_permissions_saved');})}>{$t('catalog_save_permissions')}</button></details>{/if}</main>
<style>
  main { width: 100%; max-width: var(--maxw); margin: 0 auto; padding: 20px var(--gutter) 80px; min-width: 0; color: var(--tx); }
  header, .detail-heading { display: flex; justify-content: space-between; gap: 24px; align-items: flex-end; flex-wrap: wrap; }
  h1 { margin: 4px 0 0; font-size: clamp(32px, 4vw, 44px); font-weight: 800; letter-spacing: -0.03em; line-height: 1.02; overflow-wrap: anywhere; }
  h2 { margin: 8px 0 4px; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; overflow-wrap: anywhere; }
  .eyebrow { margin: 0; font-size: 12px; font-weight: 700; letter-spacing: 0.07em; text-transform: uppercase; color: var(--mu); }
  p, small { color: var(--mu); }
  p { margin: 8px 0 0; font-size: 14px; line-height: 1.6; }
  small { font-size: 13px; }
  .field { display: grid; gap: 8px; }
  label { display: flex; flex-direction: column; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  form { display: flex; gap: 8px; align-items: end; flex-wrap: wrap; }
  input, select { max-width: 100%; box-sizing: border-box; height: 46px; min-height: 46px; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background-color: var(--pn); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  select { padding-right: 40px; }
  input:hover, select:hover { background-color: var(--pn-2); }
  input:focus, select:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; max-width: 100%; box-sizing: border-box; min-height: 46px; padding: 0 18px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: default; }
  button:focus-visible, a:focus-visible, summary:focus-visible, input:focus-visible, select:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  header form button, .detail-heading > button, .settings form:first-of-type > button { background: var(--btn); color: var(--btn-tx); }
  header form button:hover:not(:disabled), .detail-heading > button:hover:not(:disabled), .settings form:first-of-type > button:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  header form label { min-width: min(320px, 100%); }
  a { color: var(--tx); font-weight: 700; }
  [role='alert'] { margin: 20px 0 0; padding: 12px 14px; border-radius: var(--r-sm); background: var(--err-soft); color: var(--err); font-weight: 600; }
  main > [role='status'] { margin: 20px 0 0; padding: 12px 14px; border-radius: var(--r-sm); background: var(--ok-soft); color: var(--ok); font-weight: 600; }

  .workspace > aside, .detail { min-width: 0; }
  .workspace { display: grid; grid-template-columns: 280px minmax(0, 1fr); gap: 32px; align-items: start; margin-top: 32px; }
  aside nav { display: grid; gap: 2px; }
  aside nav button { justify-content: flex-start; min-height: 42px; padding: 0 12px; border-radius: 12px; background: transparent; color: var(--mu); font-weight: 600; text-align: left; }
  aside nav button:hover:not(:disabled) { background: var(--pn); color: var(--tx); }
  aside nav button.active, aside nav button.active:hover:not(:disabled) { background: var(--pn-2); color: var(--tx); }
  .collection { display: grid; gap: 4px; margin-top: 20px; padding-top: 16px; border-top: 1px solid var(--line); }
  .collection button { justify-content: flex-start; gap: 12px; min-height: 60px; padding: 8px 10px; border-radius: var(--r-md); background: transparent; font-weight: 600; text-align: left; }
  .collection button:hover:not(:disabled) { background: var(--pn); }
  .collection button > span:last-child { min-width: 0; overflow-wrap: anywhere; }
  .collection strong { font-size: 15px; font-weight: 700; }
  .collection small, .entry-title small { display: block; margin-top: 2px; font-weight: 500; }
  .collection button.chosen, .collection button.chosen:hover:not(:disabled) { background: var(--ac-soft); }
  .tile { display: grid; place-items: center; flex: none; width: 44px; height: 44px; border-radius: 14px; background: var(--pn-2); color: var(--mu); font-size: 18px; }
  .chosen .tile { color: var(--ac); }
  .collection p { margin: 0; padding: 8px 10px; }

  .detail { min-height: 420px; padding: 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  .detail-heading { align-items: center; }
  .detail-heading p { margin-top: 0; }
  .badge { display: inline-flex; align-items: center; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--pn-2); color: var(--mu); font-size: 12px; font-weight: 700; }
  .settings { margin-top: 20px; padding: 4px 0 20px; border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); }
  summary { display: flex; align-items: center; min-height: 48px; font-size: 15px; font-weight: 700; cursor: pointer; border-radius: var(--r-xs); }
  .settings form { margin-top: 12px; }
  .settings p { flex-basis: 100%; margin: 0; font-size: 13px; }
  .settings > label { margin-top: 16px; }
  .settings > label input { font-family: var(--font-mono); font-size: 13px; }
  .settings > small { display: block; margin-top: 8px; }
  .people { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 12px; }
  .people button { min-height: 38px; padding: 0 14px; border-radius: var(--r-pill); background: var(--pn); font-size: 14px; font-weight: 600; }
  .people button:hover:not(:disabled) { background: var(--pn-2); }
  .search { margin-top: 20px; }
  .search label { flex: 1; min-width: min(240px, 100%); }
  .results { max-height: 260px; overflow: auto; margin: 16px 0; border-radius: var(--r-md); background: var(--pn); overscroll-behavior: contain; }
  .results > div { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 56px; padding: 6px 8px 6px 16px; border-bottom: 1px solid var(--line); }
  .results > div:last-child { border-bottom: 0; }
  .results > div > span { min-width: 0; font-size: 15px; font-weight: 600; overflow-wrap: anywhere; }
  .results small { display: block; margin-top: 2px; font-weight: 500; }
  .results button { min-height: 40px; background: transparent; }
  .results button:hover:not(:disabled) { background: var(--pn-2); }
  .entries { margin: 20px 0 0; padding: 0; list-style: none; }
  .entries li { display: flex; align-items: center; gap: 8px; min-height: 60px; padding: 8px; border-top: 1px solid var(--line); transition: background-color var(--t-fast) ease; }
  .entries li:first-child { border-top: 0; }
  .entries li:hover { background: var(--pn); }
  .number { flex: none; width: 28px; font-size: 13px; font-weight: 600; color: var(--mu); font-variant-numeric: tabular-nums; text-align: center; }
  .entry-title { flex: 1; min-width: 0; }
  .entry-title strong { font-size: 15px; font-weight: 700; overflow-wrap: anywhere; }
  .entries a { display: inline-flex; align-items: center; min-height: 44px; padding: 0 14px; border-radius: 14px; font-size: 14px; }
  .entries a:hover { background: var(--pn-2); }
  .entries button { min-width: 44px; min-height: 44px; padding: 0 12px; border-radius: 14px; background: transparent; color: var(--mu); }
  .entries button:hover:not(:disabled) { background: var(--pn-2); color: var(--tx); }
  .entries > p { margin: 0; padding: 24px 8px; }
  .pagination { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; margin-top: 16px; font-size: 14px; }
  .pagination span { font-weight: 600; color: var(--mu); font-variant-numeric: tabular-nums; }
  .delete { margin-top: 28px; padding-top: 4px; border-top: 1px solid var(--line); font-size: 14px; }
  .delete summary { color: var(--err); }
  .delete p { margin: 0 0 12px; }
  .delete button { background: var(--err-soft); color: var(--err); }
  .delete button:hover:not(:disabled) { background: var(--err-soft); opacity: 0.85; }
  .empty { display: flex; flex-direction: column; align-items: center; max-width: 65ch; margin: 0 auto; padding: 56px 24px; text-align: center; }
  .empty > span { display: grid; place-items: center; width: 64px; height: 64px; margin-bottom: 8px; border-radius: var(--r-md); background: var(--pn-2); color: var(--mu); font-size: 28px; }
  .empty a { display: inline-flex; align-items: center; height: 48px; margin-top: 20px; padding: 0 22px; border-radius: var(--r-md); background: var(--pn-2); transition: background-color var(--t-fast) ease; }
  .empty a:hover { background: var(--pn-3); }
  .admin-policy { margin-top: 28px; padding: 8px 24px 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  .admin-policy p { margin: 0 0 8px; }
  .admin-policy label { display: flex; flex-direction: row; align-items: center; gap: 12px; min-height: 44px; margin: 4px 0; font-weight: 600; }
  .admin-policy > button { margin-top: 12px; background: var(--btn); color: var(--btn-tx); }
  .admin-policy > button:hover:not(:disabled) { background: var(--btn); opacity: 0.9; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; min-height: 20px; margin: 0; padding: 0; accent-color: var(--ac); }

  @media (max-width: 1023px) {
    .workspace { grid-template-columns: minmax(0, 1fr); gap: 20px; }
    aside nav { display: flex; gap: 6px; overflow-x: auto; scrollbar-width: none; padding-bottom: 4px; }
    aside nav::-webkit-scrollbar { display: none; }
    aside nav button { flex: none; min-height: 40px; white-space: nowrap; background: var(--pn); }
    .collection { display: flex; overflow-x: auto; max-width: 100%; margin-top: 12px; padding-top: 12px; scrollbar-width: none; }
    .collection::-webkit-scrollbar { display: none; }
    .collection button { flex: 0 0 220px; min-width: 0; background: var(--pn); }
  }
  @media (max-width: 640px) {
    main { padding-top: 12px; padding-bottom: calc(92px + env(safe-area-inset-bottom, 0px)); }
    header { align-items: stretch; flex-direction: column; gap: 16px; }
    header form label { flex: 1; min-width: 0; }
    .detail { padding: 16px; border-radius: var(--r-md); }
    .detail-heading { align-items: flex-start; flex-direction: column; gap: 12px; }
    .entries li { flex-wrap: wrap; }
    .admin-policy { padding: 4px 16px 16px; border-radius: var(--r-md); }
  }
</style>
