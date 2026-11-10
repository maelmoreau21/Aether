<script lang="ts">
  import { t } from '../i18n';
  import { featureModes, type FeatureMode } from './types';

  // Three-state choice of a feature that also has a server-wide setting: follow the server,
  // force it on, or force it off. "Follow the server" shows what the server currently does.
  let { value = $bindable(), label, hint = '', serverOn, id, badge = null }: {
    value: FeatureMode; label: string; hint?: string; id: string;
    /** What the server-wide setting is right now (undefined while it loads). */
    serverOn?: boolean;
    /** Optional state of what makes the feature work (e.g. the detection extension). */
    badge?: { tone: 'ok' | 'warn'; text: string } | null;
  } = $props();

  const modeLabel = (mode: FeatureMode) => mode === 'inherit'
    ? (serverOn === undefined ? $t('feature_mode_inherit') : $t('feature_mode_inherit_value', { value: serverOn ? $t('feature_mode_enabled') : $t('feature_mode_disabled') }))
    : mode === 'enabled' ? $t('feature_mode_enabled') : $t('feature_mode_disabled');
</script>

<fieldset class="mode" aria-labelledby="{id}-label">
  <legend id="{id}-label">
    <strong>{label}</strong>
    {#if badge}<span class="badge {badge.tone}">{badge.text}</span>{/if}
  </legend>
  {#if hint}<small>{hint}</small>{/if}
  <div class="options" role="radiogroup" aria-labelledby="{id}-label">
    {#each featureModes as mode (mode)}
      <label class:on={value === mode}>
        <input type="radio" name={id} value={mode} checked={value === mode} onchange={() => (value = mode)} />
        <span>{modeLabel(mode)}</span>
      </label>
    {/each}
  </div>
</fieldset>

<style>
  .mode { display: flex; flex-direction: column; gap: 8px; min-width: 0; margin: 0; padding: 0; border: 0; }
  legend { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 0; color: var(--tx); }
  small { color: var(--mu); font-size: 13px; line-height: 1.4; }
  .options { display: flex; flex-wrap: wrap; gap: 6px; }
  .options label { position: relative; display: inline-flex; align-items: center; min-height: 40px; padding: 0 14px; border: 1px solid var(--line-2); border-radius: var(--r-pill); background: none; color: var(--tx); font-size: 13px; font-weight: 700; cursor: pointer; }
  .options label.on { border-color: var(--ac); background: color-mix(in srgb, var(--ac) 18%, transparent); }
  .options label:has(input:focus-visible) { outline: 2px solid var(--tx); outline-offset: 2px; }
  .options input { position: absolute; opacity: 0; inset: 0; margin: 0; cursor: pointer; }
  .badge { padding: 2px 8px; border-radius: var(--r-pill); font-size: 12px; font-weight: 700; background: var(--pn-3); }
  .badge.ok { color: var(--ac); }
  .badge.warn { color: var(--mu); }
</style>
