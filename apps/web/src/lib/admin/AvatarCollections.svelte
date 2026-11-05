<script lang="ts">
  import { onMount } from 'svelte';
  import { adminRequest } from './request';
  let {extensionId}:{extensionId?:string}=$props();
  type Pack={id:string;name:string;count:number};
  let packs=$state<Pack[]>([]);
  let busy=$state(false),error=$state(''),notice=$state('');
  let policy=$state({allowCustom:true,allowGravatar:false,assignOnCreate:false});
  let policyLoaded=$state(false);
  let legacy=$state({packs:0,images:0});
  async function loadLegacy(){legacy=await adminRequest<typeof legacy>('/api/v1/admin/avatars/migrate-legacy');}
  async function migrateLegacy(){busy=true;error='';notice='';try{const result=await adminRequest<typeof legacy>('/api/v1/admin/avatars/migrate-legacy',{method:'POST'});await load();await loadLegacy();notice=`${result.packs} collection(s) et ${result.images} image(s) transférées.`;}catch(e){error=e instanceof Error?e.message:'Transfert impossible.';}finally{busy=false;}}
  async function loadPolicy(){policy=await adminRequest<typeof policy>('/api/v1/admin/avatar-policy');policyLoaded=true;}
  async function savePolicy(){busy=true;error='';notice='';try{await adminRequest('/api/v1/admin/avatar-policy',{method:'PUT',body:JSON.stringify(policy)});notice='Politique enregistrée.';}catch(e){error=e instanceof Error?e.message:'Enregistrement impossible.';try{await loadPolicy();}catch{policyLoaded=false;}}finally{busy=false;}}
  let uploadName=$state('Mes avatars'),uploadTarget=$state(''),uploadFiles=$state<File[]>([]);
  let uploadInput=$state<HTMLInputElement>();
  const endpoint=$derived(extensionId ? `/api/v1/admin/extensions/${encodeURIComponent(extensionId)}/avatar-packs` : `/api/v1/admin/avatar-packs`);
  const assignMissingEndpoint=$derived(extensionId ? `/api/v1/admin/extensions/${encodeURIComponent(extensionId)}/avatars/assign-missing` : `/api/v1/admin/avatars/assign-missing`);
  type Avatar={id:string;name:string;url:string};
  let selected=$state(''),images=$state<Avatar[]>([]),renaming=$state('');
  async function showPack(id:string){selected=id;renaming=packs.find(pack=>pack.id===id)?.name??'';images=[];if(!id)return;const result=await adminRequest<{images:Avatar[]}>(`${endpoint}/${encodeURIComponent(id)}/images`);if(selected===id)images=result.images;}
  async function load(){packs=(await adminRequest<{packs:Pack[]}>(endpoint)).packs;if(uploadTarget&&!packs.some(pack=>pack.id===uploadTarget))uploadTarget='';await showPack(packs.some(pack=>pack.id===selected)?selected:packs[0]?.id??'');}
  async function galleryAction(action:()=>Promise<unknown>,message:string){busy=true;error='';notice='';try{await action();await load();notice=message;}catch(e){error=e instanceof Error?e.message:'Action impossible.';}finally{busy=false;}}
  async function assignMissing(){busy=true;error='';notice='';try{const result=await adminRequest<{count:number}>(assignMissingEndpoint,{method:'POST'});notice=`${result.count} compte${result.count>1?'s':''} ${result.count>1?'ont':'a'} reçu un avatar.`;}catch(e){error=e instanceof Error?e.message:'Attribution impossible.';}finally{busy=false;}}

  onMount(()=>{void load().catch(()=>error='Collections indisponibles.');if(!extensionId){void loadPolicy().catch(()=>error='Politique indisponible.');void loadLegacy().catch(()=>error='État des anciennes collections indisponible.');}});
  async function remove(id:string){if(!window.confirm('Supprimer cette collection de la galerie ? Les photos déjà choisies par les comptes sont conservées.'))return;busy=true;error='';try{await adminRequest(endpoint+'/'+encodeURIComponent(id),{method:'DELETE'});await load();notice='Collection retirée.';}catch(e){error=e instanceof Error?e.message:'Suppression impossible.';}finally{busy=false;}}
  async function upload(){
    if(!uploadFiles.length||uploadFiles.length>500||uploadFiles.some(file=>file.size>2*1024*1024)||uploadFiles.reduce((sum,file)=>sum+file.size,0)>64*1024*1024){error='Choisissez de 1 à 500 images de 2 Mo maximum, 64 Mo au total.';return;}
    busy=true;error='';notice='';
    try{const form=new FormData();if(uploadTarget)form.append('packId',uploadTarget);else form.append('name',uploadName);for(const file of uploadFiles)form.append('images',file);const result=await adminRequest<{id:string;count:number}>(endpoint+'/upload',{method:'POST',body:form});selected=result.id;await load();notice=`${result.count} avatar${result.count>1?'s':''} envoyé${result.count>1?'s':''} au serveur.`;uploadFiles=[];if(uploadInput)uploadInput.value='';}catch(e){error=e instanceof Error?e.message:'Envoi impossible.';}finally{busy=false;}
  }

</script>
<section class="avatar-collections" class:embedded={!!extensionId} aria-label="Collections d’avatars">
  {#if extensionId}
    <p>Cette collection appartient à une ancienne extension. Les avatars sont désormais gérés nativement dans Administration → Avatars. Les anciennes collections ne sont pas transférées automatiquement : les supprimer ou désinstaller l’extension peut les retirer.</p>
  {:else}
    <p>Gérez les images proposées dans les profils des utilisateurs.</p>
  {/if}
  {#if !extensionId}
    {#if legacy.packs > 0}
      <div class="legacy-notice"><strong>Collections à transférer</strong><p>{legacy.packs} collection(s) et {legacy.images} image(s) proviennent d’une ancienne installation. Transférez-les pour les gérer ici et les conserver après sa suppression.</p><button disabled={busy} onclick={()=>void migrateLegacy()}>Transférer les collections</button></div>
    {/if}
    <fieldset disabled={busy||!policyLoaded}><legend>Politique des avatars</legend>
      <label><input type="checkbox" bind:checked={policy.allowCustom} /> Autoriser les images personnalisées</label>
      <label><input type="checkbox" bind:checked={policy.assignOnCreate} /> Attribuer un avatar de la galerie aux nouveaux comptes</label>
      <button type="button" class="primary" onclick={()=>void savePolicy()}>Enregistrer la politique</button>
    </fieldset>
  {/if}
  <h3>Votre galerie d’avatars</h3><p>Ajoutez vos images et organisez-les en collections. Les utilisateurs choisissent ensuite leur avatar dans leur profil.</p>
  <button disabled={busy||!packs.some(pack=>pack.count>0)} onclick={()=>void assignMissing()}>Attribuer les avatars manquants maintenant</button>
  <details class="upload" open><summary>Ajouter des images</summary>
    <form onsubmit={event=>{event.preventDefault();void upload();}}>
      <label>Destination<select bind:value={uploadTarget} disabled={busy}><option value="">Nouvelle collection</option>{#each packs as pack (pack.id)}<option value={pack.id}>{pack.name} · {pack.count} avatars</option>{/each}</select></label>
      {#if !uploadTarget}<label>Nom de la collection<input required maxlength="100" bind:value={uploadName} disabled={busy} /></label>{/if}
      <label>Images<input type="file" bind:this={uploadInput} required multiple accept="image/png,image/jpeg,image/webp" disabled={busy} onchange={event=>uploadFiles=Array.from(event.currentTarget.files??[])} /></label>
      <button class="primary" disabled={busy||!uploadFiles.length}>Ajouter les avatars</button>
    </form><small>PNG, JPEG, WebP · 2 Mo par image · 64 Mo par import · 500 images par collection.</small>
  </details>
  {#if busy}<p role="status">Traitement en cours…</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}
  {#if packs.length}
    <label class="collection-picker">Collection<select disabled={busy} value={selected} onchange={event=>void showPack(event.currentTarget.value).catch(()=>error='Galerie indisponible.')}>{#each packs as pack (pack.id)}<option value={pack.id}>{pack.name} · {pack.count} avatar{pack.count>1?'s':''}</option>{/each}</select></label>
    <form onsubmit={event=>{event.preventDefault();void galleryAction(()=>adminRequest(`${endpoint}/${encodeURIComponent(selected)}`,{method:'PUT',body:JSON.stringify({name:renaming})}),'Collection renommée.');}}>
      <label>Nom de la collection<input required maxlength="100" bind:value={renaming} disabled={busy} /></label><button disabled={busy}>Renommer</button>
      <button type="button" class="danger" disabled={busy} onclick={()=>void remove(selected)}>Supprimer la collection</button>
    </form>
    <div class="gallery">{#each images as avatar (avatar.id)}<article><img src={avatar.url} alt={avatar.name} loading="lazy" width="112" height="112" /><span>{avatar.name}</span><button class="danger" aria-label={`Supprimer ${avatar.name}`} disabled={busy} onclick={()=>{if(window.confirm(`Retirer « ${avatar.name} » de la galerie ? Les photos déjà choisies sont conservées.`))void galleryAction(()=>adminRequest(`${endpoint}/${encodeURIComponent(selected)}/images/${encodeURIComponent(avatar.id)}`,{method:'DELETE'}),'Avatar supprimé de la galerie.');}}>Supprimer</button></article>{:else}<p>Cette collection est vide.</p>{/each}</div>
  {:else}<p>Aucun avatar pour le moment. Ajoutez des images.</p>{/if}
</section>
<style>
  .avatar-collections { display: grid; gap: 16px; min-width: 0; margin-top: 8px; font-family: var(--font-sans); color: var(--color-text); }
  .avatar-collections > p { margin: 0; }
  h3 { margin: 4px 0 0; font-size: 16px; font-weight: 600; }
  p, small { margin: 0; font-size: 13px; line-height: 1.6; color: var(--color-muted); }
  small { display: block; }
  /* Surfaces plates */
  fieldset, .upload, .legacy-notice { min-width: 0; margin: 0; padding: 16px 20px; border: 0; border-radius: var(--radius-md); background: var(--color-surface); }
  fieldset { display: grid; gap: 0; }
  legend { float: left; width: 100%; margin: 0 0 4px; padding: 0; font-size: 15px; font-weight: 600; color: var(--color-text); }
  legend + label { clear: both; }
  fieldset label { display: flex; flex: none; align-items: center; gap: 12px; min-height: 44px; padding: 4px 0; border-top: 1px solid var(--color-line-subtle); font-size: 14px; font-weight: 500; }
  fieldset > button { justify-self: start; margin-top: 12px; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; min-height: 0; margin: 0; accent-color: var(--color-accent); }
  .legacy-notice { display: grid; justify-items: start; gap: 4px; background: var(--color-warning-soft); }
  .legacy-notice strong { font-size: 14px; font-weight: 600; color: var(--color-text); }
  .legacy-notice p { margin-bottom: 8px; }
  .avatar-collections > button { justify-self: start; }
  .upload { padding-top: 4px; padding-bottom: 4px; }
  .upload[open] { padding-bottom: 16px; }
  summary { display: flex; align-items: center; min-height: 48px; font-size: 14px; font-weight: 600; cursor: pointer; }
  .upload[open] > summary { border-bottom: 1px solid var(--color-line-subtle); }
  summary:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; border-radius: var(--radius-xs); }
  /* Formulaires */
  form { display: flex; flex-wrap: wrap; align-items: end; gap: 12px 16px; margin: 16px 0 0; }
  .upload form + small { margin-top: 12px; }
  label { display: grid; flex: 1; gap: 8px; min-width: 180px; font-size: 14px; font-weight: 600; color: var(--color-text); }
  input:not([type='checkbox']), select { box-sizing: border-box; width: 100%; height: 42px; padding: 0 12px; border: 1px solid var(--color-line-subtle); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; font-size: 14px; font-weight: 400; }
  input[type='file'] { display: flex; align-items: center; height: auto; min-height: 42px; padding: 5px 8px; font-size: 13px; color: var(--color-muted); }
  input[type='file']::file-selector-button { height: 32px; margin-right: 12px; padding: 0 12px; border: 1px solid var(--color-line); border-radius: var(--radius-xs); background: transparent; color: var(--color-text); font: inherit; cursor: pointer; }
  input:not([type='checkbox']):focus-visible, select:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
  .collection-picker { max-width: 520px; margin-top: 8px; }
  /* Boutons : primaire = action principale de la zone, le reste ghost */
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 40px; padding: 8px 18px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: transparent; color: var(--color-text); font: inherit; font-size: 14px; font-weight: 500; cursor: pointer; transition: background-color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-raised); }
  button:disabled { opacity: 0.55; cursor: wait; }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .primary { border: 0; background: var(--color-play); color: var(--color-on-play); font-weight: 600; }
  .primary:hover:not(:disabled) { background: var(--color-play-hover); }
  .danger { color: var(--status-error); }
  /* Galerie : l'image est la carte */
  .gallery { display: grid; grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); gap: 16px 12px; }
  .gallery article { display: grid; justify-items: center; align-content: start; gap: 8px; min-width: 0; }
  .gallery img { width: 100%; height: auto; aspect-ratio: 1; object-fit: cover; border-radius: var(--radius-sm); background: var(--color-raised); }
  .gallery span { max-width: 100%; font-size: 12px; text-align: center; overflow-wrap: anywhere; }
  .gallery button { width: 100%; min-height: 34px; padding: 4px 10px; font-size: 12px; border-color: transparent; }
  [role='alert'] { color: var(--status-error); }
  /* Dans la carte d'une extension : pas de panneau dans un panneau */
  .embedded fieldset, .embedded .upload { padding-inline: 0; border-radius: 0; background: transparent; }
  @media (max-width: 640px) {
    form { display: grid; grid-template-columns: minmax(0, 1fr); align-items: stretch; }
    label { min-width: 0; }
    button, fieldset > button, .avatar-collections > button { width: 100%; justify-self: stretch; }
    .gallery { grid-template-columns: repeat(auto-fill, minmax(100px, 1fr)); }
    input:not([type='checkbox']), select { font-size: 16px; }
  }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
</style>
