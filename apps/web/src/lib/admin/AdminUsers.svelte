<script lang="ts">
  import type { Library } from './types';
  import type { AuthUser } from '../auth/session';
  import Icon from '../ui/Icon.svelte';
  import { t } from '../i18n';

  interface Props {
    users: AuthUser[];
    libraries: Library[];
    currentUserId?: string;
  }

  const {
    users,
    libraries,
    currentUserId
  }: Props = $props();
</script>

<section id="users-section" class="dashboard-section" aria-labelledby="users-heading">
  <div class="section-header">
    <div class="header-icon-wrap">
      <Icon name="users" size={18} />
    </div>
    <div>
      <h2 id="users-heading" class="section-title">{$t('users_heading')}</h2>
      <p class="section-subtitle">{$t('users_subtitle')}</p>
    </div>
  </div>

  <div class="card">
    <div class="card-header">
      <div class="card-title-group">
        <h3>{$t('registered_accounts')} ({users.length})</h3>
        <span class="muted-note">{$t('strict_roles_note')}</span>
      </div>
      <div class="badge-role-stats">
        <span class="role-pill admin-pill">
          {users.filter(u => u.role === 'administrator').length} {$t('administrator')}
        </span>
        <span class="role-pill user-pill">
          {users.filter(u => u.role === 'user').length} {$t('user')}(s)
        </span>
      </div>
    </div>

    <div class="users-table-wrapper">
      <table class="admin-table">
        <thead>
          <tr>
            <th>{$t('col_user_login')}</th>
            <th>{$t('col_user_name')}</th>
            <th>{$t('col_auth_type')}</th>
            <th>{$t('col_role')}</th>
            <th>{$t('col_allowed_libs')}</th>
          </tr>
        </thead>
        <tbody>
          {#each users as user (user.id)}
            {@const isSso = user.sso ?? user.login.startsWith('sso-')}
            {@const userLibraries = libraries.filter(lib => lib.userIds.includes(user.id))}
            <tr>
              <td>
                <div class="user-login-cell">
                  <strong>{user.login}</strong>
                  {#if user.id === currentUserId}
                    <span class="current-user-badge">{$t('you')}</span>
                  {/if}
                </div>
              </td>
              <td>
                <span class="user-display-name">
                  {user.displayName || user.login}
                </span>
              </td>
              <td>
                <span class="badge" data-status={isSso ? 'disconnected' : 'connected'}>
                  {isSso ? $t('sso_badge') : $t('local_account')}
                </span>
              </td>
              <td>
                <span class="role-badge" class:admin-role={user.role === 'administrator'}>
                  {user.role === 'administrator' ? $t('administrator') : $t('user')}
                </span>
              </td>
              <td>
                {#if user.role === 'administrator'}
                  <span class="all-access-badge">{$t('all_libs_granted')}</span>
                {:else if userLibraries.length > 0}
                  <div class="lib-badges-list">
                    {#each userLibraries as lib (lib.id)}
                      <span class="badge kind-badge">{lib.name}</span>
                    {/each}
                  </div>
                {:else}
                  <span class="muted">{$t('no_libs_granted')}</span>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <div class="security-footer-note">
      <Icon name="shield" size={16} />
      <span>
        {$t('users_footer_hint')}
      </span>
    </div>
  </div>
</section>

<style>
  .dashboard-section {
    margin-bottom: 48px;
    scroll-margin-top: 24px;
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 24px;
    padding-bottom: 14px;
    border-bottom: 1px solid var(--color-line);
  }

  .header-icon-wrap {
    width: 36px;
    height: 36px;
    border-radius: var(--radius-sm);
    background: var(--color-accent-soft);
    color: var(--color-accent);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .section-title {
    font-size: 20px;
    font-weight: 650;
    letter-spacing: -0.02em;
    margin: 0;
    color: var(--color-text);
  }

  .section-subtitle {
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--color-muted);
  }

  .card {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: 24px;
  }

  .card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 20px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--color-line);
    flex-wrap: wrap;
  }

  .card-title-group h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }

  .badge-role-stats {
    display: flex;
    gap: 8px;
  }

  .role-pill {
    font-size: 11px;
    font-weight: 600;
    padding: 3px 10px;
    border-radius: 99px;
  }

  .admin-pill {
    background: var(--color-accent-soft);
    color: var(--color-accent);
  }

  .user-pill {
    background: var(--color-raised);
    color: var(--color-muted);
  }

  .users-table-wrapper {
    overflow-x: auto;
  }

  .admin-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
    text-align: left;
  }

  .admin-table th {
    padding: 12px 14px;
    border-bottom: 1px solid var(--color-line);
    color: var(--color-muted);
    font-weight: 500;
    font-size: 12px;
  }

  .admin-table td {
    padding: 14px;
    border-bottom: 1px solid var(--color-line);
    vertical-align: middle;
  }

  .user-login-cell {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .current-user-badge {
    font-size: 10px;
    font-weight: 600;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    padding: 2px 6px;
    border-radius: 4px;
  }

  .role-badge {
    font-size: 11px;
    padding: 3px 8px;
    border-radius: 4px;
    background: var(--color-raised);
    color: var(--color-muted);
  }

  .admin-role {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    font-weight: 600;
  }

  .all-access-badge {
    color: #9be7b4;
    font-size: 12px;
    font-weight: 500;
  }

  .lib-badges-list {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    font-weight: 500;
    padding: 2px 8px;
    border-radius: 99px;
    border: 1px solid var(--color-line);
    vertical-align: middle;
  }

  .badge[data-status='connected'] {
    color: #9be7b4;
    border-color: #2e6b44;
    background: #14281c;
  }

  .badge[data-status='disconnected'] {
    color: #f5c97b;
    border-color: #75541e;
    background: #2b200e;
  }

  .kind-badge {
    background: var(--color-raised);
    color: #d1cadf;
  }

  .security-footer-note {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 20px;
    padding: 12px 16px;
    background: var(--color-raised);
    border-radius: var(--radius-sm);
    font-size: 12px;
    color: var(--color-muted);
  }

  .muted-note {
    font-size: 12px;
    color: var(--color-muted);
  }

  .muted {
    color: var(--color-subtle);
    font-size: 12px;
  }
</style>
