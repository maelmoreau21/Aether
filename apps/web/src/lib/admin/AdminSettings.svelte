<script lang="ts">
  import { adminRequest } from './request';
  import { serverPresentation } from '../config/presentation';
  import PlaylistPolicy from './PlaylistPolicy.svelte';
  let { section }: { section: string } = $props();
  let name = $state(''), welcome = $state(''), ssoLabel = $state('Se connecter avec SSO'), error = $state(''), notice = $state('');
  let blockSsoLocalAccounts=$state(true);
  let trickplayEnabled = $state(true), trickplayQuality = $state(7), trickplayWidth = $state(320);
  let trickplayIntervalSeconds = $state(10), trickplayAutoGenerate = $state(false), trickplayMaxCacheMb = $state(5120);
  let minResumePct = $state(0), maxResumePct = $state(90), minResumeDurationSeconds = $state(0);
  let metadataLanguage = $state('fr'), metadataCountry = $state('FR');
  let activityLogRetentionDays = $state(30), corsHosts = $state('');
  let loading = $state(true), saving = $state(false);
  let network = $state<{ publicUrl: string; secureCookies: boolean; webPath: string; apiPath: string } | null>(null);
  let clearingFingerprints = $state(false);
  let fingerprintNotice = $state('');

  async function clearFingerprintCache() {
    if (clearingFingerprints) return;
    clearingFingerprints = true;
    fingerprintNotice = '';
    try {
      const res = await adminRequest<{ cleared?: boolean; message?: string }>('/api/v1/admin/maintenance/clear-fingerprint-cache', { method: 'POST' });
      fingerprintNotice = res.message ?? 'Cache des empreintes audio vidé avec succès.';
    } catch (cause) {
      fingerprintNotice = cause instanceof Error ? cause.message : 'Échec du vidage du cache.';
    } finally {
      clearingFingerprints = false;
    }
  }
  async function load(target: string) {
    loading = true; error = ''; notice = '';
    try {
      if (['#general-section', '#trickplay-section'].includes(target)) {
        const data = await adminRequest<{
          serverName: string; welcomeMessage: string; ssoButtonLabel: string; blockSsoLocalAccounts: boolean;
          trickplayEnabled?: boolean; trickplayQuality?: number; trickplayWidth?: number;
          trickplayIntervalSeconds?: number; trickplayAutoGenerate?: boolean; trickplayMaxCacheMb?: number; minResumePct?: number; maxResumePct?: number; minResumeDurationSeconds?: number; preferredMetadataLanguage?: string; metadataCountryCode?: string;
          activityLogRetentionDays?: number; corsHosts?: string[];
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
        minResumePct = data.minResumePct ?? 0; maxResumePct = data.maxResumePct ?? 90;
        minResumeDurationSeconds = data.minResumeDurationSeconds ?? 0;
        metadataLanguage = data.preferredMetadataLanguage ?? 'fr'; metadataCountry = data.metadataCountryCode ?? 'FR';
        activityLogRetentionDays = data.activityLogRetentionDays ?? 30;
        corsHosts = (data.corsHosts ?? []).join(', ');
      }
      else network = await adminRequest('/api/v1/admin/network');
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Chargement impossible.'; }
    finally { loading = false; }
  }
  $effect(() => { if (['#general-section', '#trickplay-section', '#network-section'].includes(section)) void load(section); });
  async function save(event: SubmitEvent) {
    event.preventDefault(); if (saving) return; saving = true; error = ''; notice = '';
    try {
      await adminRequest('/api/v1/admin/settings', {
        method: 'PUT',
        body: JSON.stringify(section === '#trickplay-section' ? {
          serverName: name, welcomeMessage: welcome,
          trickplayEnabled, trickplayQuality, trickplayWidth, trickplayIntervalSeconds,
          trickplayAutoGenerate, trickplayMaxCacheMb
        } : {
          serverName: name, welcomeMessage: welcome, ssoButtonLabel: ssoLabel, blockSsoLocalAccounts,
          minResumePct, maxResumePct, minResumeDurationSeconds,
          preferredMetadataLanguage: metadataLanguage, metadataCountryCode: metadataCountry,
          activityLogRetentionDays,
          corsHosts: corsHosts.split(/[\n,]+/).map((s: string) => s.trim()).filter(Boolean)
        })
      });
      const saved = await adminRequest<{
        serverName: string; welcomeMessage: string; ssoButtonLabel: string; blockSsoLocalAccounts: boolean;
        trickplayEnabled?: boolean; trickplayQuality?: number; trickplayWidth?: number;
        trickplayIntervalSeconds?: number; trickplayAutoGenerate?: boolean; trickplayMaxCacheMb?: number; minResumePct?: number; maxResumePct?: number; minResumeDurationSeconds?: number; preferredMetadataLanguage?: string; metadataCountryCode?: string;
        activityLogRetentionDays?: number; corsHosts?: string[];
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
      minResumePct = saved.minResumePct ?? 0; maxResumePct = saved.maxResumePct ?? 90;
      minResumeDurationSeconds = saved.minResumeDurationSeconds ?? 0;
      metadataLanguage = saved.preferredMetadataLanguage ?? 'fr'; metadataCountry = saved.metadataCountryCode ?? 'FR';
      activityLogRetentionDays = saved.activityLogRetentionDays ?? 30;
      corsHosts = (saved.corsHosts ?? []).join(', ');
      serverPresentation.set(saved);
      notice = 'Paramètres enregistrés.';
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Enregistrement impossible.'; }
    finally { saving = false; }
  }
</script>
{#if section === '#general-section'}
<section id="general-section" aria-labelledby="general-heading"><h2 id="general-heading">Général</h2>
  <form onsubmit={save}><fieldset disabled={loading || saving}>
    <div class="settings-panel">
    <label>Nom du serveur<input bind:value={name} required maxlength="80" autocomplete="off" /></label>
    <label>Message d’accueil<textarea bind:value={welcome} maxlength="500" rows="3"></textarea></label>
    <label>Texte du bouton de connexion SSO<input bind:value={ssoLabel} required maxlength="80" placeholder="Se connecter avec Authentik" /></label>
    <label class="policy-toggle"><input type="checkbox" bind:checked={blockSsoLocalAccounts} />Empêcher les utilisateurs SSO de créer des identifiants locaux</label>
    </div>

    <fieldset class="trickplay-fieldset">
      <legend>Reprise de lecture</legend>
      <p>Les nouvelles progressions sont enregistrées selon ces seuils pour tous les appareils. Avant le seuil de reprise ou sous la durée minimale, la lecture repart du début. Le seuil « vu » est prioritaire. Les progressions existantes sont conservées lors de l’enregistrement des paramètres.</p>
      <div class="trickplay-grid">
        <label>Reprise dès (% de la durée)<input type="number" min="0" max="49" step="1" required bind:value={minResumePct} /></label>
        <label>Considéré comme vu à (%)<input type="number" min="50" max="100" step="1" required bind:value={maxResumePct} /></label>
        <label>Durée minimale pour reprendre (s)<input type="number" min="0" max="86400" step="1" required bind:value={minResumeDurationSeconds} /></label>
      </div>
    </fieldset>
    <fieldset class="trickplay-fieldset">
      <legend>Métadonnées</legend>
      <div class="trickplay-grid">
        <label>Langue préférée (ex. fr, en, pt-BR)<input bind:value={metadataLanguage} required maxlength="6" pattern={'[a-z]{2,3}(-[A-Z]{2})?'} /></label>
        <label>Pays (ex. FR, US)<input bind:value={metadataCountry} required maxlength="2" pattern={'[A-Z]{2}'} /></label>
      </div>
    </fieldset>
    <fieldset class="trickplay-fieldset">
      <legend>Empreintes audio des intros et génériques</legend>
      <p>Les empreintes acoustiques Chromaprint sont conservées en cache sur chaque Media Node pour éviter les réextractions répétées lors des analyses.</p>
      <button type="button" class="maintenance-btn" disabled={clearingFingerprints} onclick={clearFingerprintCache}>
        {clearingFingerprints ? 'Nettoyage en cours...' : 'Vider le cache des empreintes audio'}
      </button>
      {#if fingerprintNotice}<p class="maintenance-notice">{fingerprintNotice}</p>{/if}
    </fieldset>
    <fieldset class="trickplay-fieldset">
      <legend>Journal des extensions et sécurité CORS</legend>
      <p>La tâche « Nettoyer le journal des extensions » supprime jusqu’à 500 événements anciens par exécution, chaque jour par défaut. Son calendrier et son lancement manuel se gèrent dans Tâches. Ce réglage ne modifie pas la durée des sessions ni l’historique de lecture.</p>
      <div class="trickplay-grid">
        <label>Conservation du journal des extensions (jours)<input type="number" min="1" max="3650" step="1" required bind:value={activityLogRetentionDays} /></label>
        <label>Origines CORS autorisées (séparées par des virgules)<input bind:value={corsHosts} placeholder="https://app.example.com, http://localhost:3000" /></label>
      </div>
    </fieldset>
    <button type="submit">{saving ? 'Enregistrement…' : 'Enregistrer'}</button>
  </fieldset></form>
  <PlaylistPolicy />
</section>
{:else if section === '#trickplay-section'}
<section id="trickplay-section" aria-labelledby="trickplay-heading">
  <h2 id="trickplay-heading">Miniatures Trickplay</h2>
  <p>Réglez les aperçus affichés lorsque vous parcourez la barre de lecture. Les miniatures sont générées sur le Media Node propriétaire du média.</p>
  <form onsubmit={save}><fieldset disabled={loading || saving}>
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
</section>
{:else if section === '#network-section'}
<section id="network-section" aria-labelledby="network-heading"><h2 id="network-heading">Réseau</h2>
  {#if network}<dl><div><dt>Adresse publique</dt><dd>{network.publicUrl}</dd></div><div><dt>Interface Web</dt><dd>{network.webPath}</dd></div><div><dt>API</dt><dd>{network.apiPath}</dd></div><div><dt>Cookies de session sécurisés (HTTPS)</dt><dd>{network.secureCookies ? 'Activés' : 'Désactivés — HTTP local'}</dd></div></dl>{/if}
  <details><summary>Modifier la connexion</summary><p>L’adresse publique est définie par <code>MEDIA_PUBLIC_URL</code> au démarrage. Les ports et l’accès depuis l’extérieur se règlent dans le déploiement et le proxy. Ces valeurs sont affichées ici en lecture seule.</p>
  <p>Les adresses des Media Nodes se gèrent dans Nœuds de stockage.</p></details>
</section>
{/if}
{#if ['#general-section','#trickplay-section','#network-section'].includes(section)}
  {#if loading}<p role="status">Chargement…</p>{/if}{#if error}<p role="alert">{error}</p><button class="retry-btn" disabled={loading} onclick={() => void load(section)}>Réessayer</button>{/if}{#if notice}<p role="status">{notice}</p>{/if}
{/if}
<style>
  section { min-width: 0; font-family: var(--font); color: var(--tx); }
  h2 { margin: 0 0 20px; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.15; }
  h2 + p { margin: -12px 0 20px; }
  p { font-size: 14px; line-height: 1.6; color: var(--mu); }
  form { max-width: 880px; }
  fieldset { min-width: 0; margin: 0; padding: 0; border: 0; }
  /* Panneaux de réglages */
  .settings-panel, .trickplay-fieldset { display: grid; gap: 16px; margin: 0 0 20px; padding: 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  .trickplay-fieldset legend { float: left; width: 100%; margin: 0 0 4px; padding: 0; font-size: 17px; font-weight: 700; color: var(--tx); }
  .trickplay-fieldset legend + * { clear: both; }
  .trickplay-fieldset p { margin: 0; }
  .trickplay-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px 20px; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 700; color: var(--tx); }
  .policy-toggle { display: flex; align-items: center; gap: 12px; min-height: 44px; font-weight: 600; cursor: pointer; }
  .policy-toggle input { flex: none; width: 20px; height: 20px; min-height: 20px; margin: 0; accent-color: var(--ac); }
  input:not([type='checkbox']), textarea { box-sizing: border-box; width: 100%; min-width: 0; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background-color: var(--pn); color: var(--tx); font: inherit; font-size: 15px; font-weight: 500; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  input:not([type='checkbox']) { height: 46px; min-height: 46px; }
  textarea { min-height: 96px; padding: 12px 14px; line-height: 1.5; resize: vertical; }
  input:not([type='checkbox']):hover, textarea:hover { background-color: var(--pn-2); }
  input:not([type='checkbox']):focus, textarea:focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  /* Boutons : principal = encre inversée, maintenance = secondaire */
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 48px; padding: 0 22px; border: 0; border-radius: var(--r-md); background: var(--btn); color: var(--btn-tx); font: inherit; font-size: 15px; font-weight: 700; box-shadow: none; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { opacity: 0.9; }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  button:focus-visible, summary:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .maintenance-btn, .retry-btn { justify-self: start; min-height: 44px; padding: 0 18px; background: var(--pn-2); color: var(--tx); font-size: 14px; }
  .maintenance-btn:hover:not(:disabled), .retry-btn:hover:not(:disabled) { background: var(--pn-3); opacity: 1; }
  .maintenance-btn:disabled { cursor: wait; }
  .maintenance-notice { font-size: 14px; font-weight: 600; color: var(--tx); }
  [role='alert'] { color: var(--err); }
  /* Réseau */
  dl { max-width: 880px; margin: 0 0 20px; padding: 4px 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  dl div { display: grid; grid-template-columns: minmax(160px, 1fr) minmax(0, 2fr); gap: 24px; padding: 16px 0; border-bottom: 1px solid var(--line); }
  dl div:last-child { border-bottom: 0; }
  dt { font-size: 14px; font-weight: 600; color: var(--mu); }
  dd { margin: 0; font-family: var(--font-mono); font-size: 14px; overflow-wrap: anywhere; }
  dl div:last-child dd { font-family: var(--font); font-weight: 600; }
  details { max-width: 880px; padding: 4px 24px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg-2); }
  details[open] { padding-bottom: 12px; }
  summary { display: flex; align-items: center; min-height: 52px; font-size: 15px; font-weight: 700; cursor: pointer; border-radius: var(--r-xs); }
  code { font-family: var(--font-mono); font-size: 13px; padding: 2px 6px; border-radius: 6px; background: var(--pn-2); color: var(--tx); }
  @media (max-width: 640px) {
    .settings-panel, .trickplay-fieldset { padding: 16px; border-radius: var(--r-md); }
    dl, details { padding-inline: 16px; border-radius: var(--r-md); }
    dl div { grid-template-columns: minmax(0, 1fr); gap: 6px; }
    button, .maintenance-btn, .retry-btn { width: 100%; justify-self: stretch; }
    input:not([type='checkbox']), textarea { font-size: 16px; }
  }
  @media (prefers-reduced-motion: reduce) { button, input, textarea { transition: none; } }
</style>
