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
  const users=$derived(new Set(sessions.map(item=>item.userId)).size);
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
<section id="overview" class="admin-overview" aria-labelledby="overview-heading">
  <div class="heading"><div><h2 id="overview-heading">Vue d’ensemble</h2><p>L’essentiel de votre serveur, en un coup d’œil.</p></div><button disabled={loading} onclick={()=>void refresh()}>Actualiser l’activité</button></div>
  <div class="overview-grid">
    <article><span>Serveur</span><strong class:healthy={coreHealth?.status==='ok'}>{coreHealth?.status==='ok'?'Actif':coreHealth?'À vérifier':'—'}</strong><small>{coreHealth?.version??'État non communiqué'}</small></article>
    <article><span>Media Nodes</span><strong class:warning={offline>0}>{nodesError?'—':`${connected} / ${nodes.length}`}</strong><small>{nodesError?'État indisponible':offline?`${offline} hors ligne`:'Connectés'}</small></article>
    <article><span>Lecteurs ouverts</span><strong>{activityError||!updated?'—':playing.length}</strong><small>{activityError?'Activité indisponible':!updated?'Chargement…':`${users} utilisateur(s) actif(s) · ${playing.filter(item=>item.paused).length} en pause`}</small></article>
    <article><span>Indexations en cours</span><strong>{running}</strong><small>{libraries.length} bibliothèque(s)</small></article>
  </div>
  <section class="panel" aria-label="Activité actuelle"><div class="heading"><h3>Qui regarde quoi ?</h3>{#if updated&&!activityError}<small>Actualisé à {updated}</small>{/if}</div>
    {#if activityError}<p role="alert">{activityError}</p>{:else if sessions.length}<div class="activity-list">{#each sessions as item,index (index)}<article class="activity-card"><div class="initial" aria-hidden="true">{item.name.slice(0,1).toLocaleUpperCase()}</div><div><strong>{item.name}</strong><p>{deviceName(item.device)}</p>{#if item.playbackId}<h4>{item.title??'Œuvre indisponible'}</h4><p>{item.paused?'En pause':'En lecture'} · {time(item.position)} / {time(item.duration)}</p><progress aria-label={`Progression de ${item.name}`} value={item.position??0} max={item.duration||1}></progress>{:else}<p>Navigation · actif récemment</p>{/if}</div></article>{/each}</div>{:else}<p>{loading?'Chargement de l’activité…':'Aucun utilisateur actif récemment.'}</p>{/if}
  </section>
  <section class="panel" aria-label="État des bibliothèques"><h3>Bibliothèques et indexation</h3>
    {#if issues.length}<p class="warning">Des fichiers ont été refusés lors du dernier scan de {issues.length} bibliothèque(s). Les médias déjà indexés restent disponibles.</p>
      {#each issues as issue (issue.library.id)}<details><summary>{issue.library.name} · {issue.scan.errorsCount??issue.scan.errors.length} erreur(s) au dernier scan</summary><p>{issue.scan.discovered} fichier(s) découvert(s) · {issue.scan.probed} analysé(s) · {issue.scan.reused} réutilisé(s)</p>{#if issue.scan.errors.length}<ul>{#each issue.scan.errors as message,index (index)}<li>{message}</li>{/each}</ul>{:else}<p>Le nœud n’a pas fourni le détail des erreurs.</p>{/if}<a href="#libraries-section">Ouvrir les bibliothèques pour relancer l’indexation</a></details>{/each}
    {:else}<p>Aucune erreur signalée par les derniers scans disponibles.</p>{/if}
  </section>
  <section class="panel"><h3>Media Nodes</h3>{#if nodesError}<p role="alert">{nodesError}</p>{:else}<p>{connected} connecté(s) sur {nodes.length}{#if offline} · <span class="warning">{offline} hors ligne</span>{/if}</p>{/if}<a href="#nodes-section">Gérer les nœuds</a></section>
  <details class="technical"><summary>Informations techniques</summary><dl><div><dt>Version serveur</dt><dd>{coreHealth?.version??'Non communiquée'}</dd></div><div><dt>Mode</dt><dd>{coreHealth?.mode??'Non communiqué'}</dd></div><div><dt>Versions des nœuds</dt><dd>{versions.join(', ')||'Aucun nœud'}</dd></div></dl></details>
</section>
<style>
  .admin-overview { display: grid; gap: 20px; min-width: 0; font-family: var(--font); color: var(--tx); }
  h2 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.15; }
  h3 { margin: 0 0 16px; font-size: 17px; font-weight: 700; }
  h4 { margin: 12px 0 6px; font-size: 15px; font-weight: 700; overflow-wrap: anywhere; }
  p { margin: 0 0 8px; font-size: 14px; line-height: 1.6; color: var(--mu); }
  small { font-size: 13px; line-height: 1.5; color: var(--mu); }
  .heading { display: flex; align-items: center; justify-content: space-between; gap: 12px 16px; flex-wrap: wrap; }
  .heading p { margin: 6px 0 0; }
  .panel .heading { margin-bottom: 16px; }
  .heading h3 { margin: 0; }
  .overview-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 16px; }
  .overview-grid article { display: grid; align-content: start; gap: 8px; min-width: 0; padding: 24px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  .overview-grid span { font-size: 13px; font-weight: 600; color: var(--mu); }
  .overview-grid strong { font-size: 30px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.1; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .overview-grid small { line-height: 1.5; }
  .healthy { color: var(--ok); }
  .warning { color: var(--warn); }
  .panel { min-width: 0; padding: 24px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  .panel > :last-child { margin-bottom: 0; }
  .activity-list { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 280px), 1fr)); column-gap: 24px; }
  .activity-card { display: flex; gap: 14px; min-width: 0; padding: 14px 0; border-top: 1px solid var(--line); }
  .activity-card > div:last-child { min-width: 0; flex: 1; overflow-wrap: anywhere; }
  .activity-card strong { font-size: 15px; font-weight: 700; }
  .activity-card p { margin: 2px 0; font-size: 13px; }
  .initial { display: grid; place-items: center; flex: 0 0 44px; height: 44px; border-radius: 50%; background: var(--pn-2); color: var(--tx); font-size: 17px; font-weight: 800; }
  progress { display: block; width: 100%; height: 4px; margin-top: 8px; border: 0; border-radius: var(--r-pill); overflow: hidden; background: var(--pn-2); color: var(--ac); appearance: none; -webkit-appearance: none; }
  progress::-webkit-progress-bar { background: var(--pn-2); border-radius: var(--r-pill); }
  progress::-webkit-progress-value { background: var(--ac); border-radius: var(--r-pill); }
  progress::-moz-progress-bar { background: var(--ac); border-radius: var(--r-pill); }
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 44px; padding: 0 18px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  a { display: inline-flex; align-items: center; min-height: 44px; color: var(--tx); font-size: 14px; font-weight: 700; text-decoration: underline; text-decoration-color: var(--line-2); text-underline-offset: 4px; transition: text-decoration-color var(--t-fast) ease; }
  a:hover { text-decoration-color: currentColor; }
  button:focus-visible, a:focus-visible, summary:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .panel details { margin: 0; padding: 4px 0 12px; border-top: 1px solid var(--line); }
  .panel details:last-of-type { padding-bottom: 0; }
  summary { display: flex; align-items: center; min-height: 44px; cursor: pointer; font-size: 14px; font-weight: 700; overflow-wrap: anywhere; }
  ul { margin: 0 0 8px; padding-left: 20px; }
  li { font-size: 13px; line-height: 1.6; color: var(--mu); overflow-wrap: anywhere; }
  [role='alert'] { color: var(--err); }
  .technical { padding: 8px 24px; background: var(--bg-2); border: 1px solid var(--line); border-radius: var(--r-lg); }
  .technical[open] { padding-bottom: 16px; }
  dl { display: grid; margin: 4px 0 0; font-size: 14px; }
  dl div { display: flex; justify-content: space-between; gap: 6px 16px; flex-wrap: wrap; padding: 12px 0; border-top: 1px solid var(--line); }
  dt { color: var(--mu); font-weight: 600; }
  dd { margin: 0; font-family: var(--font-mono); font-size: 13px; overflow-wrap: anywhere; }
  @media (max-width: 1023px) { .overview-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @media (max-width: 640px) {
    .overview-grid { gap: 12px; }
    .overview-grid article { padding: 16px; border-radius: var(--r-md); }
    .overview-grid strong { font-size: 26px; }
    .panel { padding: 16px; border-radius: var(--r-md); }
    .technical { padding: 4px 16px; border-radius: var(--r-md); }
    .heading > button { width: 100%; }
  }
  @media (prefers-reduced-motion: reduce) { button, a { transition: none; } }
</style>
