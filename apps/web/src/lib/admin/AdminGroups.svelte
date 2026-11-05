<script lang="ts">
  import { adminRequest, type UserGroup } from './request';
  import type { AuthUser } from '../auth/session';
  import type { Library } from './types';
  import Icon from '../ui/Icon.svelte';
  import { modalFocus } from './modal-focus';

  let {
    groups,
    users,
    libraries,
    onchanged
  }: {
    groups: UserGroup[];
    users: AuthUser[];
    libraries: Library[];
    onchanged: () => Promise<void>;
  } = $props();

  let selectedGroupIds = $state<string[]>([]);
  let editorOpen = $state(false);
  let activeTab = $state<'info' | 'members' | 'permissions'>('info');

  let name = $state('');
  let isDefault = $state(false);
  let userIds = $state<string[]>([]);
  let libraryIds = $state<string[]>([]);
  let editing = $state<string | null>(null);

  let busy = $state(false);
  let error = $state('');
  let notice = $state('');

  // Search & Sorting
  let search = $state('');
  let sortField = $state<'name' | 'type' | 'members' | 'libraries'>('name');
  let sortAsc = $state(true);

  function reset() {
    editing = null;
    editorOpen = false;
    name = '';
    isDefault = false;
    userIds = [];
    libraryIds = [];
    activeTab = 'info';
  }

  function edit(group: UserGroup | null) {
    if (!group) {
      reset();
      editorOpen = true;
      return;
    }
    editing = group.id;
    name = group.name;
    isDefault = !!group.isDefault;
    userIds = [...group.userIds];
    libraryIds = [...group.libraryIds];
    editorOpen = true;
    activeTab = 'info';
  }

  async function save(event?: SubmitEvent) {
    event?.preventDefault();
    if (busy || !name.trim()) return;
    busy = true;
    error = '';
    notice = '';
    try {
      await adminRequest(
        editing ? `/api/v1/admin/groups/${encodeURIComponent(editing)}` : '/api/v1/admin/groups',
        {
          method: editing ? 'PUT' : 'POST',
          body: JSON.stringify({
            name: name.trim(),
            isDefault,
            userIds,
            libraryIds
          })
        }
      );
      reset();
      await onchanged();
      notice = 'Groupe enregistré avec succès.';
    } catch (e) {
      error = e instanceof Error ? e.message : 'Enregistrement impossible.';
    } finally {
      busy = false;
    }
  }

  async function remove(group: UserGroup) {
    if (busy) return;
    if (!window.confirm(`Supprimer le groupe « ${group.name} » ? Les comptes utilisateurs seront conservés.`)) return;
    busy = true;
    error = '';
    notice = '';
    try {
      await adminRequest(`/api/v1/admin/groups/${encodeURIComponent(group.id)}`, { method: 'DELETE' });
      if (editing === group.id) reset();
      selectedGroupIds = selectedGroupIds.filter(id => id !== group.id);
      await onchanged();
      notice = 'Groupe supprimé avec succès.';
    } catch (e) {
      error = e instanceof Error ? e.message : 'Suppression impossible.';
    } finally {
      busy = false;
    }
  }

  const filteredGroups = $derived.by(() => {
    return groups.filter(g => {
      const q = search.trim().toLowerCase();
      return !q || g.name.toLowerCase().includes(q);
    }).sort((a, b) => {
      let cmp = 0;
      if (sortField === 'name') {
        cmp = a.name.localeCompare(b.name);
      } else if (sortField === 'type') {
        cmp = (a.isDefault ? 1 : 0) - (b.isDefault ? 1 : 0);
      } else if (sortField === 'members') {
        cmp = (a.isDefault ? 999999 : a.userIds.length) - (b.isDefault ? 999999 : b.userIds.length);
      } else if (sortField === 'libraries') {
        cmp = a.libraryIds.length - b.libraryIds.length;
      }
      return sortAsc ? cmp : -cmp;
    });
  });

  const allSelected = $derived(filteredGroups.length > 0 && filteredGroups.every(g => selectedGroupIds.includes(g.id)));

  function toggleSelectAll() {
    if (allSelected) {
      selectedGroupIds = [];
    } else {
      selectedGroupIds = filteredGroups.map(g => g.id);
    }
  }

  function toggleSelectGroup(id: string) {
    if (selectedGroupIds.includes(id)) {
      selectedGroupIds = selectedGroupIds.filter(item => item !== id);
    } else {
      selectedGroupIds = [...selectedGroupIds, id];
    }
  }

  function handleSort(field: 'name' | 'type' | 'members' | 'libraries') {
    if (sortField === field) {
      sortAsc = !sortAsc;
    } else {
      sortField = field;
      sortAsc = true;
    }
  }
</script>

<section id="groups-section" class="dsm-container" aria-labelledby="groups-heading">
  <!-- DSM Header -->
  <div class="dsm-header">
    <div class="dsm-title-group">
      <div class="dsm-icon-badge">
        <Icon name="users" size={22} />
      </div>
      <div>
        <h2 id="groups-heading">Groupes d’utilisateurs</h2>
        <p class="dsm-subtitle">Organisez vos utilisateurs et assignez des droits d'accès collectifs aux médiathèques.</p>
      </div>
    </div>
  </div>

  {#if error || notice}
    <div class="dsm-alert" class:alert-error={error} class:alert-success={!error && notice} role="status">
      <Icon name={error ? 'alert' : 'check'} size={18} />
      <span>{error || notice}</span>
    </div>
  {/if}

  {#if editorOpen}
    <!-- DSM Dialog / Modal Panel -->
    <div class="dsm-modal-backdrop" use:modalFocus={() => { editorOpen = false; }}>
      <div class="dsm-modal" role="dialog" aria-modal="true" aria-labelledby="modal-group-title">
        <div class="dsm-modal-header">
          <div class="modal-title-wrap">
            <Icon name="users" size={20} />
            <h3 id="modal-group-title">{editing ? `Modifier le groupe — ${name}` : 'Créer un groupe d’utilisateurs'}</h3>
          </div>
          <button type="button" class="btn-close" aria-label="Fermer" onclick={() => { editorOpen = false; }}>
            <Icon name="close" size={18} />
          </button>
        </div>

        <div class="dsm-tabs" role="tablist">
          <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'info'} onclick={() => activeTab = 'info'}>
            <Icon name="info" size={15} /> Informations
          </button>
          <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'members'} onclick={() => activeTab = 'members'}>
            <Icon name="users" size={15} /> Membres
          </button>
          <button type="button" role="tab" class="dsm-tab" class:active={activeTab === 'permissions'} onclick={() => activeTab = 'permissions'}>
            <Icon name="film" size={15} /> Permissions
          </button>
        </div>

        <div class="dsm-modal-body">
          {#if activeTab === 'info'}
            <div class="dsm-form-section">
              <h4 class="form-section-title">Propriétés du groupe</h4>
              <div class="dsm-grid-form">
                <label class="form-row">
                  <span class="row-label">Nom du groupe <abbr title="requis" class="required">*</abbr></span>
                  <div class="row-field">
                    <input required maxlength="80" bind:value={name} disabled={busy} placeholder="ex: Famille, Amis, Enfants" />
                    <span class="field-hint">Nom explicite pour identifier ce groupe d'accès.</span>
                  </div>
                </label>

                <div class="form-row">
                  <span class="row-label">Type d'affectation</span>
                  <div class="row-field">
                    <label class="dsm-checkbox-label">
                      <input type="checkbox" bind:checked={isDefault} disabled={busy} />
                      <span><strong>Groupe par défaut</strong></span>
                    </label>
                    <span class="field-hint">Tous les utilisateurs actuels et futurs font automatiquement partie de ce groupe et accèdent à ses bibliothèques.</span>
                  </div>
                </div>
              </div>
            </div>
          {:else if activeTab === 'members'}
            <div class="dsm-form-section">
              <h4 class="form-section-title">Membres du groupe</h4>
              {#if isDefault}
                <div class="dsm-info-box">
                  <Icon name="info" size={18} />
                  <span>Ce groupe est configuré comme <strong>Groupe par défaut</strong>. Tous les utilisateurs sont membres automatiquement.</span>
                </div>
              {:else}
                <p class="section-desc">Cochez les utilisateurs à intégrer dans ce groupe :</p>
                <div class="dsm-table-container">
                  <table class="dsm-table">
                    <thead>
                      <tr>
                        <th class="col-check">
                          <input
                            type="checkbox"
                            checked={users.length > 0 && users.every(u => userIds.includes(u.id))}
                            onchange={(e) => userIds = e.currentTarget.checked ? users.map(u => u.id) : []}
                          />
                        </th>
                        <th>Utilisateur</th>
                        <th>Rôle</th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each users as user (user.id)}
                        <tr>
                          <td>
                            <input
                              type="checkbox"
                              value={user.id}
                              bind:group={userIds}
                            />
                          </td>
                          <td>
                            <strong>{user.displayName || user.login}</strong>
                            <span class="text-muted user-login-paren">({user.login})</span>
                          </td>
                          <td>
                            {#if user.role === 'administrator'}
                              <span class="dsm-pill pill-admin">Administrateur</span>
                            {:else}
                              <span class="dsm-pill pill-user">Utilisateur</span>
                            {/if}
                          </td>
                        </tr>
                      {:else}
                        <tr>
                          <td colspan="3" class="empty-cell">Aucun utilisateur disponible</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
              {/if}
            </div>
          {:else if activeTab === 'permissions'}
            <div class="dsm-form-section">
              <h4 class="form-section-title">Permissions sur les bibliothèques</h4>
              <p class="section-desc">Cochez les bibliothèques accessibles par tous les membres de ce groupe :</p>
              <div class="dsm-table-container">
                <table class="dsm-table">
                  <thead>
                    <tr>
                      <th class="col-check">
                        <input
                          type="checkbox"
                          checked={libraries.length > 0 && libraries.every(l => libraryIds.includes(l.id))}
                          onchange={(e) => libraryIds = e.currentTarget.checked ? libraries.map(l => l.id) : []}
                        />
                      </th>
                      <th>Médiathèque</th>
                      <th>Type</th>
                      <th>État</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each libraries as lib (lib.id)}
                      <tr>
                        <td>
                          <input
                            type="checkbox"
                            value={lib.id}
                            bind:group={libraryIds}
                          />
                        </td>
                        <td>
                          <strong>{lib.name}</strong>
                        </td>
                        <td>
                          <span class="lib-kind-pill">{lib.kind}</span>
                        </td>
                        <td>
                          {#if lib.enabled}
                            <span class="status-indicator status-on"><span class="dot"></span> Active</span>
                          {:else}
                            <span class="status-indicator status-off"><span class="dot"></span> Désactivée</span>
                          {/if}
                        </td>
                      </tr>
                    {:else}
                      <tr>
                        <td colspan="4" class="empty-cell">Aucune bibliothèque disponible</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </div>
          {/if}
        </div>

        <div class="dsm-modal-footer">
          <button type="button" class="btn btn-default" disabled={busy} onclick={() => { editorOpen = false; }}>
            Annuler
          </button>
          <button type="button" class="btn btn-primary" disabled={busy || !name.trim()} onclick={() => save()}>
            {busy ? 'Enregistrement…' : 'Appliquer'}
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- DSM Action Toolbar -->
  <div class="dsm-toolbar">
    <div class="toolbar-left">
      <button type="button" class="btn btn-primary" disabled={busy} onclick={() => edit(null)}>
        <Icon name="users" size={16} />
        <span>Créer</span>
      </button>

      <button
        type="button"
        class="btn btn-default"
        disabled={busy || selectedGroupIds.length !== 1}
        onclick={() => {
          const g = groups.find(item => item.id === selectedGroupIds[0]);
          if (g) edit(g);
        }}
      >
        <Icon name="edit" size={16} />
        <span>Modifier</span>
      </button>

      <button
        type="button"
        class="btn btn-default"
        disabled={busy || selectedGroupIds.length !== 1}
        onclick={() => {
          const g = groups.find(item => item.id === selectedGroupIds[0]);
          if (g) remove(g);
        }}
      >
        <Icon name="trash" size={16} />
        <span>Supprimer</span>
      </button>

      <button type="button" class="btn btn-default" disabled={busy} onclick={() => onchanged()}>
        <Icon name="refresh" size={16} />
        <span>Actualiser</span>
      </button>
    </div>

    <div class="toolbar-right">
      <div class="search-box">
        <Icon name="search" size={16} />
        <input type="search" bind:value={search} placeholder="Rechercher…" />
      </div>
    </div>
  </div>

  <!-- DSM Table View -->
  <div class="dsm-table-panel">
    <table class="dsm-table main-table">
      <thead>
        <tr>
          <th class="col-check-center">
            <input type="checkbox" checked={allSelected} onchange={toggleSelectAll} aria-label="Sélectionner tous les groupes" />
          </th>
          <th class="sortable" onclick={() => handleSort('name')}>
            Nom du groupe {sortField === 'name' ? (sortAsc ? '▲' : '▼') : ''}
          </th>
          <th class="sortable" onclick={() => handleSort('type')}>
            Type {sortField === 'type' ? (sortAsc ? '▲' : '▼') : ''}
          </th>
          <th class="sortable" onclick={() => handleSort('members')}>
            Membres {sortField === 'members' ? (sortAsc ? '▲' : '▼') : ''}
          </th>
          <th class="sortable" onclick={() => handleSort('libraries')}>
            Bibliothèques accordées {sortField === 'libraries' ? (sortAsc ? '▲' : '▼') : ''}
          </th>
        </tr>
      </thead>
      <tbody>
        {#each filteredGroups as group (group.id)}
          {@const isSelected = selectedGroupIds.includes(group.id)}
          <tr
            class:selected={isSelected}
            ondblclick={() => edit(group)}
          >
            <td class="cell-center">
              <input
                type="checkbox"
                checked={isSelected}
                onchange={() => toggleSelectGroup(group.id)}
                aria-label={`Sélectionner ${group.name}`}
              />
            </td>
            <td>
              <div class="group-name-cell">
                <span class="group-icon">
                  <Icon name="users" size={16} />
                </span>
                <strong>{group.name}</strong>
              </div>
            </td>
            <td>
              {#if group.isDefault}
                <span class="dsm-pill pill-default">Par défaut (automatique)</span>
              {:else}
                <span class="dsm-pill pill-custom">Personnalisé</span>
              {/if}
            </td>
            <td>
              {#if group.isDefault}
                <span class="text-accent">Tous les utilisateurs</span>
              {:else}
                <span>{group.userIds.length} utilisateur(s)</span>
              {/if}
            </td>
            <td>
              {#if group.libraryIds.length > 0}
                <span class="lib-count-badge">{group.libraryIds.length} bibliothèque(s)</span>
              {:else}
                <span class="text-muted">Aucune</span>
              {/if}
            </td>
          </tr>
        {:else}
          <tr>
            <td colspan="5" class="empty-cell">
              Aucun groupe d’utilisateurs trouvé.
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <div class="dsm-statusbar">
    <span>Total : <strong>{filteredGroups.length}</strong> groupe(s)</span>
    <span>{selectedGroupIds.length} sélectionné(s)</span>
  </div>
</section>

<style>
.dsm-container { display: flex; flex-direction: column; gap: 16px; margin-bottom: 40px; font-family: var(--font-sans); color: var(--color-text); }

  /* En-tête de section */
  .dsm-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .dsm-title-group { display: flex; align-items: center; gap: 14px; min-width: 0; }
  .dsm-icon-badge { display: grid; place-items: center; flex: none; width: 44px; height: 44px; border-radius: var(--radius-md); background: var(--color-raised); color: var(--color-text); }
  h2 { margin: 0; font-size: 24px; font-weight: 700; letter-spacing: -0.02em; line-height: 1.2; }
  .dsm-subtitle { margin: 4px 0 0; font-size: 14px; line-height: 1.5; color: var(--color-muted); }

  /* Barre d'outils */
  .dsm-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  .toolbar-left, .toolbar-right { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; min-width: 0; }

  /* Boutons */
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 44px;
    padding: 0 16px;
    border: 0;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: 14px;
    font-weight: 700;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color var(--transition-fast), opacity var(--transition-fast), transform var(--transition-fast);
  }
  .btn:active:not(:disabled) { transform: scale(0.98); }
  .btn:focus-visible, .btn-close:focus-visible, .dsm-tab:focus-visible, .sortable:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 3px; }
  .btn-primary { background: var(--color-accent); color: var(--color-on-accent); }
  .btn-primary:hover:not(:disabled) { background: var(--color-accent-hover); }
  .btn-default { background: transparent; border: 1px solid var(--color-line); color: var(--color-text); }
  .btn-default:hover:not(:disabled) { background: var(--color-raised); }
  .btn:disabled { opacity: 0.45; cursor: not-allowed; }

  /* Recherche et filtres */
  .search-box {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    padding: 0 14px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: var(--color-raised);
    color: var(--color-muted);
    transition: border-color var(--transition-fast), background-color var(--transition-fast);
  }
  .search-box:focus-within { border-color: var(--color-accent); background: var(--color-surface-elevated); }
  .search-box input { width: 180px; min-width: 0; height: 100%; min-height: 0; box-shadow: none; padding: 0; border: 0; background: transparent; color: var(--color-text); font: inherit; font-size: 14px; outline: none; }
  .search-box input::placeholder { color: var(--color-subtle); }

  /* Tableaux */
  .dsm-table-panel { overflow-x: auto; padding: 4px 16px; border-radius: var(--radius-md); background: var(--color-surface); }
  .dsm-table-container { overflow-x: auto; border-radius: var(--radius-md); background: var(--color-surface); }
  .dsm-table { width: 100%; border-collapse: collapse; font-size: 14px; text-align: left; }
  .main-table { min-width: 720px; }
  .dsm-table th {
    height: 48px;
    padding: 0 16px;
    border-bottom: 1px solid var(--color-line-subtle);
    background: transparent;
    color: var(--color-muted);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    user-select: none;
  }
  .dsm-table th.sortable { cursor: pointer; transition: color var(--transition-fast); }
  .dsm-table th.sortable:hover { color: var(--color-text); }
  .dsm-table td { height: 60px; padding: 8px 16px; border-bottom: 1px solid var(--color-line-subtle); vertical-align: middle; }
  .dsm-table tbody tr:last-child td { border-bottom: 0; }
  .dsm-table tbody tr { transition: background-color var(--transition-fast); }
  .dsm-table tbody tr:hover { background: var(--color-surface-hover); }
  .dsm-table tbody tr.selected { background: var(--color-accent-soft); }
  .dsm-table input[type='checkbox'] { width: 20px; height: 20px; margin: 0; accent-color: var(--color-accent); cursor: pointer; vertical-align: middle; }
  .dsm-table input[type='checkbox']:disabled { cursor: not-allowed; opacity: 0.45; }


  /* Badges */
  .dsm-pill, .lib-kind-pill, .lib-count-badge {
    display: inline-flex;
    align-items: center;
    height: 24px;
    padding: 0 9px;
    border-radius: var(--radius-xs);
    background: var(--color-raised-hover);
    color: var(--color-text);
    font-size: 12px;
    font-weight: 700;
    white-space: nowrap;
  }
  .pill-admin { background: var(--color-accent-soft); color: var(--color-accent); }
  .pill-user { background: var(--color-raised-hover); color: var(--color-muted); }
  .pill-default { background: var(--color-raised); color: var(--color-info); }
  .lib-kind-pill { text-transform: capitalize; }
  .pill-custom { background: var(--color-raised-hover); color: var(--color-muted); }
  .lib-count-badge { font-weight: 600; }

  .status-indicator { display: inline-flex; align-items: center; gap: 8px; font-size: 14px; font-weight: 600; }
  .status-indicator .dot { width: 8px; height: 8px; border-radius: 50%; }
  .status-on .dot { background: var(--color-success); }
  .status-off .dot { background: var(--color-danger); }

  .text-muted { color: var(--color-muted); font-size: 13px; }
  .text-accent { color: var(--color-text); font-weight: 600; }
  .user-login-paren { margin-left: 8px; }
  .group-name-cell { display: flex; align-items: center; gap: 12px; }
  .group-icon { display: grid; place-items: center; flex: none; width: 36px; height: 36px; border-radius: 50%; background: var(--color-raised-hover); color: var(--color-muted); }
  .empty-cell { height: auto; padding: 40px 16px; color: var(--color-muted); text-align: center; }
  .dsm-statusbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; padding: 0 4px; color: var(--color-muted); font-size: 13px; }
  .dsm-statusbar strong { color: var(--color-text); }

  /* Dialogue d'édition */
  .dsm-modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--color-canvas) 70%, transparent);
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
  }
  .dsm-modal {
    display: flex;
    flex-direction: column;
    width: 100%;
    max-width: 760px;
    max-height: min(88vh, 900px);
    overflow: hidden;
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    color: var(--color-text);
    box-shadow: var(--shadow-sm);
  }
  .dsm-modal-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 20px 20px 12px 28px; }
  .modal-title-wrap { display: flex; align-items: center; gap: 12px; min-width: 0; color: var(--color-muted); }
  .modal-title-wrap h3 { margin: 0; color: var(--color-text); font-size: 20px; font-weight: 700; letter-spacing: -0.02em; overflow-wrap: anywhere; }
  .btn-close { display: grid; place-items: center; flex: none; width: 44px; height: 44px; padding: 0; border: 0; border-radius: var(--radius-md); background: transparent; color: var(--color-text); cursor: pointer; transition: background-color var(--transition-fast); }
  .btn-close:hover { background: var(--color-raised); }

  .dsm-tabs { display: flex; gap: 20px; margin: 0 28px; border-bottom: 1px solid var(--color-line-subtle); overflow-x: auto; scrollbar-width: none; }
  .dsm-tab {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 46px;
    margin-bottom: -1px;
    padding: 0;
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: 14px;
    font-weight: 700;
    white-space: nowrap;
    cursor: pointer;
    transition: color var(--transition-fast);
  }
  .dsm-tab:hover { color: var(--color-text); }
  .dsm-tab.active { color: var(--color-text); border-bottom-color: var(--color-accent); }

  .dsm-modal-body { flex: 1; min-height: 0; padding: 24px 28px; overflow-y: auto; }
  .form-section-title { margin: 0 0 16px; font-size: 17px; font-weight: 700; }
  .section-desc { margin: -8px 0 16px; color: var(--color-muted); font-size: 13px; line-height: 1.5; }

  .dsm-grid-form { display: grid; }
  .form-row { display: grid; grid-template-columns: 200px minmax(0, 1fr); align-items: start; gap: 16px; padding: 16px 0; border-bottom: 1px solid var(--color-line-subtle); font-size: 14px; }
  .form-row:first-child { padding-top: 0; }
  .form-row:last-child { border-bottom: 0; padding-bottom: 0; }
  .row-label { padding-top: 12px; color: var(--color-text); font-weight: 700; }
  .required { color: var(--color-danger); text-decoration: none; }
  .row-field { display: flex; flex-direction: column; align-items: flex-start; gap: 8px; min-width: 0; }
  .row-field input:not([type='checkbox']) {
    box-sizing: border-box;
    width: 100%;
    height: 46px;
    padding: 0 14px;
    border: 1px solid var(--color-line-subtle);
    border-radius: var(--radius-sm);
    background-color: var(--color-canvas);
    color: var(--color-text);
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    box-shadow: none;
    transition: border-color var(--transition-fast), background-color var(--transition-fast);
  }
  .row-field input:not([type='checkbox']):hover { background-color: var(--color-raised-hover); }
  .row-field input:not([type='checkbox']):focus { outline: none; border-color: var(--color-accent); background-color: var(--color-surface-elevated); }
  .row-field input:disabled { opacity: 0.45; cursor: not-allowed; }
  .field-hint { color: var(--color-muted); font-size: 13px; line-height: 1.5; }
  .dsm-checkbox-label { display: flex; align-items: center; gap: 12px; min-height: 46px; font-weight: 600; cursor: pointer; }
  .dsm-checkbox-label input { width: 20px; height: 20px; margin: 0; accent-color: var(--color-accent); }

  .dsm-info-box { display: flex; align-items: flex-start; gap: 12px; padding: 14px 16px; border-radius: var(--radius-md); background: var(--color-raised); color: var(--color-text); font-size: 14px; line-height: 1.5; }
  .dsm-info-box :global(svg) { flex: none; margin-top: 1px; color: var(--color-info); }


  .dsm-modal-footer { display: flex; align-items: center; justify-content: flex-end; gap: 8px; padding: 16px 28px 24px; border-top: 1px solid var(--color-line-subtle); }

  /* Messages */
  .dsm-alert { display: flex; align-items: center; gap: 12px; min-height: 52px; padding: 12px 16px; border-radius: var(--radius-md); color: var(--color-text); font-size: 14px; }
  .alert-success { background: var(--color-success-soft); }
  .alert-success :global(svg) { color: var(--color-success); }
  .alert-error { background: var(--color-danger-soft); }
  .alert-error :global(svg) { color: var(--color-danger); }

  .col-check { width: 52px; }
  .col-check-center { width: 52px; text-align: center; }
  .cell-center { text-align: center; }

  @media (max-width: 820px) {
    .dsm-toolbar { align-items: stretch; flex-direction: column; }
    .toolbar-right { width: 100%; }
    .search-box { flex: 1 1 100%; }
    .search-box input { width: 100%; }
  }
  @media (max-width: 640px) {
    .dsm-icon-badge { display: none; }
    .toolbar-left .btn { flex: 1 1 calc(50% - 4px); }
    .dsm-modal-backdrop { place-items: end stretch; padding: 0; }
    .dsm-modal { max-width: none; max-height: 92dvh; border-radius: var(--radius-lg) var(--radius-lg) 0 0; }
    .dsm-modal-header { padding: 16px 12px 8px 16px; }
    .dsm-tabs { margin: 0 16px; }
    .dsm-modal-body { padding: 20px 16px; }
    .dsm-modal-footer { padding: 12px 16px calc(16px + env(safe-area-inset-bottom, 0px)); }
    .dsm-modal-footer .btn { flex: 1; }
    .form-row { grid-template-columns: minmax(0, 1fr); gap: 8px; }
    .row-label { padding-top: 0; }
  }
  @media (prefers-reduced-motion: reduce) {
    .btn, .dsm-tab, .btn-close, .dsm-table tbody tr, .search-box { transition: none; }
  }
</style>
