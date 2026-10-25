<script lang="ts">
  import { adminRequest } from '../admin/request';
  import { session } from '../auth/session';
  import { avatarRevision } from '../auth/avatar';
  let busy = $state(false), error = $state(''), notice = $state('');
  let uploadAllowed=$state(false),removeAllowed=$state(false),showGallery=$state(false);
  let fileInput = $state<HTMLInputElement>();
  let gravatarAllowed=$state(false),gravatarEmail=$state('');
  async function importGravatar(){busy=true;error='';notice='';try{await adminRequest('/api/v1/account/avatar/gravatar',{method:'POST',body:JSON.stringify({email:gravatarEmail})});avatarRevision.update(value=>value+1);notice='Gravatar importé.';gravatarEmail='';}catch(e){error=e instanceof Error?e.message:'Import impossible.';}finally{busy=false;}}
  let collections=$state<{id:string;name:string;count:number}[]>([]),collectionId=$state('');
  let extensions=$state<{id:string;name:string}[]>([]), proposals=$state<{extension:string;pack:string;seed:string;url:string}[]>([]);
  async function choosePhotos() {
    showGallery=!showGallery;
    if(!showGallery || proposals.length || !extensions.length)return;
    busy=true;error='';
    try {
      const images=[];
      for(const extension of extensions){
        const listed=await adminRequest<{result:{id:string;name:string}[]}>(`/api/v1/account/avatar/extensions/${encodeURIComponent(extension.id)}`,{method:'POST',body:JSON.stringify({operation:'avatars.list'})});
        for(const pack of listed.result.slice(0,8)) {
          const seed=crypto.randomUUID();
          const generated=await adminRequest<{result:{svg:string}}>(`/api/v1/account/avatar/extensions/${encodeURIComponent(extension.id)}`,{method:'POST',body:JSON.stringify({operation:'avatars.generate',pack:pack.id,seed})});
          images.push({extension:extension.id,pack:pack.id,seed,url:`data:image/svg+xml;charset=utf-8,${encodeURIComponent(generated.result.svg)}`});
        }
      }
      proposals=images;
    }catch(cause){error=cause instanceof Error?cause.message:'Photos indisponibles.';}finally{busy=false;}
  }
  async function applyProposal(item:{extension:string;pack:string;seed:string}){
    busy=true;error='';
    try{await adminRequest(`/api/v1/account/avatar/extensions/${encodeURIComponent(item.extension)}`,{method:'POST',body:JSON.stringify({operation:'avatars.generate',pack:item.pack,seed:item.seed,apply:true})});avatarRevision.update(v=>v+1);notice='Image de profil enregistrée.';}catch(cause){error=cause instanceof Error?cause.message:'Modification impossible.';}finally{busy=false;}
  }
  let collectionImages=$state<{id:string;name:string;url:string}[]>([]);
  async function gallery(path:string,method='GET') {
    const current=$session;if(!current)return;
    busy=true;error='';
    try{
      const response=await fetch(path,{method,credentials:'same-origin',cache:'no-store',headers:{'x-csrf-token':current.csrfToken},signal:AbortSignal.timeout(20000)});
      if(!response.ok)throw new Error('Collection indisponible.');
      if($session?.user.id!==current.user.id)return;
      if(method==='POST'){avatarRevision.update(value=>value+1);notice='Avatar enregistré.';}
      else collectionImages=(await response.json()).images;
    }catch(e){error=e instanceof Error?e.message:'Collection indisponible.';}finally{busy=false;}
  }
  $effect(() => {
    const userId = $session?.user.id;
    uploadAllowed=false;removeAllowed=false;showGallery=false;
    extensions=[];proposals=[];
    collections=[];collectionImages=[];collectionId='';
    gravatarAllowed=false;gravatarEmail='';
    if (!userId) return;
    const controller = new AbortController();
    void fetch('/api/v1/account/avatar/extensions',{credentials:'same-origin',cache:'no-store',signal:controller.signal}).then(r=>r.ok?r.json():{extensions:[]}).then(v=>{if($session?.user.id===userId)extensions=v.extensions;}).catch(()=>{});
    void fetch('/api/v1/account/avatar/policy',{credentials:'same-origin',cache:'no-store',signal:controller.signal}).then(r=>{if(!r.ok)throw new Error();return r.json();}).then(v=>{if($session?.user.id===userId){uploadAllowed=v.uploadAllowed;removeAllowed=v.removeAllowed;}}).catch(()=>{error='Impossible de charger les autorisations de l’image de profil.';});
    void fetch('/api/v1/account/avatar/gravatar',{credentials:'same-origin',cache:'no-store',signal:controller.signal}).then(async response=>response.ok?response.json():{allowed:false}).then(value=>{if($session?.user.id===userId)gravatarAllowed=value.allowed;}).catch(()=>{});
    void fetch('/api/v1/account/avatar/collections',{credentials:'same-origin',cache:'no-store',signal:controller.signal})
      .then(async response=>response.ok?response.json():{packs:[]})
      .then(value=>{if($session?.user.id===userId){collections=value.packs;collectionId=collections[0]?.id??'';if(collectionId)void gallery(`/api/v1/account/avatar/collections/${encodeURIComponent(collectionId)}`);}}).catch(()=>{});

    return () => controller.abort();
  });
  async function update(file?: File) {
    const current = $session; if (!current || busy) return;
    error = ''; notice = '';
    if (file && (file.size > 2 * 1024 * 1024 || !['image/png', 'image/jpeg', 'image/webp'].includes(file.type))) { error = 'Choisissez une image PNG, JPEG ou WebP de 2 Mo maximum.'; return; }
    busy = true;
    try {
      const response = await fetch('/api/v1/account/avatar', { method: file ? 'PUT' : 'DELETE', credentials: 'same-origin', signal: AbortSignal.timeout(20000), headers: { 'x-csrf-token': current.csrfToken, ...(file ? { 'content-type': file.type } : {}) }, ...(file ? { body: file } : {}) });
      if (!response.ok) throw new Error(response.status === 413 ? 'Image trop volumineuse.' : 'Image refusée. Vérifiez son format et vos droits.');
      if ($session?.user.id !== current.user.id) return;
      avatarRevision.update(value => value + 1); notice = file ? 'Image de profil enregistrée.' : 'Image de profil supprimée.';
    } catch (cause) { if ($session?.user.id === current.user.id) error = cause instanceof Error ? cause.message : 'Modification impossible.'; }
    finally { busy = false; }
  }
</script>
<div class="avatar-editor">
  <div class="avatar-actions">
    {#if uploadAllowed}<input class="file-input" bind:this={fileInput} type="file" accept="image/png,image/jpeg,image/webp" aria-label="Importer une image de profil" disabled={busy} onchange={(event) => { const file=event.currentTarget.files?.[0];if(file)void update(file);event.currentTarget.value=''; }} /><button disabled={busy} onclick={()=>fileInput?.click()}>Importer une image</button>{/if}
    {#if removeAllowed}<button disabled={busy} onclick={()=>void update()}>Supprimer l’image de profil</button>{/if}
    {#if collections.length || extensions.length}<button disabled={busy} aria-expanded={showGallery} onclick={()=>void choosePhotos()}>Choisir une photo de profil</button>{/if}
  </div>
  {#if showGallery && proposals.length}<div class="avatar-gallery">{#each proposals as item (item.seed)}<button disabled={busy} aria-label={`Choisir la photo ${item.pack}`} onclick={()=>void applyProposal(item)}><img src={item.url} alt={`Photo ${item.pack}`} width="80" height="80" /></button>{/each}</div>{/if}
  {#if showGallery && collections.length}
    <label>Collections du serveur<select disabled={busy} bind:value={collectionId} onchange={()=>{collectionImages=[];if(collectionId)void gallery(`/api/v1/account/avatar/collections/${encodeURIComponent(collectionId)}`);}}><option value="">Choisir une collection</option>{#each collections as collection (collection.id)}<option value={collection.id}>{collection.name} · {collection.count}</option>{/each}</select></label>
    <div class="avatar-gallery">{#each collectionImages as item (item.id)}<button type="button" disabled={busy} aria-label={`Choisir ${item.name}`} onclick={()=>void gallery(item.url,'POST')}><img src={item.url} alt={item.name} width="80" height="80" loading="lazy" /></button>{/each}</div>
  {/if}
  {#if showGallery && gravatarAllowed}<form onsubmit={event=>{event.preventDefault();void importGravatar();}}><label>Adresse e-mail Gravatar<input type="email" required maxlength="254" bind:value={gravatarEmail} disabled={busy} /></label><button disabled={busy}>Importer mon Gravatar</button><p>L’adresse n’est pas conservée. Son empreinte est envoyée à Gravatar pour télécharger votre image.</p></form>{/if}
  {#if busy}<p role="status">Enregistrement…</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}{#if error}<p role="alert">{error}</p>{/if}
</div>
<style>
  .file-input { display:none; }.avatar-actions { display:flex;flex-wrap:wrap;gap:10px;margin-bottom:16px; }
  .avatar-gallery{display:grid;grid-template-columns:repeat(auto-fill,minmax(84px,1fr));gap:10px;max-height:420px;overflow:auto;margin:16px 0;}.avatar-gallery button{padding:4px;}.avatar-gallery img{width:100%;height:auto;aspect-ratio:1;object-fit:cover;border-radius:6px;}
  .avatar-editor { margin-bottom: 28px; padding-bottom: 24px; border-bottom: 1px solid var(--border-subtle); }
  label { display: grid; gap: 12px; font-size: 14px; }
  input { max-width: 100%; font: inherit; color: var(--text-primary); }
  p { font-size: 13px; line-height: 1.6; color: var(--text-secondary); }
  button { min-height: 44px; padding: 10px 16px; border: 1px solid var(--border-subtle); background: var(--bg-surface); color: var(--text-primary); border-radius: 4px; }
  [role='alert'] { color: var(--status-error); }
</style>
