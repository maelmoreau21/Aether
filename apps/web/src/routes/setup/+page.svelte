<script lang="ts">
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import Icon from '../../lib/ui/Icon.svelte';
  import PathPickerModal from '../../lib/admin/PathPickerModal.svelte';
  import { readSession, session } from '../../lib/auth/session';
  import { DEFAULT_NAME, fetchBrandName } from '../../lib/config/brand';
  import { t } from '../../lib/i18n';
  import { markAccountChosen } from '../../lib/auth/accounts';

  type Step = 'loading' | 'admin' | 'library' | 'ready' | 'done';
  type LibraryKind = 'films' | 'series' | 'music';

  let step = $state<Step>('loading');
  let name = $state(DEFAULT_NAME);
  let httpBootstrapEnabled = $state(true);

  // Step 1: Administrator creation
  let bootstrapToken = $state('');
  let bootstrapLogin = $state('');
  let bootstrapPassword = $state('');
  let bootstrapConfirm = $state('');
  let busy = $state(false);
  let error = $state('');

  // Step 2: First media library + read permission check
  let libraryKind = $state<LibraryKind>('films');
  let libraryName = $state('Films');
  let nameTouched = $state(false);
  let libraryRoot = $state('');
  let localRoots = $state<string[]>([]);
  let pickerOpen = $state(false);
  let checking = $state(false);
  let checkResult = $state<{
    readable: boolean;
    path: string;
    entriesCount: number;
    mediaFilesCount: number;
    error?: string;
  } | null>(null);
  let createdLibraryId = $state('');

  function defaultNameForKind(kind: LibraryKind): string {
    if (kind === 'series') return $t('kind_series');
    if (kind === 'music') return $t('music');
    return $t('kind_movies');
  }

  function selectKind(kind: LibraryKind) {
    libraryKind = kind;
    if (!nameTouched) libraryName = defaultNameForKind(kind);
  }

  async function loadRoots() {
    try {
      const response = await fetch('/api/v1/admin/roots', {
        credentials: 'same-origin',
        cache: 'no-store'
      });
      if (response.ok) {
        const data = await response.json() as { localRoots?: string[]; roots?: string[] };
        localRoots = data.localRoots ?? data.roots ?? [];
        if (!libraryRoot && localRoots.length > 0) {
          libraryRoot = localRoots[0];
        }
      }
    } catch {
      // Roots are optional hints; the user can still type or browse.
    }
  }

  $effect(() => {
    const controller = new AbortController();
    fetchBrandName(controller.signal).then(brandName => { name = brandName; });

    Promise.all([
      fetch('/api/v1/auth/bootstrap/status', { signal: controller.signal, cache: 'no-store' })
        .then(r => r.ok ? r.json() as Promise<{ needsBootstrap?: boolean; hasLibraries?: boolean; httpBootstrapEnabled?: boolean }> : null)
        .catch(() => null),
      readSession().catch(() => null)
    ]).then(([status, current]) => {
      if (controller.signal.aborted) return;
      if (typeof status?.httpBootstrapEnabled === 'boolean') {
        httpBootstrapEnabled = status.httpBootstrapEnabled;
      }
      if (status?.needsBootstrap) {
        step = 'admin';
      } else if (current?.user?.role === 'administrator' && status?.hasLibraries === false) {
        step = 'library';
        void loadRoots();
      } else if (current) {
        step = 'done';
      } else {
        void goto(resolve('login/'), { replaceState: true });
      }
    });

    return () => controller.abort();
  });

  async function submitBootstrap(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    error = '';

    if (!bootstrapToken.trim()) {
      error = $t('please_enter_the_initialisation_code_shown');
      return;
    }
    if (bootstrapPassword.length < 15) {
      error = $t('the_password_must_be_at_least');
      return;
    }
    if (bootstrapPassword !== bootstrapConfirm) {
      error = $t('the_password_confirmation_does_not_match');
      return;
    }

    busy = true;
    try {
      const response = await fetch('/api/v1/auth/bootstrap', {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          login: bootstrapLogin.trim(),
          password: bootstrapPassword,
          bootstrap_token: bootstrapToken.trim()
        })
      });

      if (!response.ok) {
        const body = await response.json().catch(() => ({} as { error?: string }));
        error = body.error ?? $t('initialisation_failed');
        return;
      }

      markAccountChosen();
      await readSession(true);
      step = 'library';
      await loadRoots();
    } catch {
      error = $t('initialisation_failed_try_again');
    } finally {
      bootstrapPassword = '';
      bootstrapConfirm = '';
      busy = false;
    }
  }

  async function testPermissions() {
    if (checking || !libraryRoot.trim()) return;
    checking = true;
    error = '';
    checkResult = null;
    try {
      const csrf = $session?.csrfToken ?? '';
      const response = await fetch('/api/v1/admin/filesystem/check', {
        method: 'POST',
        credentials: 'same-origin',
        headers: {
          'Content-Type': 'application/json',
          'x-csrf-token': csrf
        },
        body: JSON.stringify({ path: libraryRoot.trim(), nodeId: 'local' })
      });
      if (!response.ok) {
        error = $t('operation_failed');
        return;
      }
      checkResult = await response.json();
      if (checkResult?.readable && checkResult.path) {
        libraryRoot = checkResult.path;
      }
    } catch {
      error = $t('operation_failed');
    } finally {
      checking = false;
    }
  }

  async function submitLibrary(event: SubmitEvent) {
    event.preventDefault();
    if (busy || !libraryName.trim() || !libraryRoot.trim()) return;
    busy = true;
    error = '';
    try {
      const csrf = $session?.csrfToken ?? '';
      const response = await fetch('/api/v1/admin/libraries', {
        method: 'POST',
        credentials: 'same-origin',
        headers: {
          'Content-Type': 'application/json',
          'x-csrf-token': csrf
        },
        body: JSON.stringify({
          name: libraryName.trim(),
          kind: libraryKind,
          nodeId: 'local',
          root: libraryRoot.trim(),
          enabled: true
        })
      });
      if (!response.ok) {
        const body = await response.json().catch(() => ({} as { error?: string }));
        error = body.error ?? $t('operation_failed');
        return;
      }
      const created = await response.json() as { id: string };
      createdLibraryId = created.id;
      await fetch(`/api/v1/admin/libraries/${encodeURIComponent(created.id)}/scan`, {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json', 'x-csrf-token': csrf },
        body: JSON.stringify({ mode: 'new_only' })
      }).catch(() => {});
      step = 'ready';
    } catch {
      error = $t('operation_failed');
    } finally {
      busy = false;
    }
  }
</script>

<svelte:head>
  <title>{$t('setup_wizard_title')} · {name}</title>
</svelte:head>

<main class="setup-viewport">
  <div class="setup-wrapper">
    <header class="brand-header">
      <div class="brand-badge">
        <svg class="brand-icon" viewBox="0 0 256 256" width="44" height="44" aria-hidden="true">
          <defs><linearGradient id="setup-brand" x1="0" y1="0" x2="1" y2="1"><stop offset="0" class="logo-stop-a" /><stop offset="1" class="logo-stop-b" /></linearGradient></defs>
          <path d="M64 192 V48 L216 128 L116 180" fill="none" stroke="url(#setup-brand)" stroke-width="30" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
        <span class="brand-title">{name}</span>
      </div>
      <p class="brand-tagline">{$t('your_cinema_at_home')}</p>
    </header>

    {#if step !== 'loading' && step !== 'done'}
      <ol class="stepper" aria-label={$t('setup_wizard_title')}>
        <li class:active={step === 'admin'} class:completed={step === 'library' || step === 'ready'}>{$t('setup_step_admin')}</li>
        <li class:active={step === 'library'} class:completed={step === 'ready'}>{$t('setup_step_library')}</li>
        <li class:active={step === 'ready'}>{$t('setup_step_ready')}</li>
      </ol>
    {/if}

    {#if step === 'loading'}
      <section class="setup-card" aria-busy="true">
        <p class="card-desc">{$t('loading')}</p>
      </section>
    {:else if step === 'admin'}
      <section class="setup-card" aria-labelledby="setup-admin-title">
        <div class="card-intro">
          <span class="badge-setup">{$t('first_start')}</span>
          <h1 id="setup-admin-title">{$t('create_the_administrator')}</h1>
          <p class="card-desc">{$t('welcome_no_account_has_been_configured')}</p>
        </div>

        {#if error}
          <div class="error-banner" role="alert">
            <Icon name="alert" size={18} />
            <p>{error}</p>
          </div>
        {/if}

        {#if !httpBootstrapEnabled}
          <div class="error-banner" role="alert">
            <Icon name="alert" size={18} />
            <p>{$t('web_initialisation_is_disabled_please_run')} <code>media-bootstrap</code> {$t('in_a_terminal_to_create_the')}</p>
          </div>
        {:else}
          <form class="setup-form" onsubmit={submitBootstrap} aria-busy={busy}>
            <div class="field-group">
              <label for="setup-token">{$t('initialisation_code_server_token')}</label>
              <input id="setup-token" type="password" autocomplete="off" spellcheck="false" required bind:value={bootstrapToken} disabled={busy} placeholder={$t('code_shown_in_the_server_console')} />
              <p class="field-hint">{$t('check_the_server_logs_or_the')}</p>
            </div>
            <div class="field-group">
              <label for="setup-login">{$t('administrator_username')}</label>
              <input id="setup-login" type="text" autocomplete="username" autocapitalize="none" spellcheck="false" required bind:value={bootstrapLogin} disabled={busy} placeholder={$t('e_g_admin')} />
              <p class="field-hint">{$t('lowercase_letters_digits_hyphens_and_dots')}</p>
            </div>
            <div class="field-group">
              <label for="setup-password">{$t('password_15_characters_min')}</label>
              <input id="setup-password" type="password" autocomplete="new-password" minlength={15} required bind:value={bootstrapPassword} disabled={busy} placeholder="•••••••••••••••" />
              <p class="field-hint">{$t('core_security_requirement_at_least_15')}</p>
            </div>
            <div class="field-group">
              <label for="setup-confirm">{$t('confirm_the_password')}</label>
              <input id="setup-confirm" type="password" autocomplete="new-password" minlength={15} required bind:value={bootstrapConfirm} disabled={busy} placeholder="•••••••••••••••" />
            </div>
            <button type="submit" class="btn-primary" disabled={busy}>
              {busy ? $t('initialising') : $t('initialise_the_server')}
            </button>
          </form>
        {/if}
      </section>
    {:else if step === 'library'}
      <section class="setup-card" aria-labelledby="setup-library-title">
        <div class="card-intro">
          <span class="badge-setup">{$t('setup_step_library')}</span>
          <h1 id="setup-library-title">{$t('setup_library_heading')}</h1>
          <p class="card-desc">{$t('setup_library_intro')}</p>
        </div>

        {#if error}
          <div class="error-banner" role="alert">
            <Icon name="alert" size={18} />
            <p>{error}</p>
          </div>
        {/if}

        <form class="setup-form" onsubmit={submitLibrary} aria-busy={busy}>
          <div class="field-group">
            <span class="field-label">{$t('setup_library_kind')}</span>
            <div class="kind-grid" role="radiogroup" aria-label={$t('setup_library_kind')}>
              <button type="button" class="kind-chip" class:selected={libraryKind === 'films'} onclick={() => selectKind('films')}>{$t('kind_movies')}</button>
              <button type="button" class="kind-chip" class:selected={libraryKind === 'series'} onclick={() => selectKind('series')}>{$t('kind_series')}</button>
              <button type="button" class="kind-chip" class:selected={libraryKind === 'music'} onclick={() => selectKind('music')}>{$t('music')}</button>
            </div>
          </div>

          <div class="field-group">
            <label for="setup-lib-name">{$t('setup_library_name')}</label>
            <input id="setup-lib-name" type="text" required maxlength={80} bind:value={libraryName} oninput={() => { nameTouched = true; }} disabled={busy} />
          </div>

          <div class="field-group">
            <label for="setup-lib-path">{$t('setup_library_path')}</label>
            <div class="path-row">
              <input id="setup-lib-path" type="text" required bind:value={libraryRoot} oninput={() => { checkResult = null; }} disabled={busy} placeholder="/media" />
              <button type="button" class="btn-secondary" onclick={() => { pickerOpen = true; }} disabled={busy}>{$t('setup_browse')}</button>
            </div>
            {#if localRoots.length > 0}
              <div class="roots-row">
                <span class="field-hint">{$t('setup_detected_roots')} :</span>
                {#each localRoots as root (root)}
                  <button type="button" class="root-chip" onclick={() => { libraryRoot = root; checkResult = null; }}>{root}</button>
                {/each}
              </div>
            {/if}
          </div>

          <div class="check-row">
            <button type="button" class="btn-secondary" onclick={testPermissions} disabled={checking || busy || !libraryRoot.trim()}>
              {checking ? $t('setup_testing_permissions') : $t('setup_test_permissions')}
            </button>
          </div>

          {#if checkResult}
            {#if checkResult.readable}
              <div class="success-banner" role="status">
                <p>{$t('setup_permissions_ok', { entries: checkResult.entriesCount, media: checkResult.mediaFilesCount })}</p>
              </div>
            {:else}
              <div class="error-banner" role="alert">
                <Icon name="alert" size={18} />
                <p>{$t('setup_permissions_failed', { reason: checkResult.error ?? $t('operation_failed') })}</p>
              </div>
            {/if}
          {/if}

          <div class="actions-row">
            <button type="submit" class="btn-primary" disabled={busy || !libraryName.trim() || !libraryRoot.trim()}>
              {busy ? $t('setup_creating_library') : $t('setup_create_and_scan')}
            </button>
            <a class="btn-ghost" href={resolve('/')}>{$t('setup_skip_library')}</a>
          </div>
        </form>
      </section>
    {:else if step === 'ready'}
      <section class="setup-card" aria-labelledby="setup-ready-title">
        <div class="card-intro">
          <span class="badge-setup">{$t('setup_step_ready')}</span>
          <h1 id="setup-ready-title">{$t('setup_ready_heading')}</h1>
          <p class="card-desc">{$t('setup_ready_intro')}</p>
        </div>
        <div class="actions-row">
          <a class="btn-primary" href={resolve('/')}>{$t('setup_open_catalog')}</a>
          {#if createdLibraryId}
            <a class="btn-secondary" href={resolve('admin/libraries/#libraries-section')}>{$t('setup_open_admin')}</a>
          {/if}
        </div>
      </section>
    {:else}
      <section class="setup-card" aria-labelledby="setup-done-title">
        <div class="card-intro">
          <h1 id="setup-done-title">{$t('setup_already_configured')}</h1>
        </div>
        <div class="actions-row">
          <a class="btn-primary" href={resolve('/')}>{$t('setup_open_catalog')}</a>
        </div>
      </section>
    {/if}
  </div>
</main>

<PathPickerModal
  open={pickerOpen}
  initialPath={libraryRoot}
  nodeId="local"
  onselect={(selected) => { libraryRoot = selected; checkResult = null; pickerOpen = false; }}
  onclose={() => { pickerOpen = false; }}
/>

<style>
  .setup-viewport {
    min-height: 100dvh;
    display: grid;
    place-items: center;
    padding: 32px 20px;
    background: var(--color-canvas);
    color: var(--color-text);
    font-family: var(--font-sans);
  }
  .setup-wrapper {
    width: 100%;
    max-width: 520px;
    display: grid;
    gap: 24px;
  }
  .brand-header {
    display: grid;
    justify-items: center;
    gap: 8px;
    text-align: center;
  }
  .brand-badge {
    display: inline-flex;
    align-items: center;
    gap: 12px;
  }
  .brand-title {
    font-size: 28px;
    font-weight: 800;
    letter-spacing: -0.03em;
  }
  .brand-tagline {
    margin: 0;
    font-size: 12px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--color-muted);
  }
  .stepper {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .stepper li {
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    border: 1px solid var(--color-line-subtle);
    color: var(--color-muted);
    font-size: 12px;
    font-weight: 600;
    text-align: center;
  }
  .stepper li.active {
    border-color: var(--color-accent);
    color: var(--color-text);
  }
  .stepper li.completed {
    color: var(--color-accent);
  }
  .setup-card {
    display: grid;
    gap: 20px;
    padding: 28px;
    border-radius: var(--radius-lg, 14px);
    background: var(--color-surface);
    border: 1px solid var(--color-line-subtle);
  }
  .card-intro {
    display: grid;
    gap: 8px;
  }
  .badge-setup {
    justify-self: start;
    padding: 3px 10px;
    border-radius: var(--r-pill, 999px);
    background: color-mix(in srgb, var(--color-accent) 16%, transparent);
    color: var(--color-accent);
    font-size: 12px;
    font-weight: 700;
  }
  h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 750;
    letter-spacing: -0.02em;
  }
  .card-desc {
    margin: 0;
    font-size: 14px;
    line-height: 1.5;
    color: var(--color-muted);
  }
  .setup-form {
    display: grid;
    gap: 16px;
  }
  .field-group {
    display: grid;
    gap: 6px;
  }
  label, .field-label {
    font-size: 13px;
    font-weight: 600;
  }
  input {
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    height: 44px;
    padding: 0 12px;
    border: 1px solid var(--color-line-subtle);
    border-radius: var(--radius-sm);
    background: var(--color-raised);
    color: var(--color-text);
    font: inherit;
    font-size: 14px;
  }
  input:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 1px;
  }
  .field-hint {
    margin: 0;
    font-size: 12px;
    color: var(--color-muted);
  }
  .kind-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }
  .kind-chip {
    min-height: 40px;
    padding: 8px 10px;
    border: 1px solid var(--color-line-subtle);
    border-radius: var(--radius-sm);
    background: var(--color-raised);
    color: var(--color-text);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .kind-chip.selected {
    border-color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 18%, transparent);
  }
  .path-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
  }
  .roots-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
  }
  .root-chip {
    padding: 4px 10px;
    border: 1px solid var(--color-line-subtle);
    border-radius: var(--r-pill, 999px);
    background: var(--color-raised);
    color: var(--color-text);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .check-row {
    display: flex;
    justify-content: flex-start;
  }
  .actions-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    margin-top: 4px;
  }
  .btn-primary, .btn-secondary, .btn-ghost {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 44px;
    padding: 0 18px;
    border-radius: var(--radius-sm);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    text-decoration: none;
    cursor: pointer;
  }
  .btn-primary {
    border: 0;
    background: var(--color-play);
    color: var(--color-on-play);
  }
  .btn-primary:hover:not(:disabled) {
    background: var(--color-play-hover);
  }
  .btn-secondary {
    border: 1px solid var(--color-line);
    background: transparent;
    color: var(--color-text);
  }
  .btn-ghost {
    border: 0;
    background: transparent;
    color: var(--color-muted);
  }
  button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .error-banner, .success-banner {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 14px;
    border-radius: var(--radius-sm);
    font-size: 13px;
    line-height: 1.45;
  }
  .error-banner {
    background: color-mix(in srgb, var(--status-error, #e05252) 14%, transparent);
    color: var(--status-error, #e05252);
  }
  .success-banner {
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    color: var(--color-text);
  }
  .error-banner p, .success-banner p {
    margin: 0;
  }
  .logo-stop-a { stop-color: var(--color-accent, #ff8a3d); }
  .logo-stop-b { stop-color: var(--color-play, #ff5e2b); }
</style>
