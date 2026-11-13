<script lang="ts">
  import { adminRequest } from './request';
  import { serverPresentation } from '../config/presentation';
  import PlaylistPolicy from './PlaylistPolicy.svelte';
  import { t } from '../i18n';
  let { section }: { section: string } = $props();
  let name = $state(''), welcome = $state(''), ssoLabel = $state('Se connecter avec SSO'), error = $state(''), notice = $state('');
  let blockSsoLocalAccounts=$state(true);
  let trickplayEnabled = $state(true), trickplayAutoGenerate = $state(true);
  let trickplayQuality = $state(7), trickplayWidth = $state(320), trickplayIntervalSeconds = $state(10);
  let trickplayColumns = $state(10), trickplayRows = $state(10), trickplayMaxCacheMb = $state(5120);
  // Server-wide defaults a library in "inherit" mode follows (each library can force on or off).
  let chapterImagesEnabled = $state(true), introDetectionEnabled = $state(true), markerMinConfidence = $state(80);
  let introAnalysisPercent = $state(25), introAnalysisMaxMinutes = $state(10);
  let minResumePct = $state(0), maxResumePct = $state(90), minResumeDurationSeconds = $state(0);
  let activityLogRetentionDays = $state(30), corsHosts = $state('');
  let clusterEnabled = $state(false);
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
      fingerprintNotice = res.message ?? $t('memory_released');
    } catch (cause) {
      fingerprintNotice = cause instanceof Error ? cause.message : $t('operation_failed');
    } finally {
      clearingFingerprints = false;
    }
  }
  async function load(target: string) {
    loading = true; error = ''; notice = '';
    try {
      if (target === '#general-section') {
        const data = await adminRequest<{
          serverName: string; welcomeMessage: string; ssoButtonLabel: string; blockSsoLocalAccounts: boolean;
          trickplayEnabled?: boolean; trickplayAutoGenerate?: boolean;
          trickplayQuality?: number; trickplayWidth?: number; trickplayIntervalSeconds?: number;
          trickplayColumns?: number; trickplayRows?: number; trickplayMaxCacheMb?: number;
          chapterImagesEnabled?: boolean; introDetectionEnabled?: boolean; markerMinConfidence?: number; introAnalysisPercent?: number; introAnalysisMaxMinutes?: number;
          minResumePct?: number; maxResumePct?: number; minResumeDurationSeconds?: number;
          activityLogRetentionDays?: number; corsHosts?: string[];
          clusterEnabled?: boolean;
        }>('/api/v1/admin/settings');
        blockSsoLocalAccounts = data.blockSsoLocalAccounts;
        name = data.serverName;
        welcome = data.welcomeMessage;
        ssoLabel = data.ssoButtonLabel ?? $t('sign_in_with_sso');
        trickplayEnabled = data.trickplayEnabled ?? true;
        trickplayAutoGenerate = data.trickplayAutoGenerate ?? true;
        trickplayQuality = data.trickplayQuality ?? 7;
        trickplayWidth = data.trickplayWidth ?? 320;
        trickplayIntervalSeconds = data.trickplayIntervalSeconds ?? 10;
        trickplayColumns = data.trickplayColumns ?? 10;
        trickplayRows = data.trickplayRows ?? 10;
        trickplayMaxCacheMb = data.trickplayMaxCacheMb ?? 5120;
        chapterImagesEnabled = data.chapterImagesEnabled ?? true;
        introDetectionEnabled = data.introDetectionEnabled ?? true;
        markerMinConfidence = data.markerMinConfidence ?? 80;
        introAnalysisPercent = data.introAnalysisPercent ?? 25;
        introAnalysisMaxMinutes = data.introAnalysisMaxMinutes ?? 10;
        minResumePct = data.minResumePct ?? 0; maxResumePct = data.maxResumePct ?? 90;
        minResumeDurationSeconds = data.minResumeDurationSeconds ?? 0;
        activityLogRetentionDays = data.activityLogRetentionDays ?? 30;
        corsHosts = (data.corsHosts ?? []).join(', ');
        clusterEnabled = data.clusterEnabled === true;
      }
      else network = await adminRequest('/api/v1/admin/network');
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('loading_impossible'); }
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
          trickplayEnabled, trickplayAutoGenerate,
          trickplayQuality, trickplayWidth, trickplayIntervalSeconds,
          trickplayColumns, trickplayRows, trickplayMaxCacheMb,
          chapterImagesEnabled, introDetectionEnabled, markerMinConfidence, introAnalysisPercent, introAnalysisMaxMinutes,
          minResumePct, maxResumePct, minResumeDurationSeconds,
          activityLogRetentionDays,
          clusterEnabled,
          corsHosts: corsHosts.split(/[\n,]+/).map((s: string) => s.trim()).filter(Boolean)
        })
      });
      const saved = await adminRequest<{
        serverName: string; welcomeMessage: string; ssoButtonLabel: string; blockSsoLocalAccounts: boolean;
        trickplayEnabled?: boolean; trickplayAutoGenerate?: boolean;
        trickplayQuality?: number; trickplayWidth?: number; trickplayIntervalSeconds?: number;
        trickplayColumns?: number; trickplayRows?: number; trickplayMaxCacheMb?: number;
          chapterImagesEnabled?: boolean; introDetectionEnabled?: boolean; markerMinConfidence?: number; introAnalysisPercent?: number; introAnalysisMaxMinutes?: number;
        minResumePct?: number; maxResumePct?: number; minResumeDurationSeconds?: number;
        activityLogRetentionDays?: number; corsHosts?: string[];
        clusterEnabled?: boolean;
      }>('/api/v1/admin/settings');
      blockSsoLocalAccounts = saved.blockSsoLocalAccounts;
      name = saved.serverName;
      welcome = saved.welcomeMessage;
      ssoLabel = saved.ssoButtonLabel;
      trickplayEnabled = saved.trickplayEnabled ?? true;
      trickplayAutoGenerate = saved.trickplayAutoGenerate ?? true;
      trickplayQuality = saved.trickplayQuality ?? 7;
      trickplayWidth = saved.trickplayWidth ?? 320;
      trickplayIntervalSeconds = saved.trickplayIntervalSeconds ?? 10;
      trickplayColumns = saved.trickplayColumns ?? 10;
      trickplayRows = saved.trickplayRows ?? 10;
      trickplayMaxCacheMb = saved.trickplayMaxCacheMb ?? 5120;
      chapterImagesEnabled = saved.chapterImagesEnabled ?? true;
      introDetectionEnabled = saved.introDetectionEnabled ?? true;
      markerMinConfidence = saved.markerMinConfidence ?? 80;
      introAnalysisPercent = saved.introAnalysisPercent ?? 25;
      introAnalysisMaxMinutes = saved.introAnalysisMaxMinutes ?? 10;
      minResumePct = saved.minResumePct ?? 0; maxResumePct = saved.maxResumePct ?? 90;
      minResumeDurationSeconds = saved.minResumeDurationSeconds ?? 0;
      activityLogRetentionDays = saved.activityLogRetentionDays ?? 30;
      corsHosts = (saved.corsHosts ?? []).join(', ');
      clusterEnabled = saved.clusterEnabled === true;
      serverPresentation.set(saved);
      notice = $t('settings_saved');
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('operation_failed'); }
    finally { saving = false; }
  }
</script>
{#if section === '#general-section'}
<section id="general-section" aria-labelledby="general-heading"><h2 id="general-heading">{$t('general')}</h2>
  <p class="lead">{$t('general_settings_intro')}</p>
  <form onsubmit={save}><fieldset disabled={loading || saving}>
    <div class="settings-panel">
    <label>{$t('server_name')}<input bind:value={name} required maxlength="80" autocomplete="off" /></label>
    <label>{$t('welcome_message')}<textarea bind:value={welcome} maxlength="500" rows="3"></textarea></label>
    <label>{$t('sso_button_label')}<input bind:value={ssoLabel} required maxlength="80" placeholder={$t('sign_in_with_authentik')} /></label>
    <label class="policy-toggle"><input type="checkbox" bind:checked={blockSsoLocalAccounts} />{$t('block_sso_local')}</label>
    <p class="hint">{$t('playback_settings_moved')}</p>
    </div>

    <details class="adv">
      <summary>{$t('previews_in_the_playback_bar')}</summary>
      <fieldset class="settings-fieldset">
        <p>{$t('trickplay_server_note')}</p>
        <label class="policy-toggle"><input type="checkbox" bind:checked={trickplayEnabled} />{$t('previews_in_the_playback_bar')}</label>
        {#if trickplayEnabled}
          <label class="policy-toggle"><input type="checkbox" bind:checked={trickplayAutoGenerate} />{$t('prepare_thumbnails_after_media_analysis')}</label>
          <div class="settings-grid">
            <label>{$t('interval_s')}<input type="number" min="1" max="60" step="1" required bind:value={trickplayIntervalSeconds} /></label>
            <label>{$t('width_px')}<input type="number" min="160" max="1920" step="1" required bind:value={trickplayWidth} /></label>
            <label>{$t('jpeg_quality')}<input type="number" min="1" max="31" step="1" required bind:value={trickplayQuality} /></label>
            <label>{$t('trickplay_columns')}<input type="number" min="1" max="20" step="1" required bind:value={trickplayColumns} /></label>
            <label>{$t('trickplay_rows')}<input type="number" min="1" max="20" step="1" required bind:value={trickplayRows} /></label>
            <label>{$t('trickplay_max_cache_mb')}<input type="number" min="128" max="1048576" step="1" required bind:value={trickplayMaxCacheMb} /></label>
          </div>
        {/if}
      </fieldset>
    </details>
    <details class="adv">
      <summary>{$t('server_media_defaults')}</summary>
      <fieldset class="settings-fieldset">
        <p>{$t('server_media_defaults_note')}</p>
        <label class="policy-toggle"><input type="checkbox" bind:checked={chapterImagesEnabled} />{$t('show_chapter_images')}</label>
        <label class="policy-toggle"><input type="checkbox" bind:checked={introDetectionEnabled} />{$t('detect_intros_with_enabled_extensions')}</label>
        {#if introDetectionEnabled}
          <label>{$t('marker_min_confidence', { value: markerMinConfidence })}<input type="range" min="50" max="99" step="1" bind:value={markerMinConfidence} /><small>{$t('marker_min_confidence_hint')}</small></label>
          <label>{$t('intro_analysis_percent', { value: introAnalysisPercent })}<input type="range" min="1" max="50" step="1" bind:value={introAnalysisPercent} /><small>{$t('intro_analysis_percent_hint')}</small></label>
          <label>{$t('intro_analysis_max_minutes')}<input type="number" min="1" max="10" step="1" required bind:value={introAnalysisMaxMinutes} /><small>{$t('intro_analysis_max_minutes_hint')}</small></label>
          <p class="hint">{$t('server_intro_thresholds_note')}</p>
        {/if}
      </fieldset>
    </details>
    <details class="adv">
      <summary>{$t('playback_resume')}</summary>
      <fieldset class="settings-fieldset">
        <p>{$t('playback_resume_note')}</p>
        <div class="settings-grid">
          <label>{$t('resume_at')}<input type="number" min="0" max="49" step="1" required bind:value={minResumePct} /></label>
          <label>{$t('watched_at')}<input type="number" min="50" max="100" step="1" required bind:value={maxResumePct} /></label>
          <label>{$t('minimum_resume_duration')}<input type="number" min="0" max="86400" step="1" required bind:value={minResumeDurationSeconds} /></label>
        </div>
      </fieldset>
    </details>
    <details class="adv">
      <summary>{$t('audio_fingerprints')}</summary>
      <fieldset class="settings-fieldset">
        <p>{$t('audio_fingerprints_note')}</p>
        <button type="button" class="maintenance-btn" disabled={clearingFingerprints} onclick={clearFingerprintCache}>
          {clearingFingerprints ? $t('clearing') : $t('clear_fingerprint_cache')}
        </button>
        {#if fingerprintNotice}<p class="maintenance-notice">{fingerprintNotice}</p>{/if}
      </fieldset>
    </details>
    <details class="adv">
      <summary>{$t('extension_log_cors')}</summary>
      <fieldset class="settings-fieldset">
        <p>{$t('extension_log_note')}</p>
        <div class="settings-grid">
          <label>{$t('log_retention')}<input type="number" min="1" max="3650" step="1" required bind:value={activityLogRetentionDays} /></label>
          <label>{$t('allowed_cors')}<input bind:value={corsHosts} placeholder={$t('https_app_example_com_http_localhost')} /></label>
        </div>
      </fieldset>
    </details>
    <details class="adv">
      <summary>{$t('advanced_capabilities_heading')}</summary>
      <fieldset class="settings-fieldset">
        <p>{$t('advanced_capabilities_note')}</p>
        <label class="policy-toggle"><input type="checkbox" bind:checked={clusterEnabled} />{$t('enable_cluster_nodes')}</label>
      </fieldset>
    </details>
    <div class="actions"><button type="submit">{saving ? $t('loading') : $t('save')}</button></div>
  </fieldset></form>
  <PlaylistPolicy />
</section>
{:else if section === '#network-section'}
<section id="network-section" aria-labelledby="network-heading"><h2 id="network-heading">{$t('network')}</h2>
  {#if network}<dl><div><dt>{$t('public_address')}</dt><dd>{network.publicUrl}</dd></div><div><dt>{$t('web_interface')}</dt><dd>{network.webPath}</dd></div><div><dt>API</dt><dd>{network.apiPath}</dd></div><div><dt>{$t('secure_session_cookies')}</dt><dd>{network.secureCookies ? $t('enabled') : $t('disabled_local_http')}</dd></div></dl>{/if}
</section>
{/if}
{#if ['#general-section','#network-section'].includes(section)}
  {#if loading}<p role="status">{$t('loading_settings_short')}</p>{/if}{#if error}<p role="alert">{error}</p><button class="maintenance-btn" disabled={loading} onclick={() => void load(section)}>{$t('retry_action')}</button>{/if}{#if notice}<p role="status">{notice}</p>{/if}
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
  .settings-fieldset { gap: 12px; padding: 16px 0; }
  .settings-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px 20px; }
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
  :global(code) { font-family: ui-monospace, monospace; font-size: 12px; color: var(--color-text); }
  @media (max-width: 640px) {
    dl div { grid-template-columns: minmax(0, 1fr); gap: 6px; }
    button { width: 100%; }
    input:not([type='checkbox']), textarea { font-size: 16px; }
  }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
</style>
