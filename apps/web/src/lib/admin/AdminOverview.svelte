<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { adminRequest } from './request';
  import type { CoreHealth, AdminNode, Library, Scan } from './types';
  let {coreHealth,nodes,libraries,scans,nodesError='',active=true}:{coreHealth:CoreHealth|null;nodes:AdminNode[];libraries:Library[];scans:Record<string,Scan>;nodesError?:string;active?:boolean}=$props();
  type Activity={userId:string;name:string;device:string;lastSeenAt:number;playbackId:string|null;title:string|null;position:number|null;duration:number|null;paused:boolean|null};
  let sessions=$state<Activity[]>([]),activityError=$state(''),updated=$state(''),loading=$state(false);
  const connected=$derived(nodes.filter(node=>node.status==='connected').length);
  const offline=$derived(nodes.filter(node=>node.status==='disconnected').length);
  const running=$derived(Object.values(scans).filter(scan=>scan.status==='queued'||scan.status==='running').length);
  const issues=$derived(libraries.flatMap(library=>{const scan=scans[library.id];return scan&&(scan.errorsCount??scan.errors?.length??0)>0?[{library,scan}]:[];}));
  const playing=$derived(sessions.filter(item=>item.playbackId));
  const versions=$derived([...new Set(nodes.map(node=>node.version||'Non communiquée'))]);
  async function refresh(){if(!active||loading)return;loading=true;try{sessions=(await adminRequest<{sessions:Activity[]}>('/api/v1/admin/activity')).sessions;activityError='';updated=new Date().toLocaleTimeString();}catch{activityError='Activité indisponible. Réessayez.';sessions=[];}finally{loading=false;}}
  $effect(()=>{if(active)untrack(()=>void refresh());});
  onMount(()=>{const timer=setInterval(()=>void refresh(),10000);return()=>clearInterval(timer);});
  function deviceName(device:string){
    if(!device)return 'Appareil non communiqué';
    if(!device.startsWith('Mozilla/'))return device.slice(0,100);
    const browser=/Edg\//.test(device)?'Edge':/Firefox\//.test(device)?'Firefox':/Chrome\//.test(device)?'Chrome':/Safari\//.test(device)?'Safari':'Navigateur';
    const system=/Android/.test(device)?'Android':/iPhone|iPad/.test(device)?'iOS':/Windows/.test(device)?'Windows':/Mac OS/.test(device)?'macOS':/Linux/.test(device)?'Linux':'';
    return [browser,system].filter(Boolean).join(' · ');
  }
  const time=(seconds:number|null)=>{const value=Math.floor(seconds??0);return `${Math.floor(value/60)}:${String(value%60).padStart(2,'0')}`;};
</script>

<section id="overview" aria-labelledby="overview-heading" class="overview">
  <header class="page-head">
    <div><h2 id="overview-heading">Vue d’ensemble</h2><p>L’essentiel de votre serveur.</p></div>
    <button class="ghost" disabled={loading} onclick={()=>void refresh()}>Actualiser</button>
  </header>

  <div class="stats">
    <div class="stat"><span class="dot" class:ok={coreHealth?.status==='ok'} class:warn={coreHealth && coreHealth.status!=='ok'}></span><div><small>Serveur</small><strong>{coreHealth?.status==='ok'?'Actif':coreHealth?'À vérifier':'—'}</strong></div></div>
    <div class="stat"><span class="dot" class:ok={!nodesError&&offline===0&&nodes.length>0} class:warn={offline>0}></span><div><small>Nœuds</small><strong>{nodesError?'—':`${connected} / ${nodes.length}`}</strong></div></div>
    <div class="stat"><span class="dot" class:ok={playing.length>0}></span><div><small>Lectures</small><strong>{activityError||!updated?'—':playing.length}</strong></div></div>
    <div class="stat"><span class="dot" class:ok={running>0}></span><div><small>Indexations</small><strong>{running}</strong></div></div>
  </div>

  {#if issues.length}
    <div class="callout" role="status">
      <strong>{issues.length} bibliothèque(s) avec des fichiers refusés au dernier scan.</strong>
      <a href="#libraries-section">Voir les bibliothèques</a>
    </div>
  {/if}
  {#if offline}
    <div class="callout" role="status"><strong>{offline} nœud(s) hors ligne.</strong><a href="#nodes-section">Gérer les nœuds</a></div>
  {/if}

  <section class="block" aria-labelledby="activity-heading">
    <div class="block-head"><h3 id="activity-heading">En ce moment</h3>{#if updated&&!activityError}<small>Actualisé à {updated}</small>{/if}</div>
    {#if activityError}<p class="empty" role="alert">{activityError}</p>
    {:else if sessions.length}
      <ul class="rows">
        {#each sessions as item,index (index)}
          <li class="row">
            <span class="avatar" aria-hidden="true">{item.name.slice(0,1).toLocaleUpperCase()}</span>
            <div class="row-main">
              <strong>{item.name}</strong>
              <span class="sub">{item.playbackId?(item.title??'Œuvre indisponible'):'Navigation'} · {deviceName(item.device)}</span>
              {#if item.playbackId}<progress aria-label={`Progression de ${item.name}`} value={item.position??0} max={item.duration||1}></progress>{/if}
            </div>
            {#if item.playbackId}<span class="meta">{item.paused?'Pause':'Lecture'} · {time(item.position)}</span>{/if}
          </li>
        {/each}
      </ul>
    {:else}<p class="empty">{loading?'Chargement…':'Aucun utilisateur actif récemment.'}</p>{/if}
  </section>

  <details class="technical"><summary>Informations techniques</summary>
    <dl>
      <div><dt>Version serveur</dt><dd>{coreHealth?.version??'Non communiquée'}</dd></div>
      <div><dt>Mode</dt><dd>{coreHealth?.mode??'Non communiqué'}</dd></div>
      <div><dt>Versions des nœuds</dt><dd>{versions.join(', ')||'Aucun nœud'}</dd></div>
    </dl>
  </details>
</section>

<style>
  .overview { display: grid; gap: 24px; min-width: 0; font-family: var(--font); color: var(--tx); }
  .page-head { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px 16px; flex-wrap: wrap; }
  h2 { margin: 0; font-size: 24px; font-weight: 700; letter-spacing: -0.02em; line-height: 1.15; }
  .page-head p { margin: 4px 0 0; font-size: 14px; color: var(--mu); }
  h3 { margin: 0; font-size: 16px; font-weight: 600; }
  small { font-size: 12px; color: var(--mu); }
  .stats { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; }
  .stat { display: flex; align-items: center; gap: 14px; min-width: 0; padding: 18px 20px; background: var(--pn); border-radius: var(--r-md); }
  .stat small { display: block; margin-bottom: 2px; }
  .stat strong { font-size: 22px; font-weight: 700; font-variant-numeric: tabular-nums; }
  .dot { flex: none; width: 10px; height: 10px; border-radius: 50%; background: var(--line-2); }
  .dot.ok { background: var(--ok); } .dot.warn { background: var(--warn); }
  .callout { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; padding: 14px 18px; border-radius: var(--r-md); background: var(--warn-soft); font-size: 14px; }
  .callout a, .technical summary { color: var(--ac); font-weight: 600; text-decoration: none; }
  .block { padding: 20px; background: var(--pn); border-radius: var(--r-md); }
  .block-head { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; margin-bottom: 12px; }
  .rows { list-style: none; margin: 0; padding: 0; display: grid; }
  .row { display: flex; align-items: center; gap: 14px; padding: 12px 0; border-top: 1px solid var(--line); }
  .row:first-child { border-top: 0; }
  .avatar { display: grid; place-items: center; flex: none; width: 36px; height: 36px; border-radius: 50%; background: var(--ac-soft); color: var(--ac); font-weight: 700; }
  .row-main { flex: 1; min-width: 0; display: grid; gap: 2px; }
  .row-main strong { font-size: 14px; }
  .sub, .meta { font-size: 13px; color: var(--mu); overflow-wrap: anywhere; }
  .meta { flex: none; }
  progress { width: 100%; max-width: 320px; height: 4px; margin-top: 4px; accent-color: var(--ac); }
  .empty { margin: 0; font-size: 14px; color: var(--mu); } .empty[role='alert'] { color: var(--err); }
  .ghost { min-height: 38px; padding: 8px 16px; background: transparent; color: var(--tx); border: 1px solid var(--line-2); border-radius: var(--r-md); font: inherit; font-size: 13px; cursor: pointer; }
  .ghost:hover { background: var(--pn-2); } .ghost:disabled { opacity: 0.5; cursor: not-allowed; }
  .technical summary { cursor: pointer; font-size: 13px; }
  .ghost:focus-visible, .callout a:focus-visible, summary:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  dl { display: grid; gap: 10px; margin: 12px 0 0; font-size: 13px; }
  dl div { display: flex; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
  dt { color: var(--mu); } dd { margin: 0; overflow-wrap: anywhere; }
  @media (max-width: 900px) { .stats { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
