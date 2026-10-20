<script lang="ts">
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { DEFAULT_NAME, fetchBrandName } from '../../lib/config/brand';
  import { login, bootstrapAdmin } from '../../lib/auth/session';
  import Icon from '../../lib/ui/Icon.svelte';

  let name = $state(DEFAULT_NAME);
  let identifier = $state('');
  let password = $state('');
  let ssoEnabled = $state(false);
  let ssoProvider = $state('SSO');
  let ssoButtonLabel = $state('Se connecter avec SSO');
  let localLoginEnabled = $state(true);
  let needsBootstrap = $state(false);
  let busy = $state(false);
  let error = $state('');

  // Bootstrap form fields
  let bootstrapLogin = $state('admin');
  let bootstrapPassword = $state('');
  let bootstrapConfirm = $state('');
  let bootstrapToken = $state('');
  let httpBootstrapEnabled = $state(true);

  function mapError(code: string): string {
    switch (code) {
      case 'local_login_disabled':
        return 'La connexion locale est désactivée. Veuillez vous connecter avec le SSO.';
      case 'local_administrator_required':
        return "Un compte administrateur local de secours doit être créé avant d'utiliser le SSO.";
      case 'oidc_invalid_response':
        return 'La réponse du fournisseur SSO est invalide ou la session de connexion a expiré.';
      case 'oidc_group_denied':
        return 'Accès refusé : votre compte SSO ne dispose pas des droits requis (groupe non autorisé).';
      case 'oidc_account_disabled':
        return 'Votre compte utilisateur a été désactivé par un administrateur.';
      case 'oidc_provider_unavailable':
        return 'Le fournisseur d’identité SSO est indisponible ou injoignable.';
      case 'oidc_disabled':
        return 'L’authentification SSO (OpenID Connect) n’est pas activée sur ce serveur.';
      case 'oidc_identity_already_linked':
        return 'Cette identité SSO est déjà liée à un autre compte utilisateur.';
      case 'link_session_expired':
        return 'La session temporaire de liaison de compte a expiré.';
      case 'try_later':
        return 'Trop de tentatives récentes. Veuillez patienter avant de réessayer.';
      case 'session_expired':
        return 'Votre session a expiré. Veuillez vous reconnecter.';
      case 'authentication_required':
        return 'Authentification requise pour accéder à votre médiathèque.';
      case 'bootstrap_closed':
        return "L'initialisation du serveur est déjà clôturée.";
      case 'invalid_bootstrap_token':
        return "Code d'initialisation invalide. Consultez la console du serveur ou le fichier .bootstrap_token.";
      case 'http_bootstrap_disabled':
        return "L'initialisation via le Web est désactivée. Utilisez la commande locale 'media-bootstrap'.";
      default:
        return code ? decodeURIComponent(code) : 'Une erreur est survenue lors de l’authentification.';
    }
  }

  async function sso() {
    if (busy) return;
    busy = true;
    error = '';
    try {
      const response = await fetch('/api/v1/auth/oidc/start', {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        signal: AbortSignal.timeout(15000)
      });
      if (!response.ok) {
        const data = await response.json().catch(() => null);
        const code = data?.error;
        throw new Error(code ? mapError(code) : 'Le SSO est indisponible. Réessayez ou utilisez votre compte local.');
      }
      const data: { url: string } = await response.json();
      if (!data.url) throw new Error('URL de redirection SSO invalide.');
      window.location.assign(data.url);
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'Connexion SSO indisponible.';
      busy = false;
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    busy = true;
    error = '';
    try {
      await login(identifier, password);
      password = '';
      await goto(resolve('/'), { replaceState: true });
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'La connexion est indisponible. Réessayez.';
    } finally {
      password = '';
      busy = false;
    }
  }

  async function submitBootstrap(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    if (!bootstrapToken.trim()) {
      error = "Veuillez renseigner le code d'initialisation affiché dans la console du serveur.";
      return;
    }
    if (bootstrapPassword.length < 15) {
      error = 'Le mot de passe doit comporter au moins 15 caractères.';
      return;
    }
    if (bootstrapPassword !== bootstrapConfirm) {
      error = 'La confirmation du mot de passe ne correspond pas.';
      return;
    }
    busy = true;
    error = '';
    try {
      await bootstrapAdmin(bootstrapLogin.trim().toLowerCase(), bootstrapPassword, bootstrapToken.trim());
      bootstrapPassword = '';
      bootstrapConfirm = '';
      bootstrapToken = '';
      await goto(resolve('/'), { replaceState: true });
    } catch (failure) {
      error = failure instanceof Error ? failure.message : "Échec de l'initialisation.";
    } finally {
      bootstrapPassword = '';
      bootstrapConfirm = '';
      busy = false;
    }
  }

  $effect(() => {
    const controller = new AbortController();

    // Check query params for redirected errors
    const errParam = page.url.searchParams.get('error');
    if (errParam) {
      error = mapError(errParam);
    }

    // Check bootstrap status (is this the first run?)
    fetch('/api/v1/auth/bootstrap/status', { signal: controller.signal })
      .then(r => r.ok ? r.json() : { needsBootstrap: false })
      .then(data => {
        needsBootstrap = data.needsBootstrap === true;
        if (typeof data.localLoginEnabled === 'boolean') {
          localLoginEnabled = data.localLoginEnabled;
        }
        if (typeof data.httpBootstrapEnabled === 'boolean') {
          httpBootstrapEnabled = data.httpBootstrapEnabled;
        }
      })
      .catch(() => {});

    // Check OIDC status
    fetch('/api/v1/auth/oidc/status', { signal: controller.signal })
      .then(r => r.ok ? r.json() : { enabled: false })
      .then(data => {
        ssoEnabled = data.enabled === true;
        if (typeof data.buttonLabel === 'string' && data.buttonLabel.trim()) {
          ssoButtonLabel = data.buttonLabel;
        }
        if (data.provider && typeof data.provider === 'string') {
          ssoProvider = data.provider;
        }
        if (typeof data.localLoginEnabled === 'boolean') {
          localLoginEnabled = data.localLoginEnabled;
        }
      })
      .catch(() => {});

    fetchBrandName(controller.signal).then(brandName => {
      name = brandName;
    });

    return () => controller.abort();
  });
</script>

<svelte:head>
  <title>{needsBootstrap ? 'Initialisation' : 'Connexion'} · {name}</title>
  <meta name="description" content="Connectez-vous à votre médiathèque Aether." />
</svelte:head>

<main class="login-viewport">
  <div class="ambient-glow" aria-hidden="true"></div>

  <div class="login-wrapper">
    <header class="brand-header">
      <div class="brand-badge">
        <img
          src={`${resolve('/').replace(/\/$/, '')}/images/logo-animated.svg`}
          alt=""
          width="40"
          height="48"
          class="brand-icon"
        />
        <span class="brand-title">{name}</span>
      </div>
      <p class="brand-tagline">VOTRE CINÉMA, CHEZ VOUS</p>
    </header>

    {#if needsBootstrap}
      <!-- Premier démarrage : création de l'administrateur initial -->
      <section class="login-card" aria-labelledby="setup-title">
        <div class="card-intro">
          <span class="badge-setup">Premier démarrage</span>
          <h1 id="setup-title">Créer l'administrateur</h1>
          <p class="card-desc">
            Bienvenue ! Aucun compte n'a encore été configuré. Définissez votre compte administrateur local de secours pour initialiser Aether (requis avant d'activer le SSO).
          </p>
        </div>

        {#if error}
          <div class="error-banner" role="alert">
            <Icon name="alert" size={18} />
            <p class="error-text">{error}</p>
          </div>
        {/if}

        {#if !httpBootstrapEnabled}
          <div class="error-banner" role="alert">
            <Icon name="alert" size={18} />
            <p class="error-text">
              L'initialisation via le Web est désactivée. Veuillez exécuter la commande locale <code>media-bootstrap</code> dans un terminal pour créer l'administrateur.
            </p>
          </div>
        {:else}
        <form class="login-form" onsubmit={submitBootstrap} aria-busy={busy}>
          <div class="field-group">
            <label for="bootstrap-token" class="field-label">
              <span>Code d'initialisation (Jeton serveur)</span>
            </label>
            <div class="input-container">
              <span class="input-icon" aria-hidden="true"><Icon name="settings" size={17} /></span>
              <input
                id="bootstrap-token"
                name="bootstrap-token"
                type="password"
                autocomplete="off"
                spellcheck="false"
                required
                bind:value={bootstrapToken}
                disabled={busy}
                placeholder="Code affiché dans la console du serveur"
              />
            </div>
            <span class="field-hint">Consultez les journaux du serveur ou le fichier .bootstrap_token.</span>
          </div>

          <div class="field-group">
            <label for="bootstrap-login" class="field-label">
              <span>Identifiant administrateur</span>
            </label>
            <div class="input-container">
              <span class="input-icon" aria-hidden="true"><Icon name="profile" size={17} /></span>
              <input
                id="bootstrap-login"
                name="bootstrap-login"
                autocomplete="username"
                autocapitalize="none"
                spellcheck="false"
                maxlength="64"
                required
                bind:value={bootstrapLogin}
                disabled={busy}
                placeholder="Ex. admin"
              />
            </div>
            <span class="field-hint">Lettres minuscules, chiffres, tirets et points.</span>
          </div>

          <div class="field-group">
            <label for="bootstrap-password" class="field-label">
              <span>Mot de passe (15 caractères min.)</span>
            </label>
            <div class="input-container">
              <span class="input-icon" aria-hidden="true"><Icon name="settings" size={17} /></span>
              <input
                id="bootstrap-password"
                name="bootstrap-password"
                type="password"
                autocomplete="new-password"
                minlength="15"
                maxlength="512"
                required
                bind:value={bootstrapPassword}
                disabled={busy}
                placeholder="••••••••••••••••"
              />
            </div>
            <span class="field-hint">Exigence de sécurité du Core : au moins 15 caractères.</span>
          </div>

          <div class="field-group">
            <label for="bootstrap-confirm" class="field-label">
              <span>Confirmer le mot de passe</span>
            </label>
            <div class="input-container">
              <span class="input-icon" aria-hidden="true"><Icon name="settings" size={17} /></span>
              <input
                id="bootstrap-confirm"
                name="bootstrap-confirm"
                type="password"
                autocomplete="new-password"
                minlength="15"
                maxlength="512"
                required
                bind:value={bootstrapConfirm}
                disabled={busy}
                placeholder="••••••••••••••••"
              />
            </div>
          </div>

          <button type="submit" class="btn-submit" disabled={busy}>
            <span class="btn-submit-text">{busy ? 'Initialisation en cours…' : 'Initialiser le serveur'}</span>
            <span class="arrow-glyph" aria-hidden="true">→</span>
          </button>
        </form>
        {/if}

        <footer class="card-footer">
          <p class="help-text">
            Ce compte est unique et définitif. Conservez précieusement ces identifiants pour les opérations de maintenance.
          </p>
        </footer>
      </section>

    {:else}
      <!-- Connexion standard (SSO ou compte local) -->
      <section class="login-card" aria-labelledby="login-title">
        <div class="card-intro">
          <h1 id="login-title">Bienvenue chez vous.</h1>
          <p class="card-desc">Connectez-vous pour retrouver vos films, séries et votre progression.</p>
        </div>

        {#if error}
          <div class="error-banner" role="alert">
            <Icon name="alert" size={18} />
            <p class="error-text">{error}</p>
          </div>
        {/if}

        {#if ssoEnabled}
          <div class="sso-section">
            <button
              type="button"
              class="btn-sso"
              onclick={sso}
              disabled={busy}
              aria-label={ssoButtonLabel}
            >
              <span class="btn-sso-left">
                <span class="sso-icon"><Icon name="shield" size={20} /></span>
                <span class="btn-sso-text">{ssoButtonLabel}</span>
              </span>
              <span class="btn-sso-right">
                {#if ssoProvider && ssoProvider !== 'SSO'}
                  <span class="provider-pill" aria-hidden="true">{ssoProvider}</span>
                {/if}
                <span class="arrow-glyph" aria-hidden="true">→</span>
              </span>
            </button>
          </div>

          {#if localLoginEnabled}
            <div class="divider" role="separator">
              <span class="divider-line"></span>
              <span class="divider-text">ou avec un identifiant local</span>
              <span class="divider-line"></span>
            </div>
          {/if}
        {/if}

        {#if localLoginEnabled}
          <form class="login-form" onsubmit={submit} aria-busy={busy}>
            <div class="field-group">
              <label for="login" class="field-label">
                <span>Identifiant</span>
              </label>
              <div class="input-container">
                <span class="input-icon" aria-hidden="true"><Icon name="profile" size={17} /></span>
                <input
                  id="login"
                  name="username"
                  autocomplete="username"
                  autocapitalize="none"
                  spellcheck="false"
                  maxlength="64"
                  required
                  bind:value={identifier}
                  disabled={busy}
                  placeholder="Ex. nom d’utilisateur"
                />
              </div>
            </div>

            <div class="field-group">
              <label for="password" class="field-label">
                <span>Mot de passe</span>
              </label>
              <div class="input-container">
                <span class="input-icon" aria-hidden="true"><Icon name="settings" size={17} /></span>
                <input
                  id="password"
                  name="password"
                  type="password"
                  autocomplete="current-password"
                  maxlength="512"
                  required
                  bind:value={password}
                  disabled={busy}
                  placeholder="••••••••••••"
                />
              </div>
            </div>

            <button type="submit" class="btn-submit" disabled={busy}>
              <span class="btn-submit-text">{busy ? 'Connexion en cours…' : 'Se connecter'}</span>
              <span class="arrow-glyph" aria-hidden="true">→</span>
            </button>
          </form>
        {/if}

        <footer class="card-footer">
          {#if !localLoginEnabled}
            <p class="help-text">
              La connexion locale est désactivée sur ce serveur. L'authentification s'effectue exclusivement via votre fournisseur SSO ({ssoProvider}).
            </p>
          {:else if ssoEnabled}
            <p class="help-text">
              Authentification unique (SSO) activée. Le compte local d'administration reste toujours disponible en secours.
            </p>
          {:else}
            <p class="help-text">
              Utilisez le compte local créé lors de l’installation de votre serveur.
            </p>
          {/if}
        </footer>
      </section>
    {/if}

    <footer class="site-footer">
      <span>{name} · Votre cinéma, autrement.</span>
    </footer>
  </div>
</main>

<style>
  .login-viewport {
    position: relative;
    min-height: 100svh;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 32px 20px;
    background-color: var(--color-canvas, #090a10);
    overflow-x: hidden;
  }

  .ambient-glow {
    position: absolute;
    top: 15%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 650px;
    height: 450px;
    background: radial-gradient(
      ellipse at center,
      rgba(139, 92, 246, 0.16) 0%,
      rgba(99, 102, 241, 0.08) 45%,
      transparent 70%
    );
    filter: blur(70px);
    pointer-events: none;
    z-index: 0;
  }

  .login-wrapper {
    position: relative;
    z-index: 1;
    width: 100%;
    max-width: 440px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 28px;
  }

  .brand-header {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 8px;
  }

  .brand-badge {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .brand-icon {
    filter: drop-shadow(0 4px 12px rgba(167, 139, 250, 0.35));
  }

  .brand-title {
    font-size: 32px;
    font-weight: 700;
    letter-spacing: -0.05em;
    color: var(--color-text-bright, #ffffff);
  }

  .brand-tagline {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.18em;
    color: var(--color-accent, #a78bfa);
    text-transform: uppercase;
    margin: 0;
  }

  .login-card {
    width: 100%;
    background: var(--color-surface, #11131c);
    border: 1px solid var(--color-line-strong, rgba(255, 255, 255, 0.12));
    border-radius: var(--radius-lg, 16px);
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.65), 0 0 0 1px rgba(255, 255, 255, 0.04);
    backdrop-filter: blur(16px);
    padding: 36px 32px;
    display: flex;
    flex-direction: column;
    gap: 22px;
  }

  .card-intro {
    text-align: left;
  }

  .badge-setup {
    display: inline-block;
    font-size: 11px;
    font-weight: 650;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    padding: 4px 10px;
    background: rgba(167, 139, 250, 0.2);
    border: 1px solid rgba(167, 139, 250, 0.4);
    border-radius: var(--radius-full, 9999px);
    color: var(--color-accent, #a78bfa);
    margin-bottom: 12px;
  }

  h1 {
    font-size: 24px;
    font-weight: 600;
    letter-spacing: -0.03em;
    color: var(--color-text-bright, #ffffff);
    margin: 0 0 6px 0;
  }

  .card-desc {
    font-size: 13px;
    line-height: 1.5;
    color: var(--color-muted, #94a3b8);
    margin: 0;
  }

  .error-banner {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    background: rgba(248, 113, 113, 0.12);
    border: 1px solid rgba(248, 113, 113, 0.35);
    border-radius: var(--radius-sm, 8px);
    padding: 12px 14px;
    color: var(--color-danger, #f87171);
  }

  .error-text {
    font-size: 12px;
    line-height: 1.5;
    margin: 0;
    font-weight: 500;
  }

  .sso-section {
    display: flex;
    flex-direction: column;
  }

  .btn-sso {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    min-height: 52px;
    padding: 12px 18px;
    background: linear-gradient(135deg, rgba(139, 92, 246, 0.22) 0%, rgba(99, 102, 241, 0.18) 100%);
    border: 1px solid rgba(167, 139, 250, 0.4);
    border-radius: var(--radius-sm, 8px);
    color: var(--color-text-bright, #ffffff);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: all var(--transition-normal, 200ms ease);
    box-shadow: 0 4px 14px rgba(139, 92, 246, 0.18);
  }

  .btn-sso:hover:not(:disabled) {
    background: linear-gradient(135deg, rgba(139, 92, 246, 0.35) 0%, rgba(99, 102, 241, 0.3) 100%);
    border-color: var(--color-accent, #a78bfa);
    box-shadow: 0 6px 20px rgba(139, 92, 246, 0.3);
    transform: translateY(-1px);
  }

  .btn-sso:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .btn-sso-left {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .sso-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--color-accent, #a78bfa);
  }

  .btn-sso-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .provider-pill {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 3px 8px;
    background: rgba(167, 139, 250, 0.25);
    border: 1px solid rgba(167, 139, 250, 0.4);
    border-radius: var(--radius-full, 9999px);
    color: var(--color-accent-hover, #c4b5fd);
  }

  .divider {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
  }

  .divider-line {
    flex: 1;
    height: 1px;
    background: var(--color-line, rgba(255, 255, 255, 0.08));
  }

  .divider-text {
    font-size: 11px;
    color: var(--color-subtle, #64748b);
    text-transform: lowercase;
    letter-spacing: 0.02em;
    white-space: nowrap;
  }

  .login-form {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .field-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--color-text, #f8fafc);
  }

  .field-hint {
    font-size: 11px;
    color: var(--color-subtle, #64748b);
  }

  .input-container {
    position: relative;
    display: flex;
    align-items: center;
  }

  .input-icon {
    position: absolute;
    left: 14px;
    color: var(--color-subtle, #64748b);
    pointer-events: none;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  input {
    width: 100%;
    min-height: 48px;
    padding: 12px 14px 12px 42px;
    background: var(--color-raised, #181a25);
    border: 1px solid var(--color-line-strong, rgba(255, 255, 255, 0.12));
    border-radius: var(--radius-sm, 8px);
    color: var(--color-text-bright, #ffffff);
    font-size: 13px;
    transition: all var(--transition-fast, 150ms ease);
  }

  input::placeholder {
    color: var(--color-subtle, #64748b);
  }

  input:focus {
    outline: none;
    border-color: var(--color-accent, #a78bfa);
    box-shadow: 0 0 0 3px rgba(167, 139, 250, 0.2);
    background: var(--color-raised-hover, #222534);
  }

  input:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .btn-submit {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    min-height: 50px;
    padding: 12px 18px;
    background: var(--color-accent, #a78bfa);
    color: #171123;
    border: none;
    border-radius: var(--radius-sm, 8px);
    font-size: 14px;
    font-weight: 650;
    cursor: pointer;
    margin-top: 4px;
    transition: all var(--transition-normal, 200ms ease);
    box-shadow: 0 4px 14px rgba(167, 139, 250, 0.25);
  }

  .btn-submit:hover:not(:disabled) {
    background: var(--color-accent-hover, #c4b5fd);
    box-shadow: 0 6px 20px rgba(167, 139, 250, 0.4);
    transform: translateY(-1px);
  }

  .btn-submit:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .arrow-glyph {
    font-size: 16px;
    font-weight: 700;
    transition: transform var(--transition-fast, 150ms ease);
  }

  .btn-sso:hover:not(:disabled) .arrow-glyph,
  .btn-submit:hover:not(:disabled) .arrow-glyph {
    transform: translateX(3px);
  }

  .card-footer {
    border-top: 1px solid var(--color-line, rgba(255, 255, 255, 0.08));
    padding-top: 16px;
    margin-top: 2px;
  }

  .help-text {
    font-size: 11px;
    line-height: 1.6;
    color: var(--color-subtle, #64748b);
    margin: 0;
    text-align: center;
  }

  .site-footer {
    font-size: 11px;
    color: var(--color-subtle, #64748b);
    text-align: center;
  }

  @media (max-width: 480px) {
    .login-card {
      padding: 28px 22px;
    }
    h1 {
      font-size: 21px;
    }
  }
</style>
