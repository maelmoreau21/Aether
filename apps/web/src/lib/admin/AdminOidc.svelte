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
        <p class="card-subtitle">Le texte du bouton de connexion se modifie dans Général → Texte du bouton de connexion SSO.</p>
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
.dashboard-section { margin-bottom: 48px; scroll-margin-top: 24px; font-family: var(--font); color: var(--tx); }
  .section-header { display: flex; align-items: center; gap: 14px; margin-bottom: 20px; }
  .header-icon-wrap { display: grid; place-items: center; flex: none; width: 44px; height: 44px; border-radius: 14px; background: var(--pn); color: var(--tx); }
  .section-title { margin: 0; color: var(--tx); font-size: 24px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.2; }
  .section-subtitle { margin: 4px 0 0; color: var(--mu); font-size: 14px; line-height: 1.5; }
  .card { min-width: 0; padding: 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  .card-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; margin-bottom: 20px; }
  .card-title-group { min-width: 0; }
  .card-title-group h3 { margin: 0 0 6px; font-size: 17px; font-weight: 700; }
  .card-subtitle { margin: 2px 0 0; color: var(--mu); font-size: 13px; line-height: 1.5; }
  .badge { display: inline-flex; align-items: center; flex: none; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--pn-2); color: var(--tx); font-size: 12px; font-weight: 700; white-space: nowrap; }
  .badge[data-status='connected'] { background: var(--ok-soft); color: var(--ok); }
  .badge[data-status='disconnected'] { background: var(--pn-2); color: var(--mu); }
  .details-grid { display: grid; border-top: 1px solid var(--line); }
  .detail-item { display: grid; grid-template-columns: 240px minmax(0, 1fr); align-items: center; gap: 16px; min-height: 60px; padding: 12px 0; border-bottom: 1px solid var(--line); }
  .detail-label { color: var(--tx); font-size: 14px; font-weight: 700; }
  .detail-value { min-width: 0; color: var(--mu); font-size: 14px; overflow-wrap: anywhere; }
  .detail-value code { color: var(--tx); font-family: var(--font-mono); font-size: 13px; }
  .muted-note { margin: 0 0 12px; color: var(--mu); font-size: 14px; line-height: 1.6; }
  .env-code { margin: 0; padding: 16px; overflow-x: auto; border-radius: var(--r-md); background: var(--pn); color: var(--tx); font-family: var(--font-mono); font-size: 13px; line-height: 1.7; }
  .env-code code { font-family: inherit; }
  .security-note { display: flex; align-items: flex-start; gap: 12px; margin-top: 20px; padding: 14px 16px; border-radius: var(--r-md); background: var(--ok-soft); color: var(--tx); font-size: 14px; line-height: 1.5; }
  .security-note :global(svg) { flex: none; margin-top: 2px; color: var(--ok); }
  .security-note strong { display: block; margin-bottom: 4px; font-weight: 700; }
  .security-note p { margin: 0; color: var(--mu); font-size: 13px; }
  .security-note code { font-family: var(--font-mono); font-size: 12px; color: var(--tx); }
  @media (max-width: 820px) {
    .detail-item { grid-template-columns: minmax(0, 1fr); gap: 4px; }
  }
  @media (max-width: 640px) {
    .header-icon-wrap { display: none; }
    .card { padding: 16px; border-radius: var(--r-md); }
    .card-header { flex-direction: column; }
  }
</style>
