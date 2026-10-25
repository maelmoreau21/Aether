<script lang="ts">
  import { adminRequest, type UserGroup } from './request';
  import type { AuthUser } from '../auth/session';
  import type { Library } from './types';
  import Icon from '../ui/Icon.svelte';

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
    <div class="dsm-modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) { editorOpen = false; } }}>
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
  .dsm-container {
    display: flex;
    flex-direction: column;
    gap: 14px;
    margin-bottom: 40px;
  }

  .dsm-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--color-line);
  }

  .dsm-title-group {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .dsm-icon-badge {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border-radius: var(--radius-md);
  }

  h2 {
    margin: 0;
    font-size: 20px;
    font-weight: 700;
  }

  .dsm-subtitle {
    margin: 2px 0 0;
    font-size: 13px;
    color: var(--color-muted);
  }

  /* Toolbar */
  .dsm-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 8px 12px;
  }

  .toolbar-left, .toolbar-right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
    border-radius: var(--radius-sm);
    font-size: 13px;
    font-weight: 500;
    border: 1px solid transparent;
    cursor: pointer;
    background: transparent;
    transition: all 0.15s ease;
  }

  .btn-primary {
    background: var(--color-accent);
    color: var(--color-on-accent);
  }
  .btn-primary:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .btn-default {
    background: var(--color-raised);
    border-color: var(--color-line);
    color: var(--color-text);
  }
  .btn-default:hover:not(:disabled) {
    background: var(--color-surface);
    border-color: var(--color-accent);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .search-box {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 0 8px;
    height: 32px;
    color: var(--color-muted);
  }

  .search-box input {
    border: none;
    background: transparent;
    color: var(--color-text);
    outline: none;
    font-size: 13px;
    width: 140px;
  }

  /* Table */
  .dsm-table-panel {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    overflow-x: auto;
  }

  .dsm-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
    text-align: left;
  }

  .dsm-table th {
    background: var(--color-raised);
    border-bottom: 1px solid var(--color-line);
    padding: 8px 12px;
    font-weight: 600;
    color: var(--color-muted);
    user-select: none;
    white-space: nowrap;
  }

  .dsm-table th.sortable {
    cursor: pointer;
  }
  .dsm-table th.sortable:hover {
    color: var(--color-text);
  }

  .dsm-table td {
    padding: 8px 12px;
    border-bottom: 1px solid var(--color-line);
    vertical-align: middle;
  }

  .dsm-table tbody tr:hover {
    background: var(--color-raised);
  }

  .dsm-table tbody tr.selected {
    background: var(--color-accent-soft);
  }

  .group-name-cell {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .group-icon {
    display: grid;
    place-items: center;
    color: var(--color-accent);
  }

  .dsm-pill {
    font-size: 11px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 99px;
  }

  .pill-default {
    background: #0284c726;
    color: #38bdf8;
  }

  .pill-custom {
    background: var(--color-raised);
    color: var(--color-muted);
  }

  .pill-admin {
    background: var(--color-accent-soft);
    color: var(--color-accent);
  }

  .pill-user {
    background: var(--color-raised);
    color: var(--color-muted);
  }

  .text-accent {
    color: var(--color-accent);
    font-weight: 500;
  }

  .text-muted {
    color: var(--color-muted);
    font-size: 12px;
  }

  .lib-count-badge {
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    font-size: 12px;
  }

  .empty-cell {
    text-align: center;
    padding: 30px;
    color: var(--color-muted);
  }

  .dsm-statusbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 12px;
    color: var(--color-muted);
    padding: 0 4px;
  }

  /* Modal */
  .dsm-modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: grid;
    place-items: center;
    z-index: 1000;
    padding: 20px;
    backdrop-filter: blur(2px);
  }

  .dsm-modal {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    width: 100%;
    max-width: 680px;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }

  .dsm-modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 20px;
    background: var(--color-raised);
    border-bottom: 1px solid var(--color-line);
  }

  .modal-title-wrap {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .modal-title-wrap h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 700;
  }

  .btn-close {
    background: transparent;
    border: none;
    color: var(--color-muted);
    cursor: pointer;
    display: grid;
    place-items: center;
    padding: 4px;
    border-radius: var(--radius-sm);
  }
  .btn-close:hover {
    color: var(--color-text);
    background: var(--color-line);
  }

  .dsm-tabs {
    display: flex;
    gap: 2px;
    padding: 0 16px;
    background: var(--color-raised);
    border-bottom: 1px solid var(--color-line);
    overflow-x: auto;
  }

  .dsm-tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 10px 14px;
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-muted);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    white-space: nowrap;
  }
  .dsm-tab:hover {
    color: var(--color-text);
  }
  .dsm-tab.active {
    color: var(--color-accent);
    border-bottom-color: var(--color-accent);
    font-weight: 600;
  }

  .dsm-modal-body {
    padding: 20px;
    overflow-y: auto;
    flex: 1;
  }

  .form-section-title {
    margin: 0 0 12px;
    font-size: 14px;
    font-weight: 600;
  }

  .section-desc {
    font-size: 13px;
    color: var(--color-muted);
    margin: 0 0 14px;
  }

  .dsm-grid-form {
    display: grid;
    gap: 16px;
  }

  .form-row {
    display: grid;
    grid-template-columns: 180px 1fr;
    align-items: start;
    gap: 16px;
    font-size: 13px;
  }

  .row-label {
    padding-top: 8px;
    color: var(--color-muted);
    font-weight: 500;
  }

  .row-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .row-field input:not([type="checkbox"]) {
    width: 100%;
    height: 34px;
    padding: 0 10px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    color: var(--color-text);
    font-size: 13px;
    box-sizing: border-box;
  }

  .field-hint {
    font-size: 11px;
    color: var(--color-muted);
  }

  .dsm-checkbox-label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    padding-top: 6px;
  }

  .dsm-info-box {
    display: flex;
    align-items: center;
    gap: 12px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    padding: 12px 16px;
    border-radius: var(--radius-sm);
    font-size: 13px;
    color: var(--color-text);
  }

  .status-indicator {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }
  .status-indicator .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .status-on .dot {
    background: #22c55e;
  }
  .status-off .dot {
    background: #ef4444;
  }

  .lib-kind-pill {
    text-transform: capitalize;
    font-size: 11px;
    background: var(--color-raised);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
  }

  .dsm-modal-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    padding: 14px 20px;
    background: var(--color-raised);
    border-top: 1px solid var(--color-line);
  }

  .dsm-alert {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    font-size: 13px;
  }
  .alert-success {
    background: var(--color-success-soft);
    border: 1px solid var(--color-success);
    color: var(--color-success);
  }
  .alert-error {
    background: var(--color-danger-soft);
    border: 1px solid var(--color-danger);
    color: var(--color-danger);
  }

  @media(max-width: 640px) {
    .form-row {
      grid-template-columns: 1fr;
      gap: 6px;
    }
  }

  .col-check { width: 48px; }
  .col-check-center { width: 40px; text-align: center; }
  .cell-center { text-align: center; }
  .user-login-paren { margin-left: 8px; }

</style>
