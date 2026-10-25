<script lang="ts">
  import { adminRequest } from '../admin/request';
  import { session } from '../auth/session';
  import { avatarRevision } from '../auth/avatar';
  import Icon from '../ui/Icon.svelte';

  type AvatarItem = {
    id: string;
    name: string;
    url: string;
    kind: 'extension' | 'collection';
    extension?: string;
    pack?: string;
    seed?: string;
  };

  let busy = $state(false), error = $state(''), notice = $state('');
  let uploadAllowed = $state(false), removeAllowed = $state(false);
  let showModal = $state(false);
  let fileInput = $state<HTMLInputElement>();
  let gravatarAllowed = $state(false), gravatarEmail = $state('');

  let collections = $state<{ id: string; name: string; count: number }[]>([]);
  let extensions = $state<{ id: string; name: string }[]>([]);
  let allImages = $state<AvatarItem[]>([]);
  let selectedItem = $state<AvatarItem | null>(null);
  let modalLoading = $state(false);

  async function openModal() {
    showModal = true;
    error = '';
    notice = '';
    selectedItem = null;
    modalLoading = true;
    try {
      const items: AvatarItem[] = [];

      // 1. Extensions avatars (generate 4 variants per pack)
      for (const extension of extensions) {
        try {
          const listed = await adminRequest<{ result: { id: string; name: string }[] }>(
            `/api/v1/account/avatar/extensions/${encodeURIComponent(extension.id)}`,
            { method: 'POST', body: JSON.stringify({ operation: 'avatars.list' }) }
          );
          for (const pack of listed.result.slice(0, 8)) {
            for (let i = 0; i < 4; i++) {
              const seed = crypto.randomUUID();
              const generated = await adminRequest<{ result: { svg: string } }>(
                `/api/v1/account/avatar/extensions/${encodeURIComponent(extension.id)}`,
                { method: 'POST', body: JSON.stringify({ operation: 'avatars.generate', pack: pack.id, seed }) }
              );
              items.push({
                id: `ext-${extension.id}-${pack.id}-${seed}`,
                name: `${pack.name || pack.id} #${i + 1}`,
                url: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(generated.result.svg)}`,
                kind: 'extension',
                extension: extension.id,
                pack: pack.id,
                seed
              });
            }
          }
        } catch {
          // ignore extension error to preserve other images
        }
      }

      // 2. Server collections: flatten all images without forcing pack selection
      for (const pack of collections) {
        try {
          const res = await fetch(`/api/v1/account/avatar/collections/${encodeURIComponent(pack.id)}`, {
            credentials: 'same-origin',
            cache: 'no-store'
          });
          if (res.ok) {
            const data = await res.json();
            for (const img of (data.images || [])) {
              items.push({
                id: `col-${pack.id}-${img.id}`,
                name: img.name || 'Avatar',
                url: img.url,
                kind: 'collection'
              });
            }
          }
        } catch {
          // ignore pack error
        }
      }

      allImages = items;
      if (allImages.length > 0) {
        selectedItem = allImages[0];
      }
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Photos indisponibles.';
    } finally {
      modalLoading = false;
    }
  }

  function closeModal() {
    showModal = false;
    selectedItem = null;
  }

  async function saveSelected() {
    if (!selectedItem) return;
    const current = $session;
    if (!current) return;
    busy = true;
    error = '';
    notice = '';
    try {
      if (selectedItem.kind === 'extension' && selectedItem.extension && selectedItem.pack && selectedItem.seed) {
        await adminRequest(
          `/api/v1/account/avatar/extensions/${encodeURIComponent(selectedItem.extension)}`,
          {
            method: 'POST',
            body: JSON.stringify({
              operation: 'avatars.generate',
              pack: selectedItem.pack,
              seed: selectedItem.seed,
              apply: true
            })
          }
        );
      } else if (selectedItem.kind === 'collection') {
        const response = await fetch(selectedItem.url, {
          method: 'POST',
          credentials: 'same-origin',
          cache: 'no-store',
          headers: { 'x-csrf-token': current.csrfToken },
          signal: AbortSignal.timeout(20000)
        });
        if (!response.ok) throw new Error('Impossible d’enregistrer cet avatar.');
      }
      avatarRevision.update((v) => v + 1);
      notice = 'Photo de profil enregistrée.';
      closeModal();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Modification impossible.';
    } finally {
      busy = false;
    }
  }

  async function importGravatar() {
    busy = true;
    error = '';
    notice = '';
    try {
      await adminRequest('/api/v1/account/avatar/gravatar', {
        method: 'POST',
        body: JSON.stringify({ email: gravatarEmail })
      });
      avatarRevision.update((v) => v + 1);
      notice = 'Gravatar importé.';
      gravatarEmail = '';
    } catch (e) {
      error = e instanceof Error ? e.message : 'Import impossible.';
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    const userId = $session?.user.id;
    uploadAllowed = false;
    removeAllowed = false;
    showModal = false;
    extensions = [];
    collections = [];
    allImages = [];
    selectedItem = null;
    gravatarAllowed = false;
    gravatarEmail = '';
    if (!userId) return;
    const controller = new AbortController();

    void fetch('/api/v1/account/avatar/extensions', {
      credentials: 'same-origin',
      cache: 'no-store',
      signal: controller.signal
    })
      .then((r) => (r.ok ? r.json() : { extensions: [] }))
      .then((v) => {
        if ($session?.user.id === userId) extensions = v.extensions;
      })
      .catch(() => {});

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
        error = 'Impossible de charger les autorisations de l’image de profil.';
      });

    void fetch('/api/v1/account/avatar/gravatar', {
      credentials: 'same-origin',
      cache: 'no-store',
      signal: controller.signal
    })
      .then(async (r) => (r.ok ? r.json() : { allowed: false }))
      .then((v) => {
        if ($session?.user.id === userId) gravatarAllowed = v.allowed;
      })
      .catch(() => {});

    void fetch('/api/v1/account/avatar/collections', {
      credentials: 'same-origin',
      cache: 'no-store',
      signal: controller.signal
    })
      .then(async (r) => (r.ok ? r.json() : { packs: [] }))
      .then((v) => {
        if ($session?.user.id === userId) {
          collections = v.packs;
        }
      })
      .catch(() => {});

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
    {#if collections.length || extensions.length}
      <button disabled={busy} aria-expanded={showModal} onclick={() => void openModal()}>
        Choisir une photo de profil
      </button>
    {/if}
  </div>

  {#if gravatarAllowed}
    <form
      class="gravatar-section"
      onsubmit={(event) => {
        event.preventDefault();
        void importGravatar();
      }}
    >
      <label>
        Adresse e-mail Gravatar
        <input type="email" required maxlength="254" bind:value={gravatarEmail} disabled={busy} />
      </label>
      <button disabled={busy}>Importer mon Gravatar</button>
      <p>L’adresse n’est pas conservée. Son empreinte est envoyée à Gravatar pour télécharger votre image.</p>
    </form>
  {/if}

  {#if busy}
    <p role="status">Enregistrement…</p>
  {/if}
  {#if notice}
    <p role="status">{notice}</p>
  {/if}
  {#if error}
    <p role="alert">{error}</p>
  {/if}

  {#if showModal}
    <div
      class="avatar-modal-backdrop"
      role="dialog"
      aria-modal="true"
      aria-labelledby="modal-avatar-title"
      tabindex="-1"
      onkeydown={(e) => { if (e.key === 'Escape') closeModal(); }}
    >
      <div class="avatar-modal">
        <div class="modal-header">
          <h3 id="modal-avatar-title">Choisir une photo de profil</h3>
          <button type="button" class="close-btn" aria-label="Fermer" onclick={closeModal}>
            <Icon name="close" size={18} />
          </button>
        </div>

        <div class="modal-body">
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
      </div>
    </div>
  {/if}
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
  .gravatar-section {
    margin-top: 16px;
  }
  label {
    display: grid;
    gap: 12px;
    font-size: 14px;
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
  .avatar-modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
  }
  .avatar-modal {
    background: var(--bg-elevated, #1f1f23);
    color: var(--text-primary);
    border: 1px solid var(--border-subtle, #333);
    border-radius: var(--radius-lg, 12px);
    width: 100%;
    max-width: 640px;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 16px 36px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }
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
