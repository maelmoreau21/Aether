<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { deviceRequest } from './quick-connect';
  import { pollDeviceConnection } from './session';
  interface Challenge { secret: string; code: string; expiresIn: number; qrWidth: number; qrModules: boolean[]; }
  let challenge = $state<Challenge | null>(null);
  let error = $state('');
  let busy = $state(false);
  let expired = $state(false);
  let controller: AbortController | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let deadline = 0;
  const path = $derived(challenge?.qrModules.flatMap((dark, index) => dark ? [`M${index % challenge!.qrWidth + 4} ${Math.floor(index / challenge!.qrWidth) + 4}h1v1h-1z`] : []).join('') ?? '');
  async function poll(current: Challenge, signal: AbortSignal) {
    if (signal.aborted) return;
    if (Date.now() >= deadline) { expired = true; return; }
    try {
      if (await pollDeviceConnection(current.secret, signal)) { await goto(resolve('/'), { replaceState: true }); return; }
      if (!signal.aborted) timer = setTimeout(() => void poll(current, signal), 2500);
    } catch (cause) {
      if (!signal.aborted) { error = cause instanceof Error ? cause.message : 'Connexion indisponible.'; expired = true; }
    }
  }
  async function start() {
    controller?.abort(); clearTimeout(timer);
    controller = new AbortController();
    const signal = controller.signal;
    busy = true; error = ''; expired = false; challenge = null;
    try {
      const result = await deviceRequest<Challenge>('start', {}, undefined, signal);
      if (signal.aborted) return;
      challenge = result; deadline = Date.now() + result.expiresIn * 1000;
      timer = setTimeout(() => void poll(result, signal), 2500);
    } catch (cause) { if (!signal.aborted) error = cause instanceof Error ? cause.message : 'Connexion indisponible.'; }
    finally { if (!signal.aborted) busy = false; }
  }
  onMount(() => () => { controller?.abort(); clearTimeout(timer); });
</script>

<section class="quick-connect" aria-labelledby="quick-connect-title">
  <h2 id="quick-connect-title">Avec un appareil déjà connecté</h2>
  <p>Scannez le QR code avec votre téléphone, puis confirmez la connexion. Fonctionne aussi avec votre compte SSO.</p>
  {#if challenge && !expired}
    <svg class="qr" viewBox={`0 0 ${challenge.qrWidth + 8} ${challenge.qrWidth + 8}`} role="img" aria-label="QR code de connexion à scanner avec un appareil déjà connecté" shape-rendering="crispEdges"><path class="qr-background" d={`M0 0h${challenge.qrWidth + 8}v${challenge.qrWidth + 8}H0z`} /><path d={path} /></svg>
    <p class="code" aria-label={`Code ${challenge.code}`}>{challenge.code.slice(0, 4)}–{challenge.code.slice(4)}</p>
    <p class="hint">Ou saisissez ce code dans Paramètres → Appareils et sessions sur l’appareil déjà connecté. Valable 5 minutes.</p>
    <p role="status">En attente de votre confirmation…</p>
  {/if}
  {#if expired && !error}<p role="status">Ce QR code a expiré. Générez un nouveau code pour connecter cet appareil.</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  <button type="button" disabled={busy} onclick={() => void start()}>{busy ? 'Création du code…' : challenge ? 'Générer un nouveau code' : 'Afficher le QR code'}</button>
</section>

<style>
  .quick-connect { border-top: 1px solid var(--line); padding-top: 20px; text-align: center; }
  h2 { margin: 0 0 8px; font-size: 16px; }
  p { color: var(--mu); font-size: 13px; line-height: 1.6; }
  .qr { display: block; width: min(100%, 216px); height: auto; margin: 20px auto 12px; fill: #101010; border-radius: 12px; }
  .qr-background { fill: #fff; }
  .code { font: 700 26px var(--font-mono, monospace); letter-spacing: .12em; color: var(--tx); margin: 12px 0; }
  button { min-height: 44px; border: 1px solid var(--line); border-radius: var(--r-md); padding: 10px 16px; font: inherit; font-size: 14px; font-weight: 600; color: var(--tx); background: var(--pn); cursor: pointer; }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  button:disabled { opacity: .5; cursor: wait; }
  [role='alert'] { color: var(--err); }
</style>
