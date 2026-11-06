<script lang="ts">
  import { session } from '../auth/session';
  import { avatarRevision } from '../auth/avatar';
  import Icon from '../ui/Icon.svelte';

  type AvatarItem = {
    id: string;
    name: string;
    url: string;
  };

  let busy = $state(false), error = $state(''), notice = $state('');
  let permissions = $state({uploadAllowed:false,removeAllowed:false});
  let uploadInput = $state<HTMLInputElement>();
  async function loadPermissions(userId:string,signal:AbortSignal){
    try{const response=await fetch('/api/v1/account/avatar/policy',{credentials:'same-origin',cache:'no-store',signal});if(!response.ok)throw new Error('Paramètres des avatars indisponibles.');const data=await response.json();if($session?.user.id===userId&&!signal.aborted)permissions=data;}catch(cause){if(!signal.aborted)error=cause instanceof Error?cause.message:'Paramètres indisponibles.';}
  }
  async function changePersonal(file?:File){
    const current=$session;if(!current)return;
    if(file&&(!['image/png','image/jpeg','image/webp'].includes(file.type)||file.size>2*1024*1024)){error='Choisissez une image PNG, JPEG ou WebP de 2 Mo maximum.';return;}
    busy=true;error='';notice='';
    try{const response=await fetch('/api/v1/account/avatar',{method:file?'PUT':'DELETE',credentials:'same-origin',headers:{'x-csrf-token':current.csrfToken,...(file?{'Content-Type':file.type}:{})},body:file,signal:AbortSignal.timeout(20000)});if(!response.ok)throw new Error('Modification impossible. Vérifiez les autorisations et le format de l’image.');if($session?.user.id===current.user.id&&$session.csrfToken===current.csrfToken){avatarRevision.update(v=>v+1);notice=file?'Photo de profil importée.':'Photo de profil supprimée.';}}catch(cause){if($session?.user.id===current.user.id)error=cause instanceof Error?cause.message:'Modification impossible.';}finally{busy=false;if(uploadInput)uploadInput.value='';}
  }
  let showModal = $state(false);
  let dialog = $state<HTMLDialogElement>();
  let galleryButton = $state<HTMLButtonElement>();

  let collections = $state<{ id: string; name: string; count: number }[]>([]);
  let selectedPack = $state('');
  let allImages = $state<AvatarItem[]>([]);
  let selectedItem = $state<AvatarItem | null>(null);
  let modalLoading = $state(false);
  let requestGeneration = 0;

  async function openModal() {
    const userId = $session?.user.id;
    if (!userId) return;
    const generation = ++requestGeneration;
    showModal = true;
    dialog?.showModal();
    error = '';
    notice = '';
    selectedItem = null;
    modalLoading = true;
    try {
      const response = await fetch('/api/v1/account/avatar/collections', {
        credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(20000)
      });
      if (!response.ok) throw new Error('Impossible de charger la galerie des avatars.');
      const data = await response.json() as { packs: typeof collections };
      if ($session?.user.id !== userId || !dialog?.open || generation !== requestGeneration) return;
      collections = data.packs;
      selectedPack = collections[0]?.id ?? '';
      if (selectedPack) await loadPack(selectedPack);
      else allImages = [];
    } catch (cause) {
      if ($session?.user.id === userId && dialog?.open && generation === requestGeneration) error = cause instanceof Error ? cause.message : 'Photos indisponibles.';
    } finally {
      if (generation === requestGeneration) modalLoading = false;
    }
  }

  async function loadPack(pack: string) {
    const userId = $session?.user.id;
    const generation = ++requestGeneration;
    selectedPack = pack; selectedItem = null; allImages = []; error = ''; modalLoading = true;
    try {
      const response = await fetch(`/api/v1/account/avatar/collections/${encodeURIComponent(pack)}`, {
        credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(20000)
      });
      if (!response.ok) throw new Error('Impossible de charger cette collection.');
      const data = await response.json() as { images: { id: string; name: string; url: string }[] };
      if ($session?.user.id === userId && generation === requestGeneration && dialog?.open)
        allImages = data.images.map(img => ({ id: `${pack}-${img.id}`, name: img.name || 'Avatar', url: img.url }));
    } catch (cause) {
      if ($session?.user.id === userId && generation === requestGeneration && dialog?.open)
        error = cause instanceof Error ? cause.message : 'Collection indisponible.';
    } finally { if (generation === requestGeneration) modalLoading = false; }
  }

  function closeModal() {
    if (!busy) dialog?.close();
  }

  async function saveSelected() {
    if (!selectedItem) return;
    const current = $session;
    if (!current) return;
    busy = true;
    error = '';
    notice = '';
    try {
      const response = await fetch(selectedItem.url, {
        method: 'POST',
        credentials: 'same-origin',
        cache: 'no-store',
        headers: { 'x-csrf-token': current.csrfToken },
        signal: AbortSignal.timeout(20000)
      });
      if (!response.ok) throw new Error('Impossible d’enregistrer cet avatar.');
      if ($session?.user.id !== current.user.id || $session.csrfToken !== current.csrfToken) return;
      avatarRevision.update((v) => v + 1);
      notice = 'Photo de profil enregistrée.';
      dialog?.close();
    } catch (cause) {
      if ($session?.user.id === current.user.id && $session.csrfToken === current.csrfToken)
        error = cause instanceof Error ? cause.message : 'Modification impossible.';
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    // Session changes invalidate an open selection and every in-flight request.
    const userId = $session?.user.id;
    requestGeneration++;
    showModal = false;
    dialog?.close();
    collections = [];
    selectedPack = '';
    allImages = [];
    selectedItem = null;
    modalLoading = false;
    permissions={uploadAllowed:false,removeAllowed:false};
    const controller=new AbortController();
    if(userId)void loadPermissions(userId,controller.signal);
    if (!userId) { error = ''; notice = ''; }
    return ()=>controller.abort();
  });

</script>

<div class="avatar-editor">
  <p>Choisissez une image dans la galerie du serveur{permissions.uploadAllowed ? ", ou importez votre propre image" : ""}. {permissions.removeAllowed ? "" : "Un avatar est obligatoire."}</p>
  <div class="avatar-actions">
    {#if permissions.uploadAllowed}<input hidden type="file" bind:this={uploadInput} accept="image/png,image/jpeg,image/webp" onchange={event=>{const file=event.currentTarget.files?.[0];if(file)void changePersonal(file);}} /><button disabled={busy} onclick={()=>uploadInput?.click()}>Importer une image</button>{/if}
    {#if permissions.removeAllowed}<button disabled={busy} onclick={()=>void changePersonal()}>Supprimer l’avatar</button>{/if}
    <button bind:this={galleryButton} disabled={busy} aria-expanded={showModal} onclick={() => void openModal()}>
      Choisir dans la galerie
    </button>
  </div>

  {#if busy}
    <p role="status">Enregistrement…</p>
  {/if}
  {#if notice}
    <p role="status">{notice}</p>
  {/if}
  {#if error}
    <p role="alert">{error}</p>
  {/if}

    <dialog
      bind:this={dialog}
      class="avatar-modal"
      aria-labelledby="modal-avatar-title"
      oncancel={(event) => { if (busy) event.preventDefault(); }}
      onclose={() => { requestGeneration++; showModal = false; selectedItem = null; galleryButton?.focus(); }}
    >
        <div class="modal-header">
          <h3 id="modal-avatar-title">Choisir une photo de profil</h3>
          <button type="button" class="close-btn" aria-label="Fermer" onclick={closeModal}>
            <Icon name="close" size={18} />
          </button>
        </div>

        <div class="modal-body">
          {#if collections.length > 1}
            <label class="pack-picker">Collection<select value={selectedPack} onchange={(event) => void loadPack(event.currentTarget.value)}>{#each collections as pack (pack.id)}<option value={pack.id}>{pack.name} · {pack.count}</option>{/each}</select></label>
          {/if}
          {#if error}<p role="alert">{error}</p>{/if}
          {#if modalLoading}
            <p class="loading-state">Chargement des photos…</p>
          {:else if allImages.length === 0}
            <p class="empty-state">Aucune photo de profil disponible pour le moment.</p>
          {:else}
            <div class="avatar-grid">
              {#each allImages as item (item.id)}
                <button
                  type="button"
                  class="avatar-option"
                  class:selected={selectedItem?.id === item.id}
                  aria-pressed={selectedItem?.id === item.id}
                  aria-label={item.name}
                  onclick={() => { selectedItem = item; }}
                >
                  <img src={item.url} alt={item.name} width="88" height="88" loading="lazy" />
                  <span class="item-label">{item.name}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>

        <div class="modal-footer">
          <button type="button" class="cancel-btn" disabled={busy} onclick={closeModal}>
            Annuler
          </button>
          <button
            type="button"
            class="confirm-btn"
            disabled={busy || !selectedItem || modalLoading}
            onclick={() => void saveSelected()}
          >
            {busy ? 'Enregistrement…' : 'Enregistrer'}
          </button>
        </div>
    </dialog>
</div>

<style>
  .avatar-editor {
    margin-bottom: 24px;
    padding-bottom: 24px;
    border-bottom: 1px solid var(--line);
  }
  .avatar-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 12px;
  }
  p {
    margin: 8px 0 0;
    font-size: 14px;
    line-height: 1.6;
    color: var(--mu);
  }
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 18px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--pn-2);
    color: var(--tx);
    font: inherit;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }
  button:hover:not(:disabled) {
    background: var(--pn-3);
  }
  button:active:not(:disabled) {
    transform: scale(0.98);
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  button:focus-visible,
  select:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }
  [role='alert'] {
    padding: 12px 14px;
    border-radius: var(--r-sm);
    background: var(--err-soft);
    color: var(--err);
    font-weight: 600;
  }

  /* Dialogue */
  .avatar-modal::backdrop {
    background: var(--media-shade);
  }
  .avatar-modal {
    background: var(--bg-3);
    color: var(--tx);
    border: 1px solid var(--line);
    border-radius: var(--r-xl);
    width: calc(100% - 32px);
    max-width: 640px;
    max-height: 85vh;
    padding: 0;
    flex-direction: column;
    box-shadow: var(--shadow);
    overflow: hidden;
    overscroll-behavior: contain;
  }
  .avatar-modal[open] { display: flex; }
  .pack-picker {
    display: grid;
    gap: 8px;
    margin-bottom: 16px;
    font-size: 14px;
    font-weight: 700;
  }
  .pack-picker select {
    width: 100%;
    height: 46px;
    min-height: 46px;
    padding: 0 40px 0 14px;
    color: var(--tx);
    background-color: var(--pn);
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    font: inherit;
    font-size: 15px;
    font-weight: 500;
    box-shadow: none;
    transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease;
  }
  .pack-picker select:hover { background-color: var(--pn-2); }
  .pack-picker select:focus { outline: none; border-color: var(--ac); background-color: var(--bg-2); }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 20px 24px 16px;
    border-bottom: 1px solid var(--line);
  }
  .modal-header h3 {
    margin: 0;
    font-size: 24px;
    font-weight: 800;
    letter-spacing: -0.02em;
  }
  .close-btn {
    flex: none;
    width: 44px;
    height: 44px;
    padding: 0;
    border-radius: 14px;
    background: var(--pn);
  }
  .close-btn:hover:not(:disabled) {
    background: var(--pn-2);
  }
  .modal-body {
    padding: 20px 24px;
    overflow-y: auto;
    flex: 1;
    min-height: 200px;
    overscroll-behavior: contain;
  }
  .loading-state, .empty-state {
    text-align: center;
    padding: 40px 0;
  }
  .avatar-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(104px, 1fr));
    gap: 12px;
  }
  .avatar-option {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: auto;
    padding: 10px 8px;
    border-radius: var(--r-md);
    background: var(--pn);
    font-weight: 600;
  }
  .avatar-option:hover:not(:disabled) {
    background: var(--pn-2);
  }
  .avatar-option.selected,
  .avatar-option.selected:hover:not(:disabled) {
    background: var(--ac-soft);
    box-shadow: inset 0 0 0 2px var(--ac);
  }
  .avatar-option img {
    width: 80px;
    height: 80px;
    aspect-ratio: 1;
    object-fit: cover;
    border-radius: 50%;
    background: var(--pn-2);
  }
  .item-label {
    font-size: 13px;
    color: var(--mu);
    max-width: 100%;
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .avatar-option.selected .item-label { color: var(--tx); }
  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 16px 24px;
    border-top: 1px solid var(--line);
  }
  .cancel-btn {
    background: transparent;
    color: var(--mu);
  }
  .cancel-btn:hover:not(:disabled) {
    background: var(--pn);
    color: var(--tx);
  }
  .confirm-btn {
    height: 48px;
    padding: 0 22px;
    background: var(--btn);
    color: var(--btn-tx);
    font-size: 15px;
  }
  .confirm-btn:hover:not(:disabled) {
    background: var(--btn);
    opacity: 0.9;
  }
  @media (max-width: 640px) {
    .avatar-modal {
      margin: auto 0 0;
      width: 100%;
      max-width: none;
      max-height: 88dvh;
      border-radius: var(--r-xl) var(--r-xl) 0 0;
    }
    .modal-header, .modal-body { padding-inline: 16px; }
    .modal-footer { padding: 12px 16px calc(12px + env(safe-area-inset-bottom, 0px)); }
    .avatar-grid { grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
    .avatar-option img { width: 64px; height: 64px; }
  }
</style>
