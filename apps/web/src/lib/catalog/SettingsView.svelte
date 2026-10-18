<script lang="ts">
  import { resolve } from '$app/paths';
  import { session } from '../auth/session';
  import OidcSettings from '../auth/OidcSettings.svelte';
  import Icon from '../ui/Icon.svelte';

  let {
    prefAudio = $bindable('fr'),
    prefSubtitles = $bindable('forced'),
    onSavePreference
  }: {
    prefAudio: string;
    prefSubtitles: string;
    onSavePreference: (key: string, value: string) => void;
  } = $props();
</script>

<div class="settings-view">
  <div class="page-heading">
    <span class="eyebrow">VOTRE ESPACE PERSONNEL</span>
    <h1>Profil & Paramètres</h1>
    <p class="heading-sub">Gérez votre identité, vos accès SSO et vos préférences de lecture.</p>
  </div>

  <div class="settings-grid">
    <!-- User Account Info Card -->
    <section class="card profile-card" aria-labelledby="profile-heading">
      <h2 id="profile-heading" class="card-title">Compte utilisateur</h2>
      <div class="profile-header-row">
        <div class="profile-avatar-large" aria-hidden="true">
          <Icon name="profile" size={32} />
        </div>
        <div>
          <h3>{$session?.user.login ?? 'Utilisateur'}</h3>
          <span class="role-badge" class:admin-badge={$session?.user.role === 'administrator'}>
            {$session?.user.role === 'administrator' ? 'Administrateur de l’instance' : 'Utilisateur standard'}
          </span>
        </div>
      </div>

      <div class="profile-info-list">
        <div class="info-row">
          <span class="label">Identifiant unique (UUID)</span>
          <span class="val"><code>{$session?.user.id ?? 'Inconnu'}</code></span>
        </div>
        <div class="info-row">
          <span class="label">Type d’authentification</span>
          <span class="val">
            {$session?.user.login.startsWith('sso-') ? 'SSO (OpenID Connect)' : 'Compte local (hash Argon2id)'}
          </span>
        </div>
        <div class="info-row">
          <span class="label">Sécurité de session</span>
          <span class="val">Cookie <code>HttpOnly</code> strict, rotation CSRF et expiration glissante.</span>
        </div>
      </div>

      {#if $session?.user.role === 'administrator'}
        <div class="admin-shortcut">
          <a class="admin-link" href={resolve('admin/libraries/')}>
            <Icon name="settings" size={16} />
            <span>Ouvrir la console Administrateur</span>
          </a>
        </div>
      {/if}
    </section>

    <!-- OIDC Settings -->
    <section class="card oidc-card" aria-labelledby="sso-settings-heading">
      <h2 id="sso-settings-heading" class="card-title">Liaison de compte SSO (OpenID Connect)</h2>
      <OidcSettings />
    </section>

    <!-- Playback Preferences Card -->
    <section class="card preferences-card" aria-labelledby="pref-heading">
      <h2 id="pref-heading" class="card-title">Préférences de lecture</h2>
      <p class="muted">
        Configurez vos préférences pour la sélection automatique des pistes audio et des sous-titres lors du lancement des médias.
      </p>

      <div class="pref-fields">
        <label>
          <span>Langue audio par défaut</span>
          <select
            bind:value={prefAudio}
            onchange={() => onSavePreference('media_pref_audio', prefAudio)}
          >
            <option value="fr">Français (par défaut)</option>
            <option value="original">Version originale (si disponible)</option>
            <option value="en">Anglais</option>
          </select>
        </label>

        <label>
          <span>Affichage des sous-titres</span>
          <select
            bind:value={prefSubtitles}
            onchange={() => onSavePreference('media_pref_subtitles', prefSubtitles)}
          >
            <option value="forced">Sous-titres forcés uniquement (recommandé)</option>
            <option value="always">Toujours afficher les sous-titres</option>
            <option value="off">Désactivés par défaut</option>
          </select>
        </label>
      </div>

      <p class="pref-note">
        ✓ Vos préférences sont enregistrées sur cet appareil et appliquées automatiquement au lecteur.
      </p>
    </section>
  </div>
</div>

<style>
  .settings-view {
    display: flex;
    flex-direction: column;
    gap: 28px;
    padding-top: 12px;
  }

  .page-heading {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  h1 {
    font-family: var(--font-display);
    font-size: clamp(30px, 3.8vw, 44px);
    font-weight: 750;
    line-height: 1.15;
    letter-spacing: -0.04em;
    color: var(--color-text-bright);
    margin: 0;
  }

  .heading-sub {
    color: var(--color-muted);
    font-size: 14.5px;
    margin: 0;
  }

  .settings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));
    gap: 24px;
  }

  .card {
    padding: 26px 28px;
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    box-shadow: var(--shadow-sm);
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .card-title {
    font-size: 18px;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: var(--color-text-bright);
    margin: 0;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--color-line);
  }

  .profile-header-row {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .profile-avatar-large {
    display: grid;
    place-items: center;
    width: 60px;
    height: 60px;
    border-radius: 50%;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border: 1px solid rgba(167, 139, 250, 0.3);
  }

  h3 {
    font-size: 18px;
    font-weight: 650;
    margin: 0 0 4px;
    color: var(--color-text-bright);
  }

  .role-badge {
    display: inline-block;
    font-size: 11px;
    font-weight: 700;
    padding: 3px 8px;
    border-radius: var(--radius-xs);
    background: rgba(255, 255, 255, 0.08);
    color: var(--color-muted);
  }

  .admin-badge {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    border: 1px solid rgba(167, 139, 250, 0.3);
  }

  .profile-info-list {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .info-row {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .label {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--color-muted);
  }

  .val {
    font-size: 13.5px;
    color: var(--color-text);
  }

  .val code {
    font-family: monospace;
    font-size: 12px;
    padding: 2px 6px;
    border-radius: var(--radius-xs);
    background: var(--color-raised);
    border: 1px solid var(--color-line);
    color: var(--color-accent-hover);
  }

  .admin-shortcut {
    padding-top: 8px;
  }

  .admin-link {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    padding: 10px 18px;
    border-radius: var(--radius-sm);
    background: var(--color-text-bright);
    color: #0b0c13;
    font-size: 13px;
    font-weight: 700;
    text-decoration: none;
    transition: all var(--transition-fast);
  }

  .admin-link:hover {
    background: #ffffff;
    box-shadow: 0 4px 16px rgba(255, 255, 255, 0.2);
    transform: translateY(-1px);
  }

  .muted {
    font-size: 13px;
    line-height: 1.55;
    color: var(--color-muted);
    margin: 0;
  }

  .pref-fields {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .pref-fields label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--color-text);
  }

  .pref-note {
    font-size: 12px;
    color: var(--color-accent);
    margin: 0;
  }
</style>
