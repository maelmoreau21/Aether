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
  <div class="login-wrapper">
    <header class="brand-header">
      <div class="brand-badge">
        <svg class="brand-icon" viewBox="0 0 256 256" width="44" height="44" aria-hidden="true">
          <defs><linearGradient id="login-brand" x1="0" y1="0" x2="1" y2="1"><stop offset="0" class="logo-stop-a" /><stop offset="1" class="logo-stop-b" /></linearGradient></defs>
          <path d="M64 192 V48 L216 128 L116 180" fill="none" stroke="url(#login-brand)" stroke-width="30" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
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
    min-height: 100svh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 40px var(--gutter);
    background: var(--bg);
    color: var(--tx);
  }

  .login-wrapper {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 24px;
    width: 100%;
    max-width: 440px;
    min-width: 0;
  }

  .brand-header {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    text-align: center;
  }

  .brand-badge {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }

  .brand-icon { flex: none; }

  .brand-title {
    min-width: 0;
    font-size: 28px;
    font-weight: 800;
    letter-spacing: -0.03em;
    color: var(--tx);
    overflow-wrap: anywhere;
  }

  .brand-tagline {
    margin: 0;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    color: var(--mu);
  }

  .login-card {
    display: flex;
    flex-direction: column;
    gap: 20px;
    min-width: 0;
    padding: 32px;
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: var(--r-xl);
    overflow-wrap: anywhere;
  }

  .card-intro {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
  }

  .badge-setup {
    display: inline-flex;
    align-items: center;
    height: 24px;
    padding: 0 9px;
    border-radius: var(--r-xs);
    background: var(--ac-soft);
    color: var(--ac);
    font-size: 12px;
    font-weight: 700;
  }

  h1 {
    margin: 0;
    font-size: clamp(28px, 4vw, 34px);
    font-weight: 800;
    letter-spacing: -0.03em;
    line-height: 1.1;
    color: var(--tx);
  }

  .card-desc {
    margin: 0;
    font-size: 15px;
    line-height: 1.6;
    color: var(--mu);
  }

  .error-banner {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 14px;
    border-radius: var(--r-sm);
    background: var(--err-soft);
    color: var(--err);
  }

  .error-banner > :global(svg) { flex: none; margin-top: 2px; }

  .error-text {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    line-height: 1.5;
  }

  .error-text code {
    font-family: var(--font-mono);
    font-size: 13px;
  }

  .sso-section {
    display: flex;
    flex-direction: column;
  }

  .btn-sso {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    min-height: 48px;
    padding: 0 16px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--pn-2);
    color: var(--tx);
    font: inherit;
    font-size: 15px;
    font-weight: 700;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }

  .btn-sso:hover:not(:disabled) {
    background: var(--pn-3);
  }

  .btn-sso:active:not(:disabled),
  .btn-submit:active:not(:disabled) {
    transform: scale(0.98);
  }

  .btn-sso:disabled,
  .btn-submit:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .btn-sso-left {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .sso-icon {
    display: grid;
    place-items: center;
    flex: none;
    color: var(--mu);
  }

  .btn-sso-right {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: none;
  }

  .provider-pill {
    display: inline-flex;
    align-items: center;
    height: 24px;
    padding: 0 9px;
    border-radius: var(--r-xs);
    background: var(--pn-2);
    color: var(--mu);
    font-size: 12px;
    font-weight: 700;
  }

  .divider {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .divider-line {
    flex: 1;
    height: 1px;
    background: var(--line);
  }

  .divider-text {
    flex: none;
    font-size: 13px;
    font-weight: 600;
    color: var(--mu);
  }

  .login-form {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .field-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .field-label {
    font-size: 14px;
    font-weight: 700;
    color: var(--tx);
  }

  .field-hint {
    font-size: 13px;
    line-height: 1.5;
    color: var(--mu);
  }

  .input-container {
    position: relative;
    display: flex;
    align-items: center;
  }

  .input-icon {
    position: absolute;
    left: 14px;
    display: grid;
    place-items: center;
    color: var(--mu);
    pointer-events: none;
  }

  input {
    width: 100%;
    height: 46px;
    min-height: 46px;
    padding: 0 14px 0 42px;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background: var(--pn);
    color: var(--tx);
    font: inherit;
    font-size: 15px;
    font-weight: 500;
    box-shadow: none;
    transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease;
  }

  input::placeholder {
    color: var(--mu-2);
  }

  input:hover:not(:disabled) {
    background: var(--pn-2);
  }

  input:focus {
    outline: none;
    border-color: var(--ac);
    background: var(--bg-3);
  }

  input:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .btn-submit {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    width: 100%;
    height: 48px;
    margin-top: 4px;
    padding: 0 22px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--btn);
    color: var(--btn-tx);
    font: inherit;
    font-size: 15px;
    font-weight: 700;
    cursor: pointer;
    transition: opacity var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }

  .btn-submit:hover:not(:disabled) {
    opacity: 0.9;
  }

  .btn-sso:focus-visible,
  .btn-submit:focus-visible,
  input:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  .arrow-glyph {
    display: inline-block;
    transition: transform var(--t-fast) var(--ease);
  }

  .btn-sso:hover:not(:disabled) .arrow-glyph,
  .btn-submit:hover:not(:disabled) .arrow-glyph {
    transform: translateX(3px);
  }

  .card-footer {
    padding-top: 16px;
    border-top: 1px solid var(--line);
  }

  .help-text {
    margin: 0;
    font-size: 13px;
    line-height: 1.6;
    color: var(--mu);
  }

  .site-footer {
    text-align: center;
    font-size: 13px;
    color: var(--mu-2);
  }

  @media (prefers-reduced-motion: reduce) {
    .btn-sso:hover:not(:disabled) .arrow-glyph,
    .btn-submit:hover:not(:disabled) .arrow-glyph { transform: none; }
  }

  @media (max-width: 480px) {
    .login-viewport {
      align-items: flex-start;
      padding-top: 32px;
      padding-bottom: 32px;
    }

    .login-card {
      padding: 20px;
      border-radius: var(--r-lg);
    }

    .brand-title { font-size: 24px; }
  }

  /* La CSP interdit les attributs style : couleurs du dégradé du logo ici. */
  .logo-stop-a { stop-color: var(--logo-a); }
  .logo-stop-b { stop-color: var(--logo-b); }
</style>
