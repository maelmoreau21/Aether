<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { getSessionGeneration, session } from './session';
  import { deviceRequest, normalizeDeviceCode, PENDING_DEVICE_CODE } from './quick-connect';
  let { initialCode = '' }: { initialCode?: string } = $props();
  let code = $state(untrack(() => initialCode));
  let device = $state('');
  let checkedCode = $state('');
  let busy = $state(false);
  let error = $state('');
  let success = $state(false);
  let controller: AbortController | null = null;
  onDestroy(() => controller?.abort());
  $effect(() => {
    void $session;
    controller?.abort(); device = ''; checkedCode = ''; busy = false; error = ''; success = false;
  });
  async function inspect(event: SubmitEvent) {
    event.preventDefault();
    const normalized = normalizeDeviceCode(code);
    if (!normalized) { error = 'Saisissez les 8 caractères du code affiché sur le nouvel appareil.'; return; }
    if (!$session || busy) return;
    controller?.abort(); controller = new AbortController();
    const signal = controller.signal, generation = getSessionGeneration();
    busy = true; error = ''; success = false; device = '';
    try {
      const result = await deviceRequest<{ device: string }>('inspect', { code: normalized }, $session.csrfToken, signal);
      if (signal.aborted || generation !== getSessionGeneration()) return;
      device = result.device; checkedCode = normalized;
    } catch (cause) { if (!signal.aborted && generation === getSessionGeneration()) error = cause instanceof Error ? cause.message : 'Code indisponible.'; }
    finally { if (!signal.aborted && generation === getSessionGeneration()) busy = false; }
  }
  async function approve() {
    if (!$session || busy || !device) return;
    controller?.abort(); controller = new AbortController();
    const signal = controller.signal, generation = getSessionGeneration();
    busy = true; error = '';
    try {
      await deviceRequest('approve', { code: checkedCode }, $session.csrfToken, signal);
      if (signal.aborted || generation !== getSessionGeneration()) return;
      success = true; device = ''; code = ''; sessionStorage.removeItem(PENDING_DEVICE_CODE);
    } catch (cause) { if (!signal.aborted && generation === getSessionGeneration()) error = cause instanceof Error ? cause.message : 'Autorisation impossible.'; }
    finally { if (!signal.aborted && generation === getSessionGeneration()) busy = false; }
  }
</script>

<section class="approval" aria-labelledby="device-approval-title">
  <h3 id="device-approval-title">Connecter un appareil</h3>
  <p>Sur le nouvel appareil, ouvrez la page de connexion et choisissez « Afficher le QR code ». Scannez-le avec cet appareil ou saisissez son code ici.</p>
  <form onsubmit={inspect}>
    <label for="device-connection-code">Code du nouvel appareil</label>
    <div class="code-field"><input id="device-connection-code" bind:value={code} oninput={() => { device = ''; success = false; }} autocomplete="off" autocapitalize="characters" spellcheck="false" maxlength="12" placeholder="ABCD–1234" required disabled={busy} /><button disabled={busy || !code.trim()}>{busy ? 'Vérification…' : 'Vérifier le code'}</button></div>
  </form>
  {#if device}
    <div class="confirmation">
      <strong>Autoriser cet appareil à accéder à votre compte ?</strong>
      <p class="device-name">{device}</p>
      <p>Code {checkedCode.slice(0, 4)}–{checkedCode.slice(4)} · Compte {$session?.user.displayName || $session?.user.login}</p>
      <p>Confirmez uniquement si ce code est affiché sur un appareil que vous souhaitez connecter.</p>
      <div class="actions"><button disabled={busy} onclick={() => void approve()}>Autoriser la connexion</button><button class="secondary" disabled={busy} onclick={() => device = ''}>Annuler</button></div>
    </div>
  {/if}
  {#if success}<p class="success" role="status">Connexion autorisée. Le nouvel appareil va ouvrir votre médiathèque.</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>

<style>
  .approval { padding: 22px; margin-bottom: 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-3); }
  h3 { margin: 0 0 8px; font-size: 18px; }
  p { color: var(--mu); line-height: 1.6; font-size: 14px; }
  label { display: block; margin-bottom: 8px; font-size: 13px; font-weight: 600; }
  .code-field, .actions { display: flex; gap: 10px; flex-wrap: wrap; }
  input { width: 180px; max-width: 100%; min-height: 44px; box-sizing: border-box; font: 600 18px var(--font-mono, monospace); letter-spacing: .08em; }
  button { min-height: 44px; padding: 10px 16px; border: 0; border-radius: var(--r-sm); background: var(--btn); color: var(--btn-tx); font: inherit; font-size: 14px; font-weight: 600; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .secondary { background: var(--pn); color: var(--tx); border: 1px solid var(--line); }
  .confirmation { margin-top: 20px; padding-top: 20px; border-top: 1px solid var(--line); }
  .device-name { color: var(--tx); overflow-wrap: anywhere; }
  .error { color: var(--err); }
  .success { color: var(--color-success, var(--tx)); }
  @media (max-width: 480px) { .approval { padding: 16px; } .code-field { flex-direction: column; } input { width: 100%; } }
</style>
