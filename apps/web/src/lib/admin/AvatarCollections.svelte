<script lang="ts">
  import { onMount } from 'svelte';
  import { adminRequest } from './request';
  let {extensionId}:{extensionId:string}=$props();
  type Pack={id:string;name:string;count:number};
  let packs=$state<Pack[]>([]),folders=$state<{folder:string;count:number}[]>([]);
  let url=$state('https://github.com/cedev-1/jellyfin-avatars'),folder=$state(''),name=$state('');
  let busy=$state(false),error=$state(''),notice=$state('');
  let uploadName=$state(''),uploadFiles=$state<File[]>([]);
  let uploadInput=$state<HTMLInputElement>();
  const endpoint=$derived(`/api/v1/admin/extensions/${encodeURIComponent(extensionId)}/avatar-packs`);
  async function load(){packs=(await adminRequest<{packs:Pack[]}>(endpoint)).packs;}
  onMount(()=>{void load().catch(()=>error='Collections indisponibles.');});
  async function discover(){busy=true;error='';notice='';try{folders=(await adminRequest<{folders:typeof folders}>(endpoint+'/source',{method:'POST',body:JSON.stringify({url})})).folders;folder=folders[0]?.folder??'';name=folder||'Collection';if(!folders.length)notice='Aucune image PNG, JPEG ou WebP trouvée.';}catch(e){error=e instanceof Error?e.message:'Source indisponible.';}finally{busy=false;}}
  async function importPack(){busy=true;error='';notice='';try{const result=await adminRequest<{count:number}>(endpoint,{method:'POST',body:JSON.stringify({url,folder,name})});await load();notice=`${result.count} avatars importés.`;}catch(e){error=e instanceof Error?e.message:'Import impossible.';}finally{busy=false;}}
  async function remove(id:string){busy=true;error='';try{await adminRequest(endpoint+'/'+encodeURIComponent(id),{method:'DELETE'});await load();notice='Collection retirée.';}catch(e){error=e instanceof Error?e.message:'Suppression impossible.';}finally{busy=false;}}
  async function upload(){
    if(!uploadFiles.length||uploadFiles.length>500||uploadFiles.some(file=>file.size>2*1024*1024)||uploadFiles.reduce((sum,file)=>sum+file.size,0)>64*1024*1024){error='Choisissez de 1 à 500 images de 2 Mo maximum, 64 Mo au total.';return;}
    busy=true;error='';notice='';
    try{const form=new FormData();form.append('name',uploadName);for(const file of uploadFiles)form.append('images',file);const result=await adminRequest<{count:number}>(endpoint+'/upload',{method:'POST',body:form});await load();notice=`${result.count} avatar${result.count>1?'s':''} envoyé${result.count>1?'s':''} au serveur.`;uploadFiles=[];if(uploadInput)uploadInput.value='';}catch(e){error=e instanceof Error?e.message:'Envoi impossible.';}finally{busy=false;}
  }

</script>
<section aria-label="Collections d’avatars">
  <h3>Collections d’avatars</h3><p>Les collections sont conservées sur le serveur et disponibles dans les profils après activation.</p>
  <form onsubmit={event=>{event.preventDefault();void discover();}}>
    <label>Lien GitHub de la collection<input type="url" required bind:value={url} disabled={busy} oninput={()=>folders=[]} /></label>
    <button disabled={busy}>Parcourir les collections</button>
  </form>
  {#if folders.length}<form class="import-form" onsubmit={event=>{event.preventDefault();void importPack();}}>
    <label>Collection<select aria-label="Collection" bind:value={folder} disabled={busy} onchange={()=>name=folder||'Collection'}>{#each folders as item (item.folder)}<option value={item.folder}>{item.folder||'Racine'} · {item.count} avatars</option>{/each}</select></label>
    <label>Nom affiché<input required maxlength="100" bind:value={name} disabled={busy} /></label><button disabled={busy}>Importer la collection</button>
  </form>{/if}
  <details class="upload"><summary>Importer depuis cet ordinateur</summary>
    <form onsubmit={event=>{event.preventDefault();void upload();}}>
      <label>Nom de la collection<input required maxlength="100" bind:value={uploadName} disabled={busy} /></label>
      <label>Images à envoyer<input type="file" bind:this={uploadInput} required multiple accept="image/png,image/jpeg,image/webp" disabled={busy} onchange={event=>uploadFiles=Array.from(event.currentTarget.files??[])} /></label>
      <button disabled={busy||!uploadFiles.length}>Envoyer au serveur</button>
    </form><small>PNG, JPEG, WebP · 2 Mo par image · 64 Mo par collection.</small>
  </details>
  {#if busy}<p role="status">Chargement… L’import peut prendre quelques minutes.</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}
  <div class="packs">{#each packs as pack (pack.id)}<article><div><strong>{pack.name}</strong><small>{pack.count} avatars</small></div><button disabled={busy} onclick={()=>void remove(pack.id)}>Retirer</button></article>{/each}</div>
</section>
<style>
section{margin-top:28px;}h3{font-size:18px;margin:0 0 8px;}p,small{font-size:13px;color:var(--text-secondary);line-height:1.5;}form{display:flex;gap:12px;align-items:end;flex-wrap:wrap;margin:20px 0;}label{display:grid;gap:8px;font-size:13px;flex:1;min-width:160px;}input,select{box-sizing:border-box;width:100%;min-height:44px;background:var(--bg-base);color:var(--text-primary);border:1px solid var(--border-subtle);border-radius:var(--radius-sm);padding:10px;font:inherit;}button{min-height:44px;padding:10px 16px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);background:var(--bg-elevated);color:var(--text-primary);font:inherit;cursor:pointer;}button:disabled{opacity:.5;cursor:wait;}.packs{display:grid;gap:8px;}.packs article{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:16px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);}.packs article div{display:grid;gap:6px;}[role=alert]{color:var(--status-error);}.import-form{padding:16px;background:var(--bg-elevated);border-radius:var(--radius-sm);}
@media(max-width:650px){form{display:grid;grid-template-columns:minmax(0,1fr);align-items:stretch;}label{min-width:0;}button{width:100%;}.packs article button{width:auto;}.import-form{padding:14px;}}
.upload{margin:24px 0;padding:16px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);}summary{font-size:14px;cursor:pointer;}input[type=file]{font-size:12px;}
</style>
