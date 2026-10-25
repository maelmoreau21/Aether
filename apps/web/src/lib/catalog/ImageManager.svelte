<script lang="ts">
  import { preferences } from '../preferences';
  import Icon from '../ui/Icon.svelte';
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import type { Library } from '../admin/types';
  type Entry={id:string;kind:string;url:string;origin:string;width?:number|null;height?:number|null;language?:string|null;provider?:string|null};
  let {title,workId,library,onclose,onchanged=async()=>{}}:{title:string;workId?:string;library?:Library;onclose:()=>void;onchanged?:()=>Promise<void>}=$props();
  const labels:Record<string,string>={poster:'Principal',thumb:'Vignette',logo:'Logo',illustration:'Illustration',banner:'Bannière',disc:'Disque',landscape:'Paysage',backdrop:'Arrière-plan'};
  let dialog:HTMLDialogElement, preview:HTMLDialogElement;
  let entries=$state<Entry[]>([]),selected=$state<Entry|null>(null),kind=$state('poster'),replacing=$state<Entry|null>(null);
  let error=$state(''),busy=$state(false),loading=$state(true),adding=$state(false);
  let fileInput=$state<HTMLInputElement>();
  let imageLanguages=$state(preferences().locale==='en'?'en, fr':'fr, en'),includeNeutral=$state(true),collectionMessage=$state('');
  const kinds=$derived(library?['poster','backdrop']:Object.keys(labels));
  const base=$derived(workId?`/api/v1/works/${encodeURIComponent(workId)}/images`:`/api/v1/admin/libraries/${encodeURIComponent(library?.id??'')}/images`);
  const imageUrl=(entry:Entry,small=true)=>workId?`${entry.url}${entry.url.includes('?')?'&':'?'}width=${small?320:1280}`:entry.url;
  async function load(){
    if(library){entries=(library.imageKinds??[]).map(kind=>({id:kind,kind,url:`${base}/${kind}?v=${library?.imageVersions?.[kind]??''}`,origin:'manual'}));loading=false;return;}
    const response=await fetch(base,{cache:'no-store',signal:AbortSignal.timeout(8000)});if(!response.ok)throw new Error('Images indisponibles.');const incoming:Entry[]=(await response.json()).images;entries=incoming.map(entry=>{const previous=entries.find(old=>old.id===entry.id);return {...entry,width:entry.width??previous?.width,height:entry.height??previous?.height};});loading=false;
  }
  onMount(()=>{dialog.showModal();void load().catch(cause=>{error=cause.message;loading=false;});});
  function add(type='poster',entry:Entry|null=null){kind=type;replacing=entry;adding=true;}
  async function upload(event:Event){
    const file=(event.currentTarget as HTMLInputElement).files?.[0];if(!file||busy)return;
    if(!['image/jpeg','image/png','image/webp'].includes(file.type)||file.size>2*1024*1024){error='Choisissez une image PNG, JPEG ou WebP de 2 Mio maximum.';if(fileInput)fileInput.value='';return;}
    busy=true;error='';
    try{
      const response=await fetch(`${base}/${kind}${workId&&replacing?`?replace=${encodeURIComponent(replacing.id)}`:''}`,{method:workId?'POST':'PUT',headers:{'x-csrf-token':$session?.csrfToken??'','content-type':file.type},body:file,signal:AbortSignal.timeout(20000)});
      if(!response.ok)throw new Error(response.status===422?'Image trop grande, corrompue ou dimensions non acceptées (4096 px maximum).':`Image non enregistrée (HTTP ${response.status}).`);
      if(library){entries=[...entries.filter(e=>e.kind!==kind),{id:kind,kind,url:`${base}/${kind}?v=${crypto.randomUUID()}`,origin:'manual'}];}else await load();
      adding=false;replacing=null;await onchanged();
    }catch(cause){error=cause instanceof Error?cause.message:'Erreur';}finally{busy=false;if(fileInput)fileInput.value='';}
  }
  async function remove(entry:Entry){
    if(busy)return;busy=true;error='';
    try{const response=await fetch(`${base}/${entry.id}`,{method:'DELETE',headers:{'x-csrf-token':$session?.csrfToken??''},signal:AbortSignal.timeout(8000)});if(!response.ok)throw new Error('Image non retirée.');if(library)entries=entries.filter(e=>e.id!==entry.id);else await load();await onchanged();}catch(cause){error=cause instanceof Error?cause.message:'Erreur';}finally{busy=false;}
  }
  async function move(entry:Entry,direction:number){
    const images=entries.filter(e=>e.kind==='backdrop');const index=images.findIndex(e=>e.id===entry.id),next=index+direction;
    if(busy||next<0||next>=images.length)return;
    [images[index],images[next]]=[images[next],images[index]];busy=true;error='';
    try{const response=await fetch(`${base}/order`,{method:'PUT',headers:{'content-type':'application/json','x-csrf-token':$session?.csrfToken??''},body:JSON.stringify({ids:images.map(e=>e.id)}),signal:AbortSignal.timeout(8000)});if(!response.ok)throw new Error('Ordre non enregistré.');entries=[...entries.filter(e=>e.kind!=='backdrop'),...images];await onchanged();}catch(cause){error=cause instanceof Error?cause.message:'Erreur';}finally{busy=false;}
  }
  function dimensions(event:Event,entry:Entry){if(!entry.width){const image=event.currentTarget as HTMLImageElement;entry.width=image.naturalWidth;entry.height=image.naturalHeight;}}
  function provenance(entry:Entry){
    if(entry.origin==='manual'||entry.origin==='legacy')return 'Image manuelle · prioritaire';
    if(entry.origin==='local')return 'Image locale';
    return [entry.language||'Langue non déclarée',entry.provider||'Fournisseur'].join(' · ');
  }
  async function collect(){
    if(busy||!workId)return;
    const languages=[...new Set(imageLanguages.split(',').map(language=>language.trim().toLowerCase()).filter(Boolean))];
    if(!languages.length||languages.length>5||languages.some(language=>!/^[a-z]{2,3}(?:-[a-z0-9]{2,8})*$/.test(language))){error='Indiquez une à cinq langues, dans l’ordre de préférence (ex. fr, en).';return;}
    busy=true;error='';collectionMessage='Recherche et téléchargement auprès des fournisseurs activés…';
    try{
      const response=await fetch('/api/v1/admin/extensions/images/collect',{method:'POST',headers:{'content-type':'application/json','x-csrf-token':$session?.csrfToken??''},body:JSON.stringify({workId,languages,includeNeutral,maxImages:12}),signal:AbortSignal.timeout(70000)});
      if(!response.ok)throw new Error(`Collecte indisponible (HTTP ${response.status}). Vérifiez les fournisseurs activés et les identifiants du média.`);
      const result=await response.json();
      collectionMessage=`${result.downloaded??0} images téléchargées · ${result.duplicates??0} doublons évités · ${result.skipped??0} ignorées · ${Array.isArray(result.errors)?result.errors.length:result.errors??0} erreurs.${result.partial?' Collecte partielle : relancez pour continuer.':''}`;
      await load();await onchanged();
    }catch(cause){collectionMessage='';error=cause instanceof Error?cause.message:'Collecte indisponible.';}finally{busy=false;}
  }
</script>
<dialog bind:this={dialog} class="image-manager" aria-labelledby="image-manager-title" onclose={onclose} oncancel={event=>{if(busy)event.preventDefault();}}>
  <header><button type="button" aria-label="Fermer l’éditeur d’images" disabled={busy} onclick={()=>dialog.close()}>←</button><h2 id="image-manager-title">Modifier les images · {title}</h2></header>
  <div class="manager-content">
    {#if error}<p role="alert">{error}</p>{/if}
    {#if loading}<p role="status">Chargement des images…</p>{/if}
    {#if workId && $session?.user.role==='administrator'}
      <section class="collection-panel" aria-label="Collecte d’images">
        <h3>Enrichir les images</h3>
        <p>Les fournisseurs activés recherchent les images à partir des identifiants de ce média. Les doublons visuels sont écartés. Les variantes changent à chaque nouvelle visite dans un onglet ; une image ajoutée manuellement reste prioritaire.</p>
        <label>Langues et secours, dans l’ordre<input bind:value={imageLanguages} disabled={busy} placeholder="fr, en" maxlength="100" /></label>
        <label class="neutral-language"><input type="checkbox" bind:checked={includeNeutral} disabled={busy} />Inclure les images sans langue déclarée</label>
        <button type="button" disabled={busy||loading} onclick={collect}>Collecter les images</button>
        {#if collectionMessage}<p role="status">{collectionMessage}</p>{/if}
      </section>
    {/if}
    {#if adding}<form class="upload-panel" onsubmit={event=>event.preventDefault()}><fieldset disabled={busy}>
      <legend>{replacing?'Remplacer une image':'Ajouter une image'}</legend>
      <label for="artwork-type">Type d’image</label><select id="artwork-type" bind:value={kind} disabled={!!replacing}>{#each kinds as type(type)}<option value={type}>{labels[type]}</option>{/each}</select>
      <label for="artwork-file">PNG, JPEG ou WebP · 2 Mio maximum</label><input id="artwork-file" bind:this={fileInput} type="file" accept="image/png,image/jpeg,image/webp" onchange={upload} />
      <p>Les images sont conservées dans les données applicatives. Les fichiers de vos médias restent intacts.</p><button type="button" onclick={()=>adding=false}>Annuler</button>
    </fieldset></form>{/if}
    {#each [false,true] as backdrops(backdrops)}
      <section aria-label={backdrops?'Arrière-plans':'Images'}>
        <div class="section-heading"><h3>{backdrops?'Arrière-plans':'Images'}</h3><button class="round" type="button" disabled={busy} aria-label={backdrops?'Ajouter un arrière-plan':'Ajouter une image'} onclick={()=>add(backdrops?'backdrop':'poster')}>+</button></div>
        <div class="image-grid">
          {#each entries.filter(e=>(e.kind==='backdrop')===backdrops).sort((a,b)=>kinds.indexOf(a.kind)-kinds.indexOf(b.kind)) as entry(entry.id)}
            <article class="image-card" data-kind={entry.kind}>
              <button class="image-preview" type="button" aria-label={`Agrandir ${labels[entry.kind]}`} onclick={()=>{selected=entry;preview.showModal();}}><img src={imageUrl(entry)} alt={`${labels[entry.kind]} de ${title}`} loading="lazy" decoding="async" onload={event=>dimensions(event,entry)} /></button>
              <div class="card-caption"><h4>{labels[entry.kind]}</h4><p>{provenance(entry)}</p>{#if entry.width&&entry.height}<p>{entry.width} × {entry.height}</p>{/if}
                <div class="card-actions"><button type="button" aria-label={`Agrandir ${labels[entry.kind]}`} title="Agrandir" onclick={()=>{selected=entry;preview.showModal();}}><Icon name="search" size={18} /></button><button type="button" disabled={busy} aria-label={`Remplacer ${labels[entry.kind]}`} title="Remplacer" onclick={()=>add(entry.kind,entry)}>↥</button>
                  {#if backdrops&&workId}<button type="button" disabled={busy||entries.filter(e=>e.kind==='backdrop')[0]?.id===entry.id} aria-label="Déplacer l’arrière-plan à gauche" onclick={()=>move(entry,-1)}>‹</button><button type="button" disabled={busy||entries.filter(e=>e.kind==='backdrop').at(-1)?.id===entry.id} aria-label="Déplacer l’arrière-plan à droite" onclick={()=>move(entry,1)}>›</button>{/if}
                  <button type="button" class="danger" disabled={busy} aria-label={`Supprimer ${labels[entry.kind]}`} title={entry.origin==='local'?'Masquer cette image dans le catalogue, sans supprimer le fichier':'Supprimer cette image personnalisée'} onclick={()=>remove(entry)}><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 6h16M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7m4-7v7" /></svg></button>
                </div>
              </div>
            </article>
          {:else}<p class="empty">{loading?'Chargement…':'Aucune image. Utilisez + pour en ajouter.'}</p>{/each}
        </div>
      </section>
    {/each}
    {#if busy}<p role="status">Enregistrement…</p>{/if}
  </div>
</dialog>
<dialog class="zoom" bind:this={preview} aria-label="Aperçu de l’image"><button type="button" onclick={()=>preview.close()}>Fermer</button>{#if selected}<img src={imageUrl(selected,false)} alt={`${labels[selected.kind]} de ${title}`} />{/if}</dialog>
<style>
  .collection-panel { display:grid;gap:12px;padding:18px;background:var(--color-raised);border-radius:var(--radius-md); } .collection-panel p { margin:0; } .collection-panel label { display:grid;gap:6px; } .collection-panel .neutral-language { display:flex;align-items:center;gap:10px; } .neutral-language input { min-height:20px;width:20px; } .collection-panel button { justify-self:start;padding:8px 14px; }
  .image-manager { width:min(1500px,calc(100vw - 24px));max-width:none;max-height:calc(100dvh - 24px);padding:0;border:1px solid var(--color-line);border-radius:14px;background:var(--color-surface);color:var(--color-text); }
  dialog::backdrop { background:rgb(0 0 0 / .8); } header { position:sticky;top:0;display:flex;align-items:center;gap:14px;padding:12px 18px;background:var(--color-surface);border-bottom:1px solid var(--color-line);z-index:2; } h2 { font-size:17px;font-weight:600;margin:0;overflow-wrap:anywhere; } .manager-content { padding:24px; } section { margin-bottom:36px; } .section-heading { display:flex;align-items:center;gap:14px;margin-bottom:22px; } h3 { font-size:18px;margin:0; } button { color:var(--color-text);background:transparent;border:1px solid var(--color-line);border-radius:6px;min-width:40px;min-height:40px;font:inherit;cursor:pointer; } button:disabled { opacity:.4;cursor:default; } button:hover:not(:disabled) { background:var(--color-raised); } .round { border-radius:50%;font-size:26px; }
  .image-grid { display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,270px),1fr));gap:24px; } .image-card { border:1px solid var(--color-line);border-radius:12px;overflow:hidden;min-width:0; } .image-preview { display:block;width:100%;aspect-ratio:16/9;border:0;border-radius:0;padding:0;background:var(--color-bg); } .image-preview img { width:100%;height:100%;object-fit:contain; } .card-caption { text-align:center;padding:10px; } h4 { font-size:16px;font-weight:500;margin:0; } p { color:var(--color-muted);font-size:13px;line-height:1.5; } .card-caption p { margin:4px 0; } .card-actions { display:flex;justify-content:center;gap:4px; } .card-actions button { border:0;font-size:22px; } .card-actions .danger:hover { color:var(--color-danger,var(--color-text)); } .upload-panel { background:var(--color-raised);border-radius:var(--radius-md);margin-bottom:26px;padding:18px; } fieldset { min-width:0;display:grid;gap:12px;border:0;margin:0;padding:0; } input,select { max-width:100%;min-height:44px;color:var(--color-text);background:var(--color-surface);border:1px solid var(--color-line);border-radius:6px;padding:8px; } .zoom { max-width:calc(100vw - 24px);max-height:calc(100dvh - 24px);background:var(--color-surface);color:var(--color-text);border:1px solid var(--color-line);border-radius:var(--radius-md); } .zoom img { display:block;max-width:100%;max-height:80dvh;object-fit:contain;margin-top:12px; }
  @media(max-width:600px) { .manager-content { padding:16px; } .image-grid { grid-template-columns:repeat(auto-fill,minmax(min(100%,220px),1fr)); } }
</style>
