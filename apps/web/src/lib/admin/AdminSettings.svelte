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
  <p class="lead">Nom du serveur, connexion SSO et réglages de lecture.</p>
  <form onsubmit={save}><fieldset disabled={loading || saving}>
    <div class="settings-panel">
    <label>Nom du serveur<input bind:value={name} required maxlength="80" autocomplete="off" /></label>
    <label>Message d’accueil<textarea bind:value={welcome} maxlength="500" rows="3"></textarea></label>
    <label>Texte du bouton de connexion SSO<input bind:value={ssoLabel} required maxlength="80" placeholder="Se connecter avec Authentik" /></label>
    <label class="policy-toggle"><input type="checkbox" bind:checked={blockSsoLocalAccounts} />Empêcher les utilisateurs SSO de créer des identifiants locaux</label>
    </div>

    <details class="adv">
      <summary>Reprise de lecture</summary>
      <fieldset class="trickplay-fieldset">
        <p>Les nouvelles progressions sont enregistrées selon ces seuils pour tous les appareils. Avant le seuil de reprise ou sous la durée minimale, la lecture repart du début. Le seuil « vu » est prioritaire. Les progressions existantes sont conservées lors de l’enregistrement des paramètres.</p>
        <div class="trickplay-grid">
          <label>Reprise dès (% de la durée)<input type="number" min="0" max="49" step="1" required bind:value={minResumePct} /></label>
          <label>Considéré comme vu à (%)<input type="number" min="50" max="100" step="1" required bind:value={maxResumePct} /></label>
          <label>Durée minimale pour reprendre (s)<input type="number" min="0" max="86400" step="1" required bind:value={minResumeDurationSeconds} /></label>
        </div>
      </fieldset>
    </details>
    <details class="adv">
      <summary>Métadonnées</summary>
      <fieldset class="trickplay-fieldset">
        <div class="trickplay-grid">
          <label>Langue préférée (ex. fr, en, pt-BR)<input bind:value={metadataLanguage} required maxlength="6" pattern={'[a-z]{2,3}(-[A-Z]{2})?'} /></label>
          <label>Pays (ex. FR, US)<input bind:value={metadataCountry} required maxlength="2" pattern={'[A-Z]{2}'} /></label>
        </div>
      </fieldset>
    </details>
    <details class="adv">
      <summary>Empreintes audio des intros et génériques</summary>
      <fieldset class="trickplay-fieldset">
        <p>Les empreintes acoustiques Chromaprint sont conservées en cache sur chaque Media Node pour éviter les réextractions répétées lors des analyses.</p>
        <button type="button" class="maintenance-btn" disabled={clearingFingerprints} onclick={clearFingerprintCache}>
          {clearingFingerprints ? 'Nettoyage en cours...' : 'Vider le cache des empreintes audio'}
        </button>
        {#if fingerprintNotice}<p class="maintenance-notice">{fingerprintNotice}</p>{/if}
      </fieldset>
    </details>
    <details class="adv">
      <summary>Journal des extensions et sécurité CORS</summary>
      <fieldset class="trickplay-fieldset">
        <p>La tâche « Nettoyer le journal des extensions » supprime jusqu’à 500 événements anciens par exécution, chaque jour par défaut. Son calendrier et son lancement manuel se gèrent dans Tâches. Ce réglage ne modifie pas la durée des sessions ni l’historique de lecture.</p>
        <div class="trickplay-grid">
          <label>Conservation du journal des extensions (jours)<input type="number" min="1" max="3650" step="1" required bind:value={activityLogRetentionDays} /></label>
          <label>Origines CORS autorisées (séparées par des virgules)<input bind:value={corsHosts} placeholder="https://app.example.com, http://localhost:3000" /></label>
        </div>
      </fieldset>
    </details>
    <div class="actions"><button type="submit">{saving ? 'Enregistrement…' : 'Enregistrer'}</button></div>
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
    <div class="actions"><button type="submit">{saving ? 'Enregistrement…' : 'Enregistrer'}</button></div>
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
  {#if loading}<p role="status">Chargement…</p>{/if}{#if error}<p role="alert">{error}</p><button class="maintenance-btn" disabled={loading} onclick={() => void load(section)}>Réessayer</button>{/if}{#if notice}<p role="status">{notice}</p>{/if}
{/if}
<style>
  section { display: grid; gap: 20px; min-width: 0; max-width: 760px; font-family: var(--font-sans); color: var(--color-text); }
  h2 { margin: 0; font-size: 26px; font-weight: 700; letter-spacing: -0.02em; line-height: 1.2; }
  p { margin: 0; font-size: 13px; line-height: 1.6; color: var(--color-muted); }
  h2 + p { font-size: 14px; margin-top: -12px; }
  form { display: grid; }
  fieldset { display: grid; min-width: 0; margin: 0; padding: 0; border: 0; }
  form > fieldset { gap: 12px; }
  /* Lignes de formulaire sur surface plate */
  .settings-panel { display: grid; padding: 4px 20px; border-radius: var(--radius-md); background: var(--color-surface); }
  .settings-panel > label { padding: 16px 0; border-top: 1px solid var(--color-line-subtle); }
  .settings-panel > label:first-child { border-top: 0; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 600; color: var(--color-text); }
  .policy-toggle { display: flex; align-items: center; gap: 12px; min-height: 44px; cursor: pointer; }
  .policy-toggle input { flex: none; width: 20px; height: 20px; min-height: 20px; margin: 0; accent-color: var(--color-accent); }
  input:not([type='checkbox']), textarea { box-sizing: border-box; width: 100%; min-width: 0; padding: 0 12px; border: 1px solid var(--color-line-subtle); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; font-size: 14px; font-weight: 400; }
  input:not([type='checkbox']) { height: 42px; }
  textarea { min-height: 96px; padding: 10px 12px; line-height: 1.5; resize: vertical; }
  input:not([type='checkbox']):focus-visible, textarea:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
  /* Options avancées repliées */
  .adv { padding: 0 20px; border-radius: var(--radius-md); background: var(--color-surface); }
  .adv > summary { display: flex; align-items: center; min-height: 52px; font-size: 14px; font-weight: 600; cursor: pointer; }
  .adv[open] > summary { border-bottom: 1px solid var(--color-line-subtle); }
  .trickplay-fieldset { gap: 12px; padding: 16px 0; }
  .trickplay-fieldset legend { float: left; width: 100%; margin: 0 0 4px; padding: 0; font-size: 15px; font-weight: 600; color: var(--color-text); }
  .trickplay-fieldset legend + * { clear: both; }
  .trickplay-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px 20px; }
  /* Section Trickplay autonome : surface plate */
  #trickplay-section .trickplay-fieldset { padding: 20px; border-radius: var(--radius-md); background: var(--color-surface); }
  /* Boutons : un seul primaire, le reste en ghost */
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 40px; padding: 8px 20px; border: 0; border-radius: var(--radius-sm); background: var(--color-play); color: var(--color-on-play); font: inherit; font-size: 14px; font-weight: 600; cursor: pointer; transition: background-color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-play-hover); }
  button:disabled { opacity: 0.55; cursor: default; }
  button:focus-visible, summary:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .maintenance-btn { justify-self: start; width: fit-content; background: transparent; color: var(--color-text); border: 1px solid var(--color-line); font-weight: 500; }
  .maintenance-btn:hover:not(:disabled) { background: var(--color-raised); }
  .maintenance-notice { color: var(--color-accent); font-weight: 500; }
  .actions { display: flex; justify-content: flex-end; }
  [role='alert'] { color: var(--status-error); }
  /* Réseau */
  dl { margin: 0; padding: 4px 20px; border-radius: var(--radius-md); background: var(--color-surface); }
  dl div { display: grid; grid-template-columns: minmax(160px, 1fr) minmax(0, 2fr); gap: 24px; padding: 16px 0; border-top: 1px solid var(--color-line-subtle); font-size: 14px; }
  dl div:first-child { border-top: 0; }
  dt { color: var(--color-muted); }
  dd { margin: 0; overflow-wrap: anywhere; }
  details:not(.adv) { padding: 0 20px; border-radius: var(--radius-md); background: var(--color-surface); }
  details:not(.adv) summary { display: flex; align-items: center; min-height: 52px; font-size: 14px; font-weight: 600; cursor: pointer; }
  details:not(.adv)[open] { padding-bottom: 16px; }
  details:not(.adv) p + p { margin-top: 8px; }
  code { font-family: ui-monospace, monospace; font-size: 12px; color: var(--color-text); }
  @media (max-width: 640px) {
    dl div { grid-template-columns: minmax(0, 1fr); gap: 6px; }
    button { width: 100%; }
    input:not([type='checkbox']), textarea { font-size: 16px; }
  }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
</style>
