<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { adminRequest } from './request';
  import type { CoreHealth, AdminNode, Library, Scan } from './types';
  let {coreHealth,nodes,libraries,scans,active=true}:{coreHealth:CoreHealth|null;nodes:AdminNode[];libraries:Library[];scans:Record<string,Scan>;active?:boolean}=$props();
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
<section id="overview" aria-labelledby="overview-heading">
  <div class="heading"><div><h2 id="overview-heading">Vue d’ensemble</h2><p>L’essentiel de votre serveur, en un coup d’œil.</p></div><button disabled={loading} onclick={()=>void refresh()}>Actualiser l’activité</button></div>
  <div class="overview-grid">
    <article><span>Serveur</span><strong class:healthy={coreHealth?.status==='ok'}>{coreHealth?.status==='ok'?'Actif':'Indisponible'}</strong><small>{coreHealth?.version??'Version non communiquée'}</small></article>
    <article><span>Utilisateurs actifs</span><strong>{activityError?'—':users}</strong><small>Activité reçue depuis moins de 2 minutes</small></article>
    <article><span>Lecteurs ouverts</span><strong>{activityError?'—':playing.length}</strong><small>{playing.filter(item=>!item.paused).length} en lecture · {playing.filter(item=>item.paused).length} en pause</small></article>
    <article><span>Bibliothèques</span><strong>{libraries.length}</strong><small>{running?`${running} indexation(s) en cours`:'Aucune indexation en cours'}</small></article>
  </div>
  <section class="panel" aria-label="Activité actuelle"><div class="heading"><h3>Qui regarde quoi ?</h3>{#if updated&&!activityError}<small>Actualisé à {updated}</small>{/if}</div>
    {#if activityError}<p role="alert">{activityError}</p>{:else if sessions.length}<div class="activity-list">{#each sessions as item,index (index)}<article class="activity-card"><div class="initial" aria-hidden="true">{item.name.slice(0,1).toLocaleUpperCase()}</div><div><strong>{item.name}</strong><p>{deviceName(item.device)}</p>{#if item.playbackId}<h4>{item.title??'Œuvre indisponible'}</h4><p>{item.paused?'En pause':'En lecture'} · {time(item.position)} / {time(item.duration)}</p><progress aria-label={`Progression de ${item.name}`} value={item.position??0} max={item.duration||1}></progress>{:else}<p>Navigation · actif récemment</p>{/if}</div></article>{/each}</div>{:else}<p>{loading?'Chargement de l’activité…':'Aucun utilisateur actif récemment.'}</p>{/if}
  </section>
  <section class="panel" aria-label="État des bibliothèques"><h3>Bibliothèques et indexation</h3>
    {#if issues.length}<p class="warning">Des fichiers ont été refusés lors du dernier scan de {issues.length} bibliothèque(s). Les médias déjà indexés restent disponibles.</p>
      {#each issues as issue (issue.library.id)}<details><summary>{issue.library.name} · {issue.scan.errorsCount??issue.scan.errors.length} erreur(s) au dernier scan</summary><p>{issue.scan.discovered} fichier(s) découvert(s) · {issue.scan.probed} analysé(s) · {issue.scan.reused} réutilisé(s)</p>{#if issue.scan.errors.length}<ul>{#each issue.scan.errors as message,index (index)}<li>{message}</li>{/each}</ul>{:else}<p>Le nœud n’a pas fourni le détail des erreurs.</p>{/if}<a href="#libraries-section">Ouvrir les bibliothèques pour relancer l’indexation</a></details>{/each}
    {:else}<p>Aucune erreur signalée par les derniers scans disponibles.</p>{/if}
  </section>
  <section class="panel"><h3>Media Nodes</h3><p>{connected} connecté(s) sur {nodes.length}{#if offline} · <span class="warning">{offline} hors ligne</span>{/if}</p><a href="#nodes-section">Gérer les nœuds</a></section>
  <details class="technical"><summary>Informations techniques</summary><dl><div><dt>Version serveur</dt><dd>{coreHealth?.version??'Non communiquée'}</dd></div><div><dt>Mode</dt><dd>{coreHealth?.mode??'Non communiqué'}</dd></div><div><dt>Versions des nœuds</dt><dd>{versions.join(', ')||'Aucun nœud'}</dd></div></dl></details>
</section>
<style>
h2{font-size:28px;letter-spacing:-.03em;margin:0;}h3{font-size:18px;margin:0 0 16px;}h4{margin:12px 0 6px;}p{font-size:14px;line-height:1.6;color:var(--text-secondary);}small{font-size:12px;color:var(--text-secondary);}.heading{display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap;margin-bottom:24px;}.heading p{margin-bottom:0;}.heading h3{margin:0;}.overview-grid{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:16px;}.overview-grid article{display:grid;gap:12px;padding:24px;background:var(--bg-surface);border:1px solid var(--border-subtle);border-radius:var(--radius-md);}.overview-grid span{font-size:13px;color:var(--text-secondary);}.overview-grid strong{font-size:28px;}.healthy{color:var(--status-success);}.panel{margin-top:24px;padding:24px;border:1px solid var(--border-subtle);border-radius:var(--radius-md);background:var(--bg-surface);}.activity-list{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,280px),1fr));gap:16px;}.activity-card{display:flex;gap:16px;min-width:0;padding:16px;background:var(--bg-elevated);border-radius:var(--radius-sm);}.activity-card>div:last-child{min-width:0;flex:1;overflow-wrap:anywhere;}.activity-card p{margin:4px 0;}.initial{display:grid;place-items:center;flex:0 0 44px;height:44px;background:var(--color-accent);color:white;border-radius:50%;font-size:20px;}progress{width:100%;height:5px;accent-color:var(--color-accent);}button{min-height:42px;padding:10px 16px;background:var(--bg-elevated);color:var(--text-primary);border:1px solid var(--border-subtle);border-radius:var(--radius-sm);font:inherit;cursor:pointer;}button:disabled{opacity:.5;}a{color:var(--color-accent);}details{margin-top:12px;padding:16px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);}summary{cursor:pointer;font-size:14px;}li{font-size:13px;line-height:1.6;overflow-wrap:anywhere;}.warning{color:var(--status-warning);}[role=alert]{color:var(--status-error);}dl{display:grid;gap:12px;font-size:13px;}dl div{display:flex;justify-content:space-between;gap:16px;flex-wrap:wrap;}dt{color:var(--text-secondary);}dd{margin:0;overflow-wrap:anywhere;}@media(max-width:1000px){.overview-grid{grid-template-columns:repeat(2,minmax(0,1fr));}}@media(max-width:600px){.overview-grid{grid-template-columns:1fr;}.panel{padding:16px;}}
</style>
