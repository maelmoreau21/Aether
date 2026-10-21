<script lang="ts">
  import ImageManager from './ImageManager.svelte';
  let imagesOpen=$state(false);
  import { session } from '../auth/session';
  import type { Work } from './api';
  let { work }: { work:Work }=$props();
  let lists=$state<{id:string;name:string}[]>([]);let listName=$state('');let error=$state('');let message=$state('');let busy=$state(false);let title=$state('');let plot=$state('');let year=$state<number|undefined>();let genres=$state('');let tags=$state('');let ids=$state('{}');let age=$state<number|undefined>();let copyId=$state('');
  $effect(()=>{title=work.title;plot=work.plot??work.description;year=work.year??undefined;genres=(work.genres??[]).join(', ');tags=(work.tags??[]).join(', ');ids=JSON.stringify(work.externalIds??{},null,2);age=work.minimumAge??undefined;});
  async function loadLists(){try{const response=await fetch('/api/v1/lists');if(!response.ok)throw new Error('Listes indisponibles.');lists=(await response.json()).lists;}catch(cause){error=cause instanceof Error?cause.message:'Listes indisponibles.';}}
  async function mutate(path:string,method:string,body?:unknown,contentType='application/json'){
    busy=true;error='';message='';try{const response=await fetch(path,{method,headers:{'content-type':contentType,'x-csrf-token':$session?.csrfToken??''},body:body instanceof File?body:body===undefined?undefined:JSON.stringify(body)});if(!response.ok)throw new Error(`Enregistrement impossible (HTTP ${response.status}).`);message='Enregistré.';return response.status===204?true:await response.json();}catch(cause){error=cause instanceof Error?cause.message:'Enregistrement impossible.';return false;}finally{busy=false;}}
  async function save(event:SubmitEvent){event.preventDefault();let externalIds;try{externalIds=JSON.parse(ids);}catch{error='Les identifiants doivent être un objet JSON.';return;}if(await mutate(`/api/v1/admin/works/${work.id}/metadata`,'PUT',{title,plot,year:year??null,genres:genres.split(',').map(s=>s.trim()).filter(Boolean),tags:tags.split(',').map(s=>s.trim()).filter(Boolean),externalIds,minimumAge:age??null}))window.location.reload();}
  async function create(){if(!listName.trim())return;const result=await mutate('/api/v1/lists','POST',{name:listName.trim()});if(result){if(await mutate(`/api/v1/lists/${encodeURIComponent(result.id)}/items/${encodeURIComponent(work.id)}`,'PUT')){listName='';await loadLists();message='Liste créée et média ajouté.';}}}

</script>
{#if imagesOpen}<ImageManager title={work.title} workId={work.id} onclose={()=>imagesOpen=false} onchanged={async()=>{window.dispatchEvent(new CustomEvent('media-artwork-updated',{detail:{id:work.id}}));}} />{/if}
<div class="work-actions">
{#if $session?.user.role==='administrator'}<button type="button" class="image-action" aria-label="Modifier les images" title="Modifier les images" onclick={()=>imagesOpen=true}>▧</button>{/if}
<details class="list-picker" ontoggle={event=>{if(event.currentTarget.open)void loadLists();}}><summary aria-label="Ajouter à une liste" title="Ajouter à une liste"><svg width="21" height="21" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 4v16M4 12h16" /></svg></summary>
<div class="list-menu"><strong>Ajouter à une liste</strong>
{#each lists as list(list.id)}<button type="button" disabled={busy} onclick={()=>mutate(`/api/v1/lists/${encodeURIComponent(list.id)}/items/${encodeURIComponent(work.id)}`,'PUT')}>{list.name}</button>{:else}<p>Aucune liste pour le moment.</p>{/each}
<form onsubmit={event=>{event.preventDefault();void create();}}><label>Nouvelle liste<input required bind:value={listName} maxlength="100" placeholder="Nom de la liste" /></label><button disabled={busy||!listName.trim()}>Créer et ajouter</button></form></div>
</details>
{#if $session?.user.role==='administrator'}<details class="media-management"><summary aria-label="Modifier les métadonnées" title="Modifier les métadonnées"><svg width="21" height="21" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="m4 16 12-12 4 4L8 20H4v-4Z" /></svg></summary><form onsubmit={save}><fieldset disabled={busy}><legend>Identification et métadonnées</legend><p>Identifiant de cette copie : <code>{work.id}</code></p>{#if work.globalMediaId}<p>Identité globale : <code>{work.globalMediaId}</code></p>{/if}<label>Titre<input required maxlength="512" bind:value={title} /></label><label>Année<input type="number" min="1800" max="2200" bind:value={year} /></label><label>Description<textarea bind:value={plot} maxlength="20000"></textarea></label><label>Genres séparés par des virgules<input bind:value={genres} /></label><label>Tags séparés par des virgules<input bind:value={tags} maxlength="6400" /></label><label>Identifiants externes (JSON : tmdb, imdb…)<textarea bind:value={ids}></textarea></label><label>Âge minimum (vide : classification inconnue)<input type="number" min="0" max="21" bind:value={age} /></label><button>Enregistrer</button><button type="button" onclick={async()=>{if(await mutate(`/api/v1/admin/works/${work.id}/metadata`,'DELETE'))window.location.reload();}}>Revenir aux métadonnées du scan</button></fieldset></form>
{#if work.kind==='film'}<fieldset disabled={busy}><legend>Regrouper des copies</legend><label>Identifiant exact du même film sur un autre Node<input bind:value={copyId} /></label><button disabled={!copyId||copyId===work.id} onclick={async()=>{if(await mutate('/api/v1/admin/media-groups','POST',{workIds:[work.id,copyId],label:work.title}))window.location.reload();}}>Associer les deux films</button><button onclick={async()=>{if(await mutate(`/api/v1/admin/media-groups/${work.id}`,'DELETE'))window.location.reload();}}>Dissocier cette copie</button></fieldset>{/if}</details>{/if}
{#if error}<p role="alert">{error}</p>{/if}{#if message}<p role="status">{message}</p>{/if}</div>
<style>
.work-actions { display:flex;gap:10px;align-items:flex-start;flex-wrap:wrap;margin-block:12px; }
summary { list-style:none;display:grid;place-items:center;width:44px;height:44px;box-sizing:border-box;border:1px solid var(--color-line);border-radius:var(--radius-sm);background:var(--color-surface);cursor:pointer; }
summary::-webkit-details-marker { display:none; } summary:hover { color:var(--color-accent);border-color:var(--color-accent); }
summary:focus-visible,button:focus-visible { outline:2px solid var(--color-accent);outline-offset:3px; }
.list-menu { display:grid;gap:10px;min-width:230px;max-width:100%;padding:16px;margin-top:8px;background:var(--color-surface);border:1px solid var(--color-line);border-radius:var(--radius-md); }
.list-menu form { display:grid;gap:10px;border-top:1px solid var(--color-line);padding-top:14px;margin-top:6px; }
fieldset { display:grid;gap:12px;margin:16px 0;border:1px solid var(--color-line);border-radius:var(--radius-sm);padding:16px; }
label { display:grid;gap:6px; } input,textarea,button { padding:10px;background:var(--color-surface);color:var(--color-text);border:1px solid var(--color-line);border-radius:var(--radius-sm);font:inherit;max-width:100%;box-sizing:border-box; }
button { cursor:pointer; } button:disabled { opacity:.5;cursor:wait; } textarea { min-height:90px; } code { overflow-wrap:anywhere; }
[role="alert"],[role="status"] { flex-basis:100%;margin:0; } [role="alert"] { color:var(--status-error); }
</style>
