<script lang="ts">
  import { adminRequest } from './request';
  import { serverPresentation } from '../config/presentation';
  import PlaylistPolicy from './PlaylistPolicy.svelte';
  let { section }: { section: string } = $props();
  let name = $state(''), welcome = $state(''), ssoLabel = $state('Se connecter avec SSO'), error = $state(''), notice = $state('');
  let blockSsoLocalAccounts=$state(true);
  let trickplayEnabled = $state(true), trickplayQuality = $state(7), trickplayWidth = $state(320);
  let trickplayIntervalSeconds = $state(10), trickplayAutoGenerate = $state(false), trickplayMaxCacheMb = $state(5120);
  let loading = $state(true), saving = $state(false);
  let network = $state<{ publicUrl: string; secureCookies: boolean; webPath: string; apiPath: string } | null>(null);
  async function load(target: string) {
    loading = true; error = ''; notice = '';
    try {
      if (target === '#general-section') {
        const data = await adminRequest<{
          serverName: string; welcomeMessage: string; ssoButtonLabel: string; blockSsoLocalAccounts: boolean;
          trickplayEnabled?: boolean; trickplayQuality?: number; trickplayWidth?: number;
          trickplayIntervalSeconds?: number; trickplayAutoGenerate?: boolean; trickplayMaxCacheMb?: number;
        }>('/api/v1/admin/settings');
        blockSsoLocalAccounts = data.blockSsoLocalAccounts;
        name = data.serverName;
        welcome = data.welcomeMessage;
        ssoLabel = data.ssoButtonLabel ?? 'Se connecter avec SSO';
        trickplayEnabled = data.trickplayEnabled ?? true;
        trickplayQuality = data.trickplayQuality ?? 7;
        trickplayWidth = data.trickplayWidth ?? 320;
        trickplayIntervalSeconds = data.trickplayIntervalSeconds ?? 10;
        trickplayAutoGenerate = data.trickplayAutoGenerate ?? false;
        trickplayMaxCacheMb = data.trickplayMaxCacheMb ?? 5120;
      }
      else network = await adminRequest('/api/v1/admin/network');
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Chargement impossible.'; }
    finally { loading = false; }
  }
  $effect(() => { if (['#general-section', '#network-section'].includes(section)) void load(section); });
  async function save(event: SubmitEvent) {
    event.preventDefault(); if (saving) return; saving = true; error = ''; notice = '';
    try {
      await adminRequest('/api/v1/admin/settings', {
        method: 'PUT',
        body: JSON.stringify({
          serverName: name, welcomeMessage: welcome, ssoButtonLabel: ssoLabel, blockSsoLocalAccounts,
          trickplayEnabled, trickplayQuality, trickplayWidth, trickplayIntervalSeconds,
          trickplayAutoGenerate, trickplayMaxCacheMb
        })
      });
      const saved = await adminRequest<{
        serverName: string; welcomeMessage: string; ssoButtonLabel: string; blockSsoLocalAccounts: boolean;
        trickplayEnabled?: boolean; trickplayQuality?: number; trickplayWidth?: number;
        trickplayIntervalSeconds?: number; trickplayAutoGenerate?: boolean; trickplayMaxCacheMb?: number;
      }>('/api/v1/admin/settings');
      blockSsoLocalAccounts = saved.blockSsoLocalAccounts;
      name = saved.serverName;
      welcome = saved.welcomeMessage;
      ssoLabel = saved.ssoButtonLabel;
      trickplayEnabled = saved.trickplayEnabled ?? true;
      trickplayQuality = saved.trickplayQuality ?? 7;
      trickplayWidth = saved.trickplayWidth ?? 320;
      trickplayIntervalSeconds = saved.trickplayIntervalSeconds ?? 10;
      trickplayAutoGenerate = saved.trickplayAutoGenerate ?? false;
      trickplayMaxCacheMb = saved.trickplayMaxCacheMb ?? 5120;
      serverPresentation.set(saved);
      notice = 'Paramètres enregistrés.';
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Enregistrement impossible.'; }
    finally { saving = false; }
  }
</script>
{#if section === '#general-section'}
<section id="general-section" aria-labelledby="general-heading"><h2 id="general-heading">Général</h2>
  <form onsubmit={save}><fieldset disabled={loading || saving}>
    <label>Nom du serveur<input bind:value={name} required maxlength="80" autocomplete="off" /></label>
    <label>Message d’accueil<textarea bind:value={welcome} maxlength="500" rows="3"></textarea></label>
    <label>Texte du bouton de connexion SSO<input bind:value={ssoLabel} required maxlength="80" placeholder="Se connecter avec Authentik" /></label>
    <label class="policy-toggle"><input type="checkbox" bind:checked={blockSsoLocalAccounts} />Empêcher les utilisateurs SSO de créer des identifiants locaux</label>
    <fieldset class="trickplay-fieldset">
      <legend>Miniatures Trickplay</legend>
      <label class="policy-toggle"><input type="checkbox" bind:checked={trickplayEnabled} />Activer le système de miniatures Trickplay</label>
      <label class="policy-toggle"><input type="checkbox" bind:checked={trickplayAutoGenerate} />Génération automatique pendant l'analyse des bibliothèques</label>
      <div class="trickplay-grid">
        <label>Qualité JPEG (1–31)<input type="number" min="1" max="31" step="1" required bind:value={trickplayQuality} /></label>
        <label>Largeur vignettes (px)<input type="number" min="160" max="1920" step="1" required bind:value={trickplayWidth} /></label>
        <label>Intervalle (secondes)<input type="number" min="1" max="60" step="1" required bind:value={trickplayIntervalSeconds} /></label>
        <label>Quota cache (Mo)<input type="number" min="128" max="1048576" step="128" required bind:value={trickplayMaxCacheMb} /></label>
      </div>
    </fieldset>
    <button type="submit">{saving ? 'Enregistrement…' : 'Enregistrer'}</button>
  </fieldset></form>
  <PlaylistPolicy />
</section>
{:else if section === '#network-section'}
<section id="network-section" aria-labelledby="network-heading"><h2 id="network-heading">Réseau</h2>
  {#if network}<dl><div><dt>Adresse publique</dt><dd>{network.publicUrl}</dd></div><div><dt>Interface Web</dt><dd>{network.webPath}</dd></div><div><dt>API</dt><dd>{network.apiPath}</dd></div><div><dt>Cookies de session sécurisés (HTTPS)</dt><dd>{network.secureCookies ? 'Activés' : 'Désactivés — HTTP local'}</dd></div></dl>{/if}
  <details><summary>Modifier la connexion</summary><p>L’adresse publique est définie par <code>MEDIA_PUBLIC_URL</code> au démarrage. Les ports et l’accès depuis l’extérieur se règlent dans le déploiement et le proxy. Ces valeurs sont affichées ici en lecture seule.</p>
  <p>Les adresses des Media Nodes se gèrent dans Nœuds de stockage.</p></details>
</section>
{/if}
{#if ['#general-section','#network-section'].includes(section)}
  {#if loading}<p role="status">Chargement…</p>{/if}{#if error}<p role="alert">{error}</p><button disabled={loading} onclick={() => void load(section)}>Réessayer</button>{/if}{#if notice}<p role="status">{notice}</p>{/if}
{/if}
<style>
  .policy-toggle { display:flex;align-items:center;gap:12px; }
  .trickplay-fieldset { border: 1px solid var(--border-subtle); padding: 16px; margin-top: 24px; border-radius: var(--radius-sm); }
  .trickplay-fieldset legend { font-weight: 600; padding: 0 8px; color: var(--text-primary); }
  .trickplay-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 16px; margin-top: 16px; }.policy-toggle input { width:20px;min-height:20px; }
  h2 { font-size: 28px; font-weight: 400; margin: 0 0 32px; }
  form { max-width: 850px; } fieldset { border: 0; padding: 0; margin: 0; min-width: 0; }
  label { display: grid; gap: 12px; margin-top: 24px; font-size: 15px; }
  input, textarea { box-sizing: border-box; width: 100%; padding: 14px; min-height: 48px; font: inherit; background: var(--bg-surface); color: var(--text-primary); border: 0; border-bottom: 2px solid var(--border-subtle); border-radius: 0; }
  textarea { resize: vertical; } p { font-size: 14px; line-height: 1.7; color: var(--text-secondary); }
  button { margin-top: 16px; padding: 12px 24px; min-height: 44px; border: 0; border-radius: var(--radius-md); background: var(--color-play); color: var(--color-on-play); }
  dl { max-width: 850px; } dl div { display: grid; grid-template-columns: minmax(160px, 1fr) minmax(0, 2fr); gap: 24px; padding: 20px; background: var(--bg-surface); border-bottom: 1px solid var(--border-subtle); }
  dd { margin: 0; overflow-wrap: anywhere; } dt { color: var(--text-secondary); }
  @media(max-width: 640px) { dl div { grid-template-columns: minmax(0,1fr); gap: 10px; } }
</style>
