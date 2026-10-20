<script lang="ts">
  import { adminRequest } from './request';
  import { serverPresentation } from '../config/presentation';
  let { section }: { section: string } = $props();
  let name = $state(''), welcome = $state(''), ssoLabel = $state('Se connecter avec SSO'), error = $state(''), notice = $state('');
  let loading = $state(true), saving = $state(false);
  let network = $state<{ publicUrl: string; secureCookies: boolean; webPath: string; apiPath: string } | null>(null);
  async function load(target: string) {
    loading = true; error = ''; notice = '';
    try {
      if (target === '#general-section') { const data = await adminRequest<{ serverName: string; welcomeMessage: string; ssoButtonLabel: string }>('/api/v1/admin/settings'); name = data.serverName; welcome = data.welcomeMessage; ssoLabel = data.ssoButtonLabel ?? 'Se connecter avec SSO'; }
      else network = await adminRequest('/api/v1/admin/network');
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Chargement impossible.'; }
    finally { loading = false; }
  }
  $effect(() => { if (['#general-section', '#network-section'].includes(section)) void load(section); });
  async function save(event: SubmitEvent) {
    event.preventDefault(); if (saving) return; saving = true; error = ''; notice = '';
    try {
      await adminRequest('/api/v1/admin/settings', { method: 'PUT', body: JSON.stringify({ serverName: name, welcomeMessage: welcome, ssoButtonLabel: ssoLabel }) });
      const saved = await adminRequest<{ serverName: string; welcomeMessage: string; ssoButtonLabel: string }>('/api/v1/admin/settings');
      name = saved.serverName; welcome = saved.welcomeMessage; ssoLabel = saved.ssoButtonLabel; serverPresentation.set(saved); notice = 'Paramètres enregistrés.';
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Enregistrement impossible.'; }
    finally { saving = false; }
  }
</script>
{#if section === '#general-section'}
<section id="general-section" aria-labelledby="general-heading"><h2 id="general-heading">Général</h2>
  <form onsubmit={save}><fieldset disabled={loading || saving}>
    <label>Nom du serveur<input bind:value={name} required maxlength="80" autocomplete="off" /></label><p>Affiché dans la navigation de cette installation.</p>
    <label>Message d’accueil<textarea bind:value={welcome} maxlength="500" rows="3"></textarea></label><p>Texte affiché sur l’accueil des utilisateurs connectés.</p>
    <label>Texte du bouton de connexion SSO<input bind:value={ssoLabel} required maxlength="80" placeholder="Se connecter avec Authentik" /></label><p>Libellé affiché sur la page de connexion. Enregistré immédiatement, sans redémarrage du Core.</p>
    <button type="submit">{saving ? 'Enregistrement…' : 'Enregistrer'}</button>
  </fieldset></form>
</section>
{:else if section === '#network-section'}
<section id="network-section" aria-labelledby="network-heading"><h2 id="network-heading">Réseau</h2><p>Configuration effective du Core.</p>
  {#if network}<dl><div><dt>Adresse publique</dt><dd>{network.publicUrl}</dd></div><div><dt>Interface Web</dt><dd>{network.webPath}</dd></div><div><dt>API</dt><dd>{network.apiPath}</dd></div><div><dt>Cookies de session sécurisés (HTTPS)</dt><dd>{network.secureCookies ? 'Activés' : 'Désactivés — HTTP local'}</dd></div></dl>{/if}
  <h3>Modifier la connexion</h3><p>L’adresse publique est définie par <code>MEDIA_PUBLIC_URL</code> au démarrage. Les ports et l’accès depuis l’extérieur se règlent dans le déploiement et le proxy. Ces valeurs sont affichées ici en lecture seule.</p>
  <p>Les adresses des Media Nodes se gèrent dans Nœuds de stockage.</p>
</section>
{/if}
{#if ['#general-section','#network-section'].includes(section)}
  {#if loading}<p role="status">Chargement…</p>{/if}{#if error}<p role="alert">{error}</p><button disabled={loading} onclick={() => void load(section)}>Réessayer</button>{/if}{#if notice}<p role="status">{notice}</p>{/if}
{/if}
<style>
  h2 { font-size: 28px; font-weight: 400; margin: 0 0 32px; } h3 { margin-top: 32px; font-weight: 500; }
  form { max-width: 850px; } fieldset { border: 0; padding: 0; margin: 0; min-width: 0; }
  label { display: grid; gap: 12px; margin-top: 24px; font-size: 15px; }
  input, textarea { box-sizing: border-box; width: 100%; padding: 14px; min-height: 48px; font: inherit; background: var(--bg-surface); color: var(--text-primary); border: 0; border-bottom: 2px solid var(--border-subtle); border-radius: 0; }
  textarea { resize: vertical; } p { font-size: 14px; line-height: 1.7; color: var(--text-secondary); }
  button { margin-top: 16px; padding: 12px 24px; min-height: 44px; border: 0; border-radius: 4px; background: var(--brand-gradient); color: var(--color-on-accent); }
  dl { max-width: 850px; } dl div { display: grid; grid-template-columns: minmax(160px, 1fr) minmax(0, 2fr); gap: 24px; padding: 20px; background: var(--bg-surface); border-bottom: 1px solid var(--border-subtle); }
  dd { margin: 0; overflow-wrap: anywhere; } dt { color: var(--text-secondary); }
  @media(max-width: 640px) { dl div { grid-template-columns: minmax(0,1fr); gap: 10px; } }
</style>
