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
  let uploadAllowed = $state(false), removeAllowed = $state(false);
  let showModal = $state(false);
  let fileInput = $state<HTMLInputElement>();
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
      avatarRevision.update((v) => v + 1);
      notice = 'Photo de profil enregistrée.';
      dialog?.close();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Modification impossible.';
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    const userId = $session?.user.id;
    uploadAllowed = false;
    removeAllowed = false;
    showModal = false;
    dialog?.close();
    collections = [];
    selectedPack = '';
    allImages = [];
    selectedItem = null;
    if (!userId) return;
    const controller = new AbortController();

    void fetch('/api/v1/account/avatar/policy', {
      credentials: 'same-origin',
      cache: 'no-store',
      signal: controller.signal
    })
      .then((r) => {
        if (!r.ok) throw new Error();
        return r.json();
      })
      .then((v) => {
        if ($session?.user.id === userId) {
          uploadAllowed = v.uploadAllowed;
          removeAllowed = v.removeAllowed;
        }
      })
      .catch(() => {
        if (!controller.signal.aborted) error = 'Impossible de charger les autorisations de l’image de profil.';
      });

    return () => controller.abort();
  });

  async function update(file?: File) {
    const current = $session;
    if (!current || busy) return;
    error = '';
    notice = '';
    if (file && (file.size > 2 * 1024 * 1024 || !['image/png', 'image/jpeg', 'image/webp'].includes(file.type))) {
      error = 'Choisissez une image PNG, JPEG ou WebP de 2 Mo maximum.';
      return;
    }
    busy = true;
    try {
      const response = await fetch('/api/v1/account/avatar', {
        method: file ? 'PUT' : 'DELETE',
        credentials: 'same-origin',
        signal: AbortSignal.timeout(20000),
        headers: {
          'x-csrf-token': current.csrfToken,
          ...(file ? { 'content-type': file.type } : {})
        },
        ...(file ? { body: file } : {})
      });
      if (!response.ok)
        throw new Error(response.status === 413 ? 'Image trop volumineuse.' : 'Image refusée. Vérifiez son format et vos droits.');
      if ($session?.user.id !== current.user.id) return;
      avatarRevision.update((value) => value + 1);
      notice = file ? 'Image de profil enregistrée.' : 'Image de profil supprimée.';
    } catch (cause) {
      if ($session?.user.id === current.user.id)
        error = cause instanceof Error ? cause.message : 'Modification impossible.';
    } finally {
      busy = false;
    }
  }
</script>

<div class="avatar-editor">
  <div class="avatar-actions">
    {#if uploadAllowed}
      <input
        class="file-input"
        bind:this={fileInput}
        type="file"
        accept="image/png,image/jpeg,image/webp"
        aria-label="Importer une image de profil"
        disabled={busy}
        onchange={(event) => {
          const file = event.currentTarget.files?.[0];
          if (file) void update(file);
          event.currentTarget.value = '';
        }}
      />
      <button disabled={busy} onclick={() => fileInput?.click()}>Importer une image</button>
    {/if}
    {#if removeAllowed}
      <button disabled={busy} onclick={() => void update()}>Supprimer l’image de profil</button>
    {/if}
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
  .file-input {
    display: none;
  }
  .avatar-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-bottom: 16px;
  }
  .avatar-editor {
    margin-bottom: 28px;
    padding-bottom: 24px;
    border-bottom: 1px solid var(--border-subtle);
  }
  input {
    max-width: 100%;
    font: inherit;
    color: var(--text-primary);
  }
  p {
    font-size: 13px;
    line-height: 1.6;
    color: var(--text-secondary);
  }
  button {
    min-height: 44px;
    padding: 10px 16px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-surface);
    color: var(--text-primary);
    border-radius: var(--radius-sm, 6px);
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  [role='alert'] {
    color: var(--status-error);
  }

  /* Modal overlay */
  .avatar-modal::backdrop {
    background: rgba(0, 0, 0, 0.7);
  }
  .avatar-modal {
    background: var(--bg-elevated, #1f1f23);
    color: var(--text-primary);
    border: 1px solid var(--border-subtle, #333);
    border-radius: var(--radius-lg, 12px);
    width: calc(100% - 32px);
    max-width: 640px;
    max-height: 85vh;
    padding: 0;
    flex-direction: column;
    box-shadow: 0 16px 36px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }
  .avatar-modal[open] { display: flex; }
  .pack-picker { display:grid; gap:8px; margin-bottom:16px; }
  .pack-picker select { width:100%; min-height:44px; padding:8px; color:var(--text-primary); background:var(--bg-surface); border:1px solid var(--border-subtle); border-radius:var(--radius-sm); font:inherit; }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 18px 24px;
    border-bottom: 1px solid var(--border-subtle, #333);
  }
  .modal-header h3 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
  }
  .close-btn {
    border: none;
    background: transparent;
    padding: 6px;
    min-height: auto;
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .close-btn:hover {
    background: var(--bg-surface, #2a2a2e);
  }
  .modal-body {
    padding: 20px 24px;
    overflow-y: auto;
    flex: 1;
    min-height: 200px;
  }
  .loading-state, .empty-state {
    text-align: center;
    padding: 40px 0;
  }
  .avatar-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(100px, 1fr));
    gap: 16px;
  }
  .avatar-option {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 8px;
    border: 2px solid transparent;
    border-radius: var(--radius-md, 8px);
    background: var(--bg-surface, #27272a);
    transition: border-color 0.15s ease, transform 0.15s ease;
  }
  .avatar-option:hover {
    border-color: var(--border-subtle, #52525b);
    transform: translateY(-2px);
  }
  .avatar-option.selected {
    border-color: var(--brand-primary, #6366f1);
    background: var(--bg-base, #18181b);
    outline: 2px solid var(--brand-primary, #6366f1);
  }
  .avatar-option img {
    width: 88px;
    height: 88px;
    aspect-ratio: 1;
    object-fit: cover;
    border-radius: 50%;
  }
  .item-label {
    font-size: 11px;
    color: var(--text-secondary);
    max-width: 90px;
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    padding: 16px 24px;
    border-top: 1px solid var(--border-subtle, #333);
    background: var(--bg-surface, #18181b);
  }
  .cancel-btn {
    background: transparent;
    border: 1px solid var(--border-subtle, #444);
  }
  .cancel-btn:hover {
    background: var(--bg-elevated, #27272a);
  }
  .confirm-btn {
    background: var(--brand-primary, #4f46e5);
    color: #fff;
    border: none;
    font-weight: 500;
  }
  .confirm-btn:hover:not(:disabled) {
    background: var(--brand-primary-hover, #4338ca);
  }
</style>
