<script lang="ts">
  import { onMount } from 'svelte';
  import { adminRequest } from './request';
  let {extensionId}:{extensionId?:string}=$props();
  type Pack={id:string;name:string;count:number};
  let packs=$state<Pack[]>([]),folders=$state<{folder:string;count:number}[]>([]);
  let url=$state('https://github.com/cedev-1/jellyfin-avatars'),folder=$state(''),name=$state('');
  let busy=$state(false),error=$state(''),notice=$state('');
  let policy=$state({allowCustom:true,allowGravatar:false,assignOnCreate:false});
  async function loadPolicy(){policy=await adminRequest<typeof policy>('/api/v1/admin/avatar-policy');}
  async function savePolicy(){busy=true;error='';notice='';try{await adminRequest('/api/v1/admin/avatar-policy',{method:'PUT',body:JSON.stringify(policy)});notice='Politique enregistrée.';}catch(e){error=e instanceof Error?e.message:'Enregistrement impossible.';await loadPolicy();}finally{busy=false;}}
  let uploadName=$state('Mes avatars'),uploadFiles=$state<File[]>([]);
  let uploadInput=$state<HTMLInputElement>();
  const endpoint=$derived(extensionId ? `/api/v1/admin/extensions/${encodeURIComponent(extensionId)}/avatar-packs` : `/api/v1/admin/avatar-packs`);
  const assignMissingEndpoint=$derived(extensionId ? `/api/v1/admin/extensions/${encodeURIComponent(extensionId)}/avatars/assign-missing` : `/api/v1/admin/avatars/assign-missing`);
  type Avatar={id:string;name:string;url:string};
  let selected=$state(''),images=$state<Avatar[]>([]),renaming=$state('');
  async function showPack(id:string){selected=id;renaming=packs.find(pack=>pack.id===id)?.name??'';images=[];if(!id)return;const result=await adminRequest<{images:Avatar[]}>(`${endpoint}/${encodeURIComponent(id)}/images`);if(selected===id)images=result.images;}
  async function load(){packs=(await adminRequest<{packs:Pack[]}>(endpoint)).packs;await showPack(packs.some(pack=>pack.id===selected)?selected:packs[0]?.id??'');}
  async function galleryAction(action:()=>Promise<unknown>,message:string){busy=true;error='';notice='';try{await action();await load();notice=message;}catch(e){error=e instanceof Error?e.message:'Action impossible.';}finally{busy=false;}}
  async function assignMissing(){busy=true;error='';notice='';try{const result=await adminRequest<{count:number}>(assignMissingEndpoint,{method:'POST'});notice=`${result.count} compte${result.count>1?'s':''} ${result.count>1?'ont':'a'} reçu un avatar.`;}catch(e){error=e instanceof Error?e.message:'Attribution impossible.';}finally{busy=false;}}

  onMount(()=>{void load().catch(()=>error='Collections indisponibles.');if(!extensionId)void loadPolicy().catch(()=>error='Politique indisponible.');});
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
  {#if !extensionId}
    <fieldset disabled={busy}><legend>Politique des avatars</legend>
      <label><input type="checkbox" bind:checked={policy.allowCustom} /> Autoriser les images personnalisées</label>
      <label><input type="checkbox" bind:checked={policy.allowGravatar} /> Autoriser Gravatar</label>
      <label><input type="checkbox" bind:checked={policy.assignOnCreate} /> Attribuer un avatar de la galerie aux nouveaux comptes</label>
      <button type="button" onclick={()=>void savePolicy()}>Enregistrer la politique</button>
    </fieldset>
  {/if}
  <h3>Votre galerie d’avatars</h3><p>Ajoutez vos images ou importez une collection. Les utilisateurs choisissent ensuite leur avatar dans leur profil.</p>
  <button disabled={busy||!packs.some(pack=>pack.count>0)} onclick={()=>void assignMissing()}>Attribuer les avatars manquants maintenant</button>
  <details class="upload" open><summary>Ajouter des images</summary>
    <form onsubmit={event=>{event.preventDefault();void upload();}}>
      <label>Nom de la collection<input required maxlength="100" bind:value={uploadName} disabled={busy} /></label>
      <label>Images<input type="file" bind:this={uploadInput} required multiple accept="image/png,image/jpeg,image/webp" disabled={busy} onchange={event=>uploadFiles=Array.from(event.currentTarget.files??[])} /></label>
      <button disabled={busy||!uploadFiles.length}>Ajouter les avatars</button>
    </form><small>PNG, JPEG, WebP · 2 Mo par image · 64 Mo par import.</small>
  </details>
  <details class="upload"><summary>Importer un pack depuis GitHub</summary>
  <form onsubmit={event=>{event.preventDefault();void discover();}}>
    <label>Lien GitHub de la collection<input type="url" required bind:value={url} disabled={busy} oninput={()=>folders=[]} /></label>
    <button disabled={busy}>Parcourir les collections</button>
  </form>
  {#if folders.length}<form class="import-form" onsubmit={event=>{event.preventDefault();void importPack();}}>
    <label>Collection<select aria-label="Collection" bind:value={folder} disabled={busy} onchange={()=>name=folder||'Collection'}>{#each folders as item (item.folder)}<option value={item.folder}>{item.folder||'Racine'} · {item.count} avatars</option>{/each}</select></label>
    <label>Nom affiché<input required maxlength="100" bind:value={name} disabled={busy} /></label><button disabled={busy}>Importer la collection</button>
  </form>{/if}
  </details>
  {#if busy}<p role="status">Chargement… L’import peut prendre quelques minutes.</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}
  {#if packs.length}
    <label class="collection-picker">Collection<select disabled={busy} value={selected} onchange={event=>void showPack(event.currentTarget.value).catch(()=>error='Galerie indisponible.')}>{#each packs as pack (pack.id)}<option value={pack.id}>{pack.name} · {pack.count} avatar{pack.count>1?'s':''}</option>{/each}</select></label>
    <form onsubmit={event=>{event.preventDefault();void galleryAction(()=>adminRequest(`${endpoint}/${encodeURIComponent(selected)}`,{method:'PUT',body:JSON.stringify({name:renaming})}),'Collection renommée.');}}>
      <label>Nom de la collection<input required maxlength="100" bind:value={renaming} disabled={busy} /></label><button disabled={busy}>Renommer</button>
      <button type="button" disabled={busy} onclick={()=>void remove(selected)}>Supprimer la collection</button>
    </form>
    <div class="gallery">{#each images as avatar (avatar.id)}<article><img src={avatar.url} alt={avatar.name} loading="lazy" width="112" height="112" /><span>{avatar.name}</span><button aria-label={`Supprimer ${avatar.name}`} disabled={busy} onclick={()=>void galleryAction(()=>adminRequest(`${endpoint}/${encodeURIComponent(selected)}/images/${encodeURIComponent(avatar.id)}`,{method:'DELETE'}),'Avatar supprimé de la galerie.')}>Supprimer</button></article>{:else}<p>Cette collection est vide.</p>{/each}</div>
  {:else}<p>Aucun avatar pour le moment. Ajoutez des images ou importez un pack.</p>{/if}
</section>
<style>
section{margin-top:28px;}fieldset{display:grid;gap:12px;border:1px solid var(--border-subtle);padding:16px;}fieldset label{display:flex;align-items:center;gap:8px;}fieldset input[type=checkbox]{width:auto;min-height:0;}h3{font-size:18px;margin:0 0 8px;}p,small{font-size:13px;color:var(--text-secondary);line-height:1.5;}form{display:flex;gap:12px;align-items:end;flex-wrap:wrap;margin:20px 0;}label{display:grid;gap:8px;font-size:13px;flex:1;min-width:160px;}input,select{box-sizing:border-box;width:100%;min-height:44px;background:var(--bg-base);color:var(--text-primary);border:1px solid var(--border-subtle);border-radius:var(--radius-sm);padding:10px;font:inherit;}button{min-height:44px;padding:10px 16px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);background:var(--bg-elevated);color:var(--text-primary);font:inherit;cursor:pointer;}button:disabled{opacity:.5;cursor:wait;}.gallery{display:grid;grid-template-columns:repeat(auto-fill,minmax(120px,1fr));gap:16px;}.gallery article{display:grid;justify-items:center;gap:8px;min-width:0;padding:12px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);}.gallery img{width:100%;height:auto;aspect-ratio:1;object-fit:cover;border-radius:var(--radius-sm);}.gallery span{font-size:12px;max-width:100%;overflow-wrap:anywhere;}.gallery button{font-size:12px;min-height:36px;padding:6px 10px;color:var(--status-error);}.collection-picker{margin-top:24px;}[role=alert]{color:var(--status-error);}.import-form{padding:16px;background:var(--bg-elevated);border-radius:var(--radius-sm);}
@media(max-width:650px){form{display:grid;grid-template-columns:minmax(0,1fr);align-items:stretch;}label{min-width:0;}button{width:100%;}.import-form{padding:14px;}}
.upload{margin:24px 0;padding:16px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);}summary{font-size:14px;cursor:pointer;}input[type=file]{font-size:12px;}
</style>
