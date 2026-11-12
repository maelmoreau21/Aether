<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { adminRequest } from './request';
  import type { CoreHealth, AdminNode, Library, Scan } from './types';
  import type { Work } from '../catalog/api';
  import { selectArtwork } from '../catalog/artwork-selection';
  import { session } from '../auth/session';
  import ClientIcon from '../ui/ClientIcon.svelte';
  import Icon from '../ui/Icon.svelte';
  import { clientInfo, clock } from '../ui/client-info';
  import { t, fmt } from '../i18n';
  let {coreHealth,nodes,libraries,scans,nodesError='',active=true}:{coreHealth:CoreHealth|null;nodes:AdminNode[];libraries:Library[];scans:Record<string,Scan>;nodesError?:string;active?:boolean}=$props();
  type Activity={userId:string;name:string;device:string;lastSeenAt:number;playbackId:string|null;title:string|null;workId?:string|null;workKind?:string|null;year?:number|null;seriesId?:string|null;seriesTitle?:string|null;seasonNumber?:number|null;episodeNumber?:number|null;position:number|null;duration:number|null;paused:boolean|null};
  /** Backgrounds in fallback order (episode, then series); the card moves on when one fails. */
  type Art={backgrounds:string[];logo:string|null};
  let sessions=$state<Activity[]>([]),activityError=$state(''),updated=$state(''),loading=$state(false);
  let artwork=$state<Record<string,Art>>({});
  let failed=$state<Record<string,number>>({});
  let commanding=$state<Record<string,boolean>>({});
  async function controlPlayback(playbackId:string,action:'pause'|'play'|'stop'){
    if(commanding[playbackId])return;
    commanding={...commanding,[playbackId]:true};
    try{
      await adminRequest(`/api/v1/admin/activity/${encodeURIComponent(playbackId)}/command`,{method:'POST',body:JSON.stringify({action})});
      if(action==='stop'){sessions=sessions.filter(item=>item.playbackId!==playbackId);}
      else{sessions=sessions.map(item=>item.playbackId===playbackId?{...item,paused:action==='pause'}:item);}
    }catch{
      activityError=$t('activity_unavailable_try_again');
    }finally{
      const next={...commanding};delete next[playbackId];commanding=next;
    }
  }
  const requested=new SvelteSet<string>();
  const connected=$derived(nodes.filter(node=>node.status==='connected').length);
  const offline=$derived(nodes.filter(node=>node.status==='disconnected').length);
  const running=$derived(Object.values(scans).filter(scan=>scan.status==='queued'||scan.status==='running').length);
  const issues=$derived(libraries.flatMap(library=>{const scan=scans[library.id];return scan&&(scan.errorsCount??scan.errors?.length??0)>0?[{library,scan}]:[];}));
  const playing=$derived(sessions.filter(item=>item.playbackId));
  // Playback first, then the most recent browsing sessions.
  const ordered=$derived([...sessions].sort((a,b)=>Number(!!b.playbackId)-Number(!!a.playbackId)||b.lastSeenAt-a.lastSeenAt));
  async function refresh(){if(!active||loading)return;loading=true;try{sessions=(await adminRequest<{sessions:Activity[]}>('/api/v1/admin/activity')).sessions;activityError='';updated=$fmt.time(new Date());for(const item of sessions)if(item.workId)void loadArt(item);}catch{activityError=$t('activity_unavailable_try_again');sessions=[];}finally{loading=false;}}
  const keyOf=(item:Activity)=>item.workId?`${item.workId}:${item.seriesId??''}`:'';
  async function images(id:string|null|undefined,kinds:string[]):Promise<{urls:string[];logo:string|null}>{
    if(!id)return{urls:[],logo:null};
    try{const chosen=selectArtwork(await adminRequest<Work>(`/api/v1/catalog/${encodeURIComponent(id)}?episodeLimit=1`),$session);return{urls:kinds.map(kind=>chosen[kind]).filter((url):url is string=>!!url),logo:chosen.logo??null};}
    catch{return{urls:[],logo:null};}
  }
  async function loadArt(item:Activity){
    const key=keyOf(item);
    if(!key||requested.has(key))return;requested.add(key);
    // Episode still first, then the series pictures, then a neutral background.
    const [own,series]=await Promise.all([images(item.workId,['thumb','landscape','backdrop']),images(item.seriesId,['backdrop','landscape','thumb'])]);
    artwork={...artwork,[key]:{backgrounds:[...new Set([...own.urls,...series.urls])],logo:series.logo??own.logo}};
  }
  const background=(item:Activity)=>{const art=artwork[keyOf(item)];return art?.backgrounds[failed[keyOf(item)]??0]??null;};
  const nextBackground=(item:Activity)=>{const key=keyOf(item);failed={...failed,[key]:(failed[key]??0)+1};};
  const heading=(item:Activity)=>item.seriesTitle?`${item.seriesTitle}`:item.title??'Œuvre indisponible';
  const episodeLine=(item:Activity)=>item.seriesTitle?`${item.seasonNumber!=null&&item.episodeNumber!=null?`S${item.seasonNumber} · É${item.episodeNumber} — `:''}${item.title??''}`:'';
  $effect(()=>{if(active)untrack(()=>void refresh());});
  onMount(()=>{const timer=setInterval(()=>void refresh(),10000);return()=>clearInterval(timer);});
  const percent=(item:Activity)=>item.duration?Math.min(100,Math.max(0,((item.position??0)/item.duration)*100)):0;
  const ago=(seconds:number)=>{const delta=Math.max(0,Math.floor(Date.now()/1000-seconds));return delta<60?$t('sched_ago_now'):`il y a ${Math.floor(delta/60)} min`;};
</script>

<section id="overview" aria-labelledby="overview-heading" class="overview">
  <header class="page-head">
    <div><h2 id="overview-heading">{$t('admin_nav_overview')}</h2><p>{$t('the_essentials_of_your_server')}</p></div>
    <button class="ghost" disabled={loading} onclick={()=>void refresh()}>{$t('refresh')}</button>
  </header>

  <div class="stats">
    <div class="stat"><span class="dot" class:ok={coreHealth?.status==='ok'} class:warn={coreHealth && coreHealth.status!=='ok'}></span><div><small>{$t('admin_group_server')}</small><strong>{coreHealth?.status==='ok'?'Actif':coreHealth?$t('to_check'):'—'}</strong></div></div>
    <div class="stat"><span class="dot" class:ok={!nodesError&&offline===0&&nodes.length>0} class:warn={offline>0}></span><div><small>{$t('nodes')}</small><strong>{nodesError?'—':`${connected} / ${nodes.length}`}</strong></div></div>
    <div class="stat"><span class="dot" class:ok={playing.length>0}></span><div><small>{$t('stats_kpi_plays')}</small><strong>{activityError||!updated?'—':playing.length}</strong></div></div>
    <div class="stat"><span class="dot" class:ok={running>0}></span><div><small>{$t('admin_indexing')}</small><strong>{running}</strong></div></div>
  </div>

  {#if issues.length}
    <div class="callout" role="status">
      <strong>{$t('with_files_rejected_in_the_last', { count: issues.length })}</strong>
      <a href="#libraries-section">{$t('view_libraries')}</a>
    </div>
  {/if}
  {#if offline}
    <div class="callout" role="status"><strong>{$t('offline', { offline })}</strong><a href="#nodes-section">{$t('admin_manage_nodes')}</a></div>
  {/if}

  <section class="block" aria-labelledby="activity-heading">
    <div class="block-head"><h3 id="activity-heading">{$t('right_now')}</h3>{#if updated&&!activityError}<small>{$t('updated_at', { updated })}</small>{/if}</div>
    {#if activityError}<p class="empty" role="alert">{activityError}</p>
    {:else if sessions.length}
      <ul class="session-grid">
        {#each ordered as item,index (index)}
          {@const client=clientInfo(item.device)}
          {@const art=artwork[keyOf(item)]}
          {@const bg=background(item)}
          <li class="session" class:watching={!!item.playbackId}>
            <div class="session-media">
              {#if bg}<img class="backdrop" src={bg} alt="" onerror={()=>nextBackground(item)} />{/if}
              <div class="scrim"></div>
              <div class="client">
                <span class="client-icon"><ClientIcon kind={client.kind} size={26} /></span>
                <span class="client-text"><strong>{client.system||'Appareil'}</strong><span>{client.app}</span></span>
              </div>
              {#if item.playbackId}
                <div class="now">
                  <div class="title-block">
                    {#if art?.logo}<img class="logo" src={art.logo} alt={heading(item)} />{:else}<strong class="title">{heading(item)}</strong>{/if}
                    {#if episodeLine(item)}<span class="episode">{episodeLine(item)}</span>{/if}
                    {#if item.year}<span>{item.year}</span>{/if}
                  </div>
                  <span class="time">{clock(item.position)} / {clock(item.duration)}</span>
                </div>
              {:else}
                <div class="now browsing"><Icon name="globe" size={16} /><span>{$t('browsing', { value: ago(item.lastSeenAt) })}</span></div>
              {/if}
            </div>
            {#if item.playbackId}<div class="progress" role="progressbar" aria-label={$t('progress_of', { name: item.name })} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(percent(item))}><span style:width={`${percent(item)}%`}></span></div>{/if}
            <div class="session-foot">
              <span class="state" title={item.playbackId?(item.paused?$t('paused'):$t('playing')):'Navigation'}>
                <Icon name={item.playbackId?(item.paused?'pause':'play'):'eye'} size={15} />
              </span>
              <span class="avatar" aria-hidden="true">{item.name.slice(0,1).toLocaleUpperCase()}</span>
              <strong class="user">{item.name}</strong>
              {#if item.playbackId}
                {@const pid = item.playbackId}
                <div class="session-actions">
                  <button
                    type="button"
                    class="session-btn"
                    disabled={!!commanding[pid]}
                    title={item.paused ? $t('play') : $t('pause')}
                    aria-label={item.paused ? $t('play') : $t('pause')}
                    onclick={() => void controlPlayback(pid, item.paused ? 'play' : 'pause')}
                  >
                    <Icon name={item.paused ? 'play' : 'pause'} size={14} />
                  </button>
                  <button
                    type="button"
                    class="session-btn danger"
                    disabled={!!commanding[pid]}
                    title={$t('stop')}
                    aria-label={$t('stop')}
                    onclick={() => void controlPlayback(pid, 'stop')}
                  >
                    <Icon name="close" size={14} />
                  </button>
                </div>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {:else}<p class="empty">{loading?'Chargement…':$t('no_recently_active_users')}</p>{/if}
  </section>

  <section class="block technical" aria-labelledby="technical-heading">
    <div class="block-head"><h3 id="technical-heading">{$t('technical_information')}</h3></div>
    <div class="tech-grid">
      <div class="tech"><Icon name="server" size={20} /><div><small>{$t('server_version')}</small><strong>{coreHealth?.version??$t('not_reported')}</strong></div></div>
      <div class="tech"><Icon name="shield" size={20} /><div><small>{$t('mode')}</small><strong>{coreHealth?.mode??$t('not_communicated')}</strong></div></div>
      <div class="tech"><Icon name="tv" size={20} /><div><small>{$t('nodes')}</small><strong>{nodesError?'Indisponible':nodes.length?$t('connected_out_of', { connected, count: nodes.length }):$t('admin_no_nodes')}</strong></div></div>
    </div>
    {#if nodes.length}
      <ul class="node-list">
        {#each nodes as node (node.id)}
          <li>
            <span class="dot" class:ok={node.status==='connected'} class:warn={node.status==='disconnected'}></span>
            <strong>{node.name}</strong>
            <span class="pill">{node.kind==='local'?'Local':'Distant'}</span>
            <span class="node-version">{node.version||$t('admin_version_not_reported')}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</section>

<style>
  .overview { display: grid; gap: 24px; min-width: 0; font-family: var(--font); color: var(--tx); }
  .page-head { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px 16px; flex-wrap: wrap; }
  h2 { margin: 0; font-size: 24px; font-weight: 700; letter-spacing: -0.02em; line-height: 1.15; }
  .page-head p { margin: 4px 0 0; font-size: 14px; color: var(--mu); }
  h3 { margin: 0; font-size: 16px; font-weight: 600; }
  small { font-size: 12px; color: var(--mu); }
  .stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px; }
  .stat { display: flex; align-items: center; gap: 14px; min-width: 0; padding: 18px 20px; background: var(--pn); border-radius: var(--r-md); }
  .stat small { display: block; margin-bottom: 2px; }
  .stat strong { font-size: 22px; font-weight: 700; font-variant-numeric: tabular-nums; }
  .dot { flex: none; width: 10px; height: 10px; border-radius: 50%; background: var(--line-2); }
  .dot.ok { background: var(--ok); } .dot.warn { background: var(--warn); }
  .callout { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; padding: 14px 18px; border-radius: var(--r-md); background: var(--warn-soft); font-size: 14px; }
  .callout a { color: var(--ac); font-weight: 600; text-decoration: none; }
  .block { padding: 20px; background: var(--pn); border-radius: var(--r-md); min-width: 0; }
  .block-head { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; margin-bottom: 14px; }
  .empty { margin: 0; font-size: 14px; color: var(--mu); } .empty[role='alert'] { color: var(--err); }
  .ghost { min-height: 38px; padding: 8px 16px; background: transparent; color: var(--tx); border: 1px solid var(--line-2); border-radius: var(--r-md); font: inherit; font-size: 13px; cursor: pointer; }
  .ghost:hover { background: var(--pn-2); } .ghost:disabled { opacity: 0.5; cursor: not-allowed; }
  .ghost:focus-visible, .callout a:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }

  /* Session cards: image of what is playing, client, title, progress and user. */
  .session-grid { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 300px), 1fr)); gap: 14px; }
  .session { display: flex; flex-direction: column; overflow: hidden; min-width: 0; border-radius: var(--r-md); background: var(--pn-2); border: 1px solid var(--line); }
  .session-media { position: relative; display: flex; flex-direction: column; justify-content: space-between; gap: 16px; aspect-ratio: 16 / 8; min-height: 140px; padding: 14px; color: #fff; background: linear-gradient(135deg, rgb(40 46 62), rgb(18 20 28)) center / cover no-repeat; }
  .session:not(.watching) .session-media { aspect-ratio: auto; min-height: 112px; }
  .backdrop { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .episode { font-size: 12px; font-weight: 500; opacity: 0.9; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  .scrim { position: absolute; inset: 0; background: linear-gradient(to bottom, rgb(0 0 0 / 0.55), rgb(0 0 0 / 0.1) 45%, rgb(0 0 0 / 0.75)); pointer-events: none; }
  .client, .now { position: relative; display: flex; gap: 10px; min-width: 0; }
  .client { align-items: center; }
  .client-icon { display: grid; place-items: center; width: 40px; height: 40px; flex: none; border-radius: 10px; background: rgb(255 255 255 / 0.14); border: 1px solid rgb(255 255 255 / 0.16); backdrop-filter: blur(8px); -webkit-backdrop-filter: blur(8px); }
  .client-text { display: grid; min-width: 0; line-height: 1.25; text-shadow: 0 1px 2px rgb(0 0 0 / 0.5); }
  .client-text strong { font-size: 14px; }
  .client-text span { font-size: 13px; opacity: 0.85; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .now { align-items: flex-end; justify-content: space-between; text-shadow: 0 1px 3px rgb(0 0 0 / 0.6); }
  .title-block { display: grid; gap: 4px; min-width: 0; justify-items: start; }
  .title { font-size: 15px; line-height: 1.25; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .logo { display: block; max-width: 140px; max-height: 40px; object-fit: contain; object-position: left bottom; filter: drop-shadow(0 1px 3px rgb(0 0 0 / 0.6)); }
  .title-block span, .time { font-size: 13px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .time { flex: none; }
  .browsing { align-items: center; justify-content: flex-start; gap: 6px; font-size: 13px; opacity: 0.9; }
  .progress { height: 4px; background: rgb(255 255 255 / 0.08); }
  .progress span { display: block; height: 100%; background: var(--ac); }
  .session-foot { display: flex; align-items: center; gap: 10px; padding: 10px 14px; min-width: 0; }
  .state { display: grid; place-items: center; width: 28px; height: 28px; flex: none; border-radius: 50%; background: var(--pn); color: var(--mu); }
  .watching .state { color: var(--ac); background: var(--ac-soft); }
  .avatar { display: grid; place-items: center; flex: none; width: 28px; height: 28px; border-radius: 50%; background: var(--ac-soft); color: var(--ac); font-size: 13px; font-weight: 700; }
  .user { min-width: 0; font-size: 14px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .session-actions { display: inline-flex; align-items: center; gap: 6px; margin-left: auto; flex: none; }
  .session-btn { display: grid; place-items: center; width: 30px; height: 30px; padding: 0; border: 1px solid var(--line-2); border-radius: 50%; background: var(--pn); color: var(--tx); cursor: pointer; transition: background-color var(--t-fast) ease, border-color var(--t-fast) ease, color var(--t-fast) ease; }
  .session-btn:hover:not(:disabled) { background: var(--pn-3); border-color: var(--ac); color: #fff; }
  .session-btn.danger:hover:not(:disabled) { border-color: var(--err); color: var(--err); }
  .session-btn:disabled { opacity: 0.45; cursor: not-allowed; }
  .session-btn:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }

  /* Technical information: always visible. */
  .tech-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 10px; }
  .tech { display: flex; align-items: center; gap: 12px; min-width: 0; padding: 14px 16px; border-radius: var(--r-sm); background: var(--pn-2); color: var(--mu); }
  .tech div { display: grid; gap: 2px; min-width: 0; }
  .tech strong { color: var(--tx); font-size: 14px; overflow-wrap: anywhere; }
  .node-list { list-style: none; margin: 12px 0 0; padding: 0; display: grid; gap: 2px; }
  .node-list li { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; padding: 10px 4px; border-top: 1px solid var(--line); font-size: 13px; }
  .node-list li:first-child { border-top: 0; }
  .node-list strong { font-size: 14px; }
  .pill { padding: 2px 8px; border-radius: var(--r-pill); background: var(--pn-2); color: var(--mu); font-size: 11px; font-weight: 700; }
  .node-version { margin-left: auto; color: var(--mu); font-variant-numeric: tabular-nums; }
  @media (max-width: 520px) { .stats { grid-template-columns: repeat(2, minmax(0, 1fr)); } .node-version { margin-left: 0; width: 100%; } }
</style>
