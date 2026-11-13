<script lang="ts">
  import { adminRequest } from './request';
  import { serverPresentation } from '../config/presentation';
  import { t } from '../i18n';

  /** Playback, transcoding and device switches; every one is enforced by the Core. */
  interface PlaybackSettings {
    serverName: string; welcomeMessage: string;
    videoTranscodingEnabled?: boolean; audioTranscodingEnabled?: boolean;
    toneMappingEnabled?: boolean; subtitleBurnInEnabled?: boolean; maxTranscodeKbps?: number;
    remoteControlEnabled?: boolean; downloadsEnabled?: boolean; watchTogetherEnabled?: boolean; clusterEnabled?: boolean;
  }
  let video = $state(false), audio = $state(false), toneMapping = $state(true), burnIn = $state(true);
  let maxMbps = $state(0), remote = $state(true), downloads = $state(true), watchTogether = $state(false);
  let base = $state<Pick<PlaybackSettings, 'serverName' | 'welcomeMessage'>>({ serverName: '', welcomeMessage: '' });
  let loading = $state(true), saving = $state(false), error = $state(''), notice = $state('');

  function apply(data: PlaybackSettings) {
    base = { serverName: data.serverName, welcomeMessage: data.welcomeMessage };
    video = data.videoTranscodingEnabled ?? false;
    audio = data.audioTranscodingEnabled ?? false;
    toneMapping = data.toneMappingEnabled ?? true;
    burnIn = data.subtitleBurnInEnabled ?? true;
    maxMbps = (data.maxTranscodeKbps ?? 0) / 1000;
    remote = data.remoteControlEnabled ?? true;
    downloads = data.downloadsEnabled ?? true;
    watchTogether = data.watchTogetherEnabled ?? false;
  }
  async function load() {
    loading = true; error = ''; notice = '';
    try { apply(await adminRequest<PlaybackSettings>('/api/v1/admin/settings')); }
    catch (cause) { error = cause instanceof Error ? cause.message : $t('operation_failed'); }
    finally { loading = false; }
  }
  $effect(() => { void load(); });

  async function save(event: SubmitEvent) {
    event.preventDefault(); if (saving) return; saving = true; error = ''; notice = '';
    try {
      await adminRequest('/api/v1/admin/settings', {
        method: 'PUT',
        body: JSON.stringify({
          ...base,
          videoTranscodingEnabled: video, audioTranscodingEnabled: audio,
          toneMappingEnabled: toneMapping, subtitleBurnInEnabled: burnIn,
          maxTranscodeKbps: Math.round(Math.max(0, maxMbps) * 1000),
          remoteControlEnabled: remote, downloadsEnabled: downloads, watchTogetherEnabled: watchTogether
        })
      });
      const saved = await adminRequest<PlaybackSettings>('/api/v1/admin/settings');
      apply(saved);
      serverPresentation.set(saved);
      notice = $t('settings_saved');
    } catch (cause) { error = cause instanceof Error ? cause.message : $t('operation_failed'); }
    finally { saving = false; }
  }
</script>

<section id="playback-section" aria-labelledby="playback-heading">
  <h2 id="playback-heading">{$t('admin_playback')}</h2>
  <p class="lead">{$t('admin_playback_intro')}</p>
  <form onsubmit={save}><fieldset disabled={loading || saving}>
    <h3>{$t('playback_transcoding_heading')}</h3>
    <div class="settings-panel">
      <label class="policy-toggle"><input type="checkbox" bind:checked={video} />{$t('enable_video_transcoding')}</label>
      <label class="policy-toggle"><input type="checkbox" bind:checked={audio} />{$t('enable_audio_transcoding')}</label>
      <label class="policy-toggle"><input type="checkbox" bind:checked={toneMapping} disabled={!video} />{$t('enable_tone_mapping')}</label>
      <label class="policy-toggle"><input type="checkbox" bind:checked={burnIn} disabled={!video} />{$t('enable_subtitle_burn_in')}</label>
      <label>{$t('max_transcode_bitrate')}
        <input type="number" min="0" max="120" step="0.5" required bind:value={maxMbps} disabled={!video} />
        <small>{$t('max_transcode_bitrate_hint')}</small>
      </label>
    </div>
    <p class="hint">{$t('enable_fallback_transcoding_note')}</p>

    <h3>{$t('playback_devices_heading')}</h3>
    <div class="settings-panel">
      <label class="policy-toggle"><input type="checkbox" bind:checked={remote} />{$t('enable_remote_control')}</label>
      <label class="policy-toggle"><input type="checkbox" bind:checked={downloads} />{$t('enable_downloads')}</label>
      <label class="policy-toggle"><input type="checkbox" bind:checked={watchTogether} />{$t('enable_watch_together')}</label>
    </div>
    <p class="hint">{$t('enable_watch_together_note')}</p>
    <p class="hint">{$t('playback_devices_note')}</p>
    <div class="actions"><button type="submit">{saving ? $t('loading') : $t('save')}</button></div>
  </fieldset></form>
  {#if loading}<p role="status">{$t('loading_settings_short')}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
</section>

<style>
  section { display: grid; gap: 20px; min-width: 0; max-width: 760px; font-family: var(--font-sans); color: var(--color-text); }
  h2 { margin: 0; font-size: 26px; font-weight: 700; letter-spacing: -0.02em; line-height: 1.2; }
  h3 { margin: 8px 0 0; font-size: 16px; font-weight: 650; }
  p { margin: 0; font-size: 13px; line-height: 1.6; color: var(--color-muted); }
  .lead { font-size: 14px; margin-top: -12px; }
  fieldset { display: grid; gap: 12px; min-width: 0; margin: 0; padding: 0; border: 0; }
  .settings-panel { display: grid; padding: 4px 20px; border-radius: var(--radius-md); background: var(--color-surface); }
  .settings-panel > label { padding: 16px 0; border-top: 1px solid var(--color-line-subtle); }
  .settings-panel > label:first-child { border-top: 0; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 600; color: var(--color-text); }
  label small { font-size: 12px; font-weight: 400; color: var(--color-muted); }
  .policy-toggle { display: flex; align-items: center; gap: 12px; min-height: 44px; cursor: pointer; }
  .policy-toggle input { flex: none; width: 20px; height: 20px; min-height: 20px; margin: 0; accent-color: var(--color-accent); }
  .policy-toggle:has(input:disabled) { opacity: 0.55; cursor: default; }
  input[type='number'] { box-sizing: border-box; width: 100%; max-width: 200px; height: 42px; padding: 0 12px; border: 1px solid var(--color-line-subtle); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; font-size: 14px; font-weight: 400; }
  input[type='number']:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
  .actions { display: flex; justify-content: flex-end; }
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 40px; padding: 8px 20px; border: 0; border-radius: var(--radius-sm); background: var(--color-play); color: var(--color-on-play); font: inherit; font-size: 14px; font-weight: 600; cursor: pointer; transition: background-color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-play-hover); }
  button:disabled { opacity: 0.55; cursor: default; }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
</style>
