<script lang="ts">
  import type { OidcAdminStatus } from './types';
  import Icon from '../ui/Icon.svelte';

  interface Props {
    oidcStatus: OidcAdminStatus | null;
  }

  const { oidcStatus }: Props = $props();
</script>

<section id="sso-section" class="dashboard-section" aria-labelledby="sso-heading">
  <div class="section-header">
    <div class="header-icon-wrap">
      <Icon name="shield" size={18} />
    </div>
    <div>
      <h2 id="sso-heading" class="section-title">Authentification SSO (OpenID Connect)</h2>
      <p class="section-subtitle">Intégration d'identité fédérée avec Authentik, Keycloak, Dex ou Pocket ID</p>
    </div>
  </div>

  <div class="card">
    <div class="card-header">
      <div class="card-title-group">
        <h3>Statut du fournisseur OIDC</h3>
        <p class="card-subtitle">Protocole sécurisé Authorization Code avec vérification PKCE</p>
      </div>
      <span class="badge" data-status={oidcStatus?.enabled ? 'connected' : 'disconnected'}>
        {oidcStatus?.enabled ? 'OIDC Configuré' : 'OIDC Non activé'}
      </span>
    </div>

    {#if oidcStatus?.enabled}
      <div class="details-grid">
        <div class="detail-item">
          <span class="detail-label">Fournisseur (Issuer)</span>
          <span class="detail-value"><code>{oidcStatus.issuer ?? 'Non renseigné'}</code></span>
        </div>
        <div class="detail-item">
          <span class="detail-label">Identifiant client (Client ID)</span>
          <span class="detail-value"><code>{oidcStatus.clientId ?? 'Non renseigné'}</code></span>
        </div>
        <div class="detail-item">
          <span class="detail-label">URI de redirection Core</span>
          <span class="detail-value"><code>{oidcStatus.redirectUri ?? 'Non renseigné'}</code></span>
        </div>
        <div class="detail-item">
          <span class="detail-label">Groupes autorisés (claim {oidcStatus.groupsClaim ?? 'groups'})</span>
          <span class="detail-value">
            {oidcStatus.userGroups?.length ? oidcStatus.userGroups.join(', ') : (oidcStatus.requireGroup ? 'Aucun groupe configuré' : 'Tous les utilisateurs authentifiés')}
          </span>
        </div>
      </div>
    {:else}
      <p class="muted-note">
        Le protocole OIDC n'est pas encore configuré sur cette instance. Compatible avec Authentik, Keycloak, Dex, Pocket ID et tout fournisseur standardisé. Définissez les variables d'environnement requises sur le Core :
      </p>
      <pre class="env-code"><code>MEDIA_OIDC_ENABLED=true
MEDIA_OIDC_ISSUER=https://auth.example.com/realms/media/
MEDIA_OIDC_CLIENT_ID=votre-client-id
MEDIA_OIDC_CLIENT_SECRET=votre-secret
MEDIA_OIDC_USER_GROUPS=media-users
# Facultatifs :
# MEDIA_OIDC_SCOPES=openid profile email
# MEDIA_OIDC_GROUPS_CLAIM=groups
# MEDIA_OIDC_REQUIRE_GROUP=true</code></pre>
    {/if}

    <div class="security-note">
      <Icon name="check" size={16} />
      <div>
        <strong>Sécurité RBAC garantie :</strong>
        <p>
          L'authentification OIDC utilise le flux sécurisé <em>Authorization Code + PKCE</em>. Les utilisateurs authentifiés via SSO sont automatiquement créés avec le rôle <code>user</code>, sans aucun droit d'accès implicite sur les bibliothèques. L'administrateur local conserve toujours un accès direct de secours.
        </p>
      </div>
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

  .card-subtitle {
    margin: 4px 0 0;
    font-size: 12px;
    color: var(--color-muted);
  }

  .details-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 20px;
  }

  .detail-item {
    padding: 12px;
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
  }

  .detail-label {
    display: block;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-muted);
    margin-bottom: 6px;
    font-weight: 500;
  }

  .detail-value {
    font-size: 13px;
    color: var(--color-text);
    line-height: 1.5;
  }

  .env-code {
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-sm);
    padding: 16px;
    overflow-x: auto;
    font-size: 12px;
    color: var(--color-text);
    line-height: 1.6;
  }

  .security-note {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-top: 20px;
    padding: 16px;
    background: var(--color-raised);
    border-radius: var(--radius-sm);
    font-size: 13px;
    line-height: 1.6;
    color: var(--color-muted);
  }

  .security-note strong {
    color: var(--color-text);
  }

  .security-note p {
    margin: 4px 0 0;
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
    color: var(--color-success);
    border-color: var(--color-success);
    background: var(--color-success-soft);
  }

  .badge[data-status='disconnected'] {
    color: var(--color-warning);
    border-color: var(--color-warning);
    background: var(--color-warning-soft);
  }

  .muted-note {
    font-size: 13px;
    color: var(--color-muted);
    line-height: 1.6;
  }
</style>
