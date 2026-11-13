<script lang="ts">
  import Icon from '../ui/Icon.svelte';
  import { t } from '../i18n';
  import type { ClassRule, MyPolicy, PolicyRule } from '../applications/api';

  let {
    rule = $bindable<PolicyRule>({ film: {}, series: {}, music: {} }),
    effective = null,
    isAdminTarget = false,
    requestsEnabled = false,
    disabled = false
  }: {
    rule: PolicyRule;
    effective?: MyPolicy | null;
    isAdminTarget?: boolean;
    requestsEnabled?: boolean;
    disabled?: boolean;
  } = $props();

  const classes = ['film', 'series', 'music'] as const;
  type MediaClass = (typeof classes)[number];

  function classLabel(mediaClass: MediaClass): string {
    return mediaClass === 'film' ? $t('kind_movie') : mediaClass === 'series' ? $t('kind_series') : $t('audio');
  }

  function ensureClass(mediaClass: MediaClass): ClassRule {
    if (!rule[mediaClass]) rule[mediaClass] = {};
    return rule[mediaClass]!;
  }

  function boolValue(v: boolean | undefined): string {
    return v === true ? 'true' : v === false ? 'false' : 'inherit';
  }

  function setBool(mediaClass: MediaClass, field: 'allow' | 'autoApprove', value: string) {
    const target = ensureClass(mediaClass);
    if (value === 'inherit') {
      delete target[field];
    } else {
      target[field] = value === 'true';
    }
    rule = { ...rule };
  }

  function setQuotaLimit(mediaClass: MediaClass, raw: string) {
    const target = ensureClass(mediaClass);
    const trimmed = raw.trim();
    if (!trimmed) {
      delete target.quota;
    } else {
      const val = Math.max(0, Math.min(10000, Math.floor(Number(trimmed) || 0)));
      target.quota = { limit: val, days: target.quota?.days ?? 7 };
    }
    rule = { ...rule };
  }

  function setQuotaDays(mediaClass: MediaClass, raw: string) {
    const target = ensureClass(mediaClass);
    if (!target.quota) return;
    target.quota = {
      limit: target.quota.limit,
      days: Math.max(1, Math.min(365, Math.floor(Number(raw) || 7)))
    };
    rule = { ...rule };
  }

  const hasOverrides = $derived(
    classes.some(c => {
      const cr = rule[c];
      return cr && (cr.allow !== undefined || cr.autoApprove !== undefined || cr.quota !== undefined);
    })
  );

  function resetAll() {
    rule = { film: {}, series: {}, music: {} };
  }
</script>

<div class="dsm-form-section ext-perms-section">
  <h4 class="form-section-title">{$t('ext_permissions_title')}</h4>
  <p class="section-desc">{$t('ext_permissions_desc')}</p>

  {#if !requestsEnabled}
    <div class="dsm-info-box">
      <Icon name="info" size={18} />
      <span>{$t('ext_permissions_none')}</span>
    </div>
  {:else}
    <div class="ext-perm-card">
      <div class="ext-perm-card-header">
        <div class="ext-perm-title">
          <Icon name="download" size={16} />
          <strong>{$t('ext_requests_name')}</strong>
        </div>
        {#if !isAdminTarget && hasOverrides}
          <button type="button" class="btn-link-reset" {disabled} onclick={resetAll}>
            {$t('ext_reset_overrides')}
          </button>
        {/if}
      </div>

      {#if isAdminTarget}
        <div class="dsm-info-box">
          <Icon name="shield" size={18} />
          <span>{$t('ext_requests_admin_bypass')}</span>
        </div>
      {:else}
        {#if effective}
          <div class="ext-effective-box">
            <span class="ext-effective-label">{$t('ext_effective_summary')}</span>
            {#if effective.restricted}
              <p class="ext-restricted-note">{$t('rq_policy_user_restricted')}</p>
            {:else}
              <div class="ext-effective-pills">
                {#each classes as mc (mc)}
                  {@const g = effective.classes[mc]}
                  <span class="ext-eff-pill" class:denied={!g.allow}>
                    <strong>{classLabel(mc)} :</strong>
                    {g.allow ? $t('rq_policy_badge_allowed') : $t('rq_policy_badge_denied')}
                    {#if g.allow}
                      · {g.autoApprove ? $t('rq_policy_badge_auto') : $t('rq_policy_badge_review')}
                      {#if g.quota}
                        · {g.quota.remaining}/{g.quota.limit} ({g.quota.days}j)
                      {/if}
                    {/if}
                  </span>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

        <div class="ext-classes-grid">
          {#each classes as mc (mc)}
            {@const cr = rule[mc] ?? {}}
            <div class="ext-class-box">
              <h5>{classLabel(mc)}</h5>
              <label class="ext-field">
                <span>{$t('rq_policy_allow')}</span>
                <select
                  {disabled}
                  value={boolValue(cr.allow)}
                  onchange={e => setBool(mc, 'allow', e.currentTarget.value)}
                >
                  <option value="inherit">{$t('ext_inherit')}</option>
                  <option value="true">{$t('ext_allow')}</option>
                  <option value="false">{$t('ext_deny')}</option>
                </select>
              </label>

              <label class="ext-field">
                <span>{$t('rq_policy_auto')}</span>
                <select
                  {disabled}
                  value={boolValue(cr.autoApprove)}
                  onchange={e => setBool(mc, 'autoApprove', e.currentTarget.value)}
                >
                  <option value="inherit">{$t('ext_inherit')}</option>
                  <option value="true">{$t('rq_policy_auto_direct')}</option>
                  <option value="false">{$t('rq_policy_auto_review')}</option>
                </select>
              </label>

              <div class="ext-field">
                <span>{$t('rq_policy_quota')}</span>
                <div class="ext-quota-row">
                  <input
                    type="number"
                    min="0"
                    max="10000"
                    {disabled}
                    placeholder={$t('ext_inherit')}
                    aria-label={$t('rq_policy_quota_limit_label')}
                    value={cr.quota?.limit ?? ''}
                    oninput={e => setQuotaLimit(mc, e.currentTarget.value)}
                  />
                  {#if cr.quota !== undefined}
                    <input
                      type="number"
                      min="1"
                      max="365"
                      {disabled}
                      aria-label={$t('rq_policy_quota_days_label')}
                      value={cr.quota.days}
                      oninput={e => setQuotaDays(mc, e.currentTarget.value)}
                    />
                  {/if}
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .ext-perm-card {
    border: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.1));
    border-radius: 8px;
    padding: 1rem;
    background: var(--surface-elevated, rgba(255, 255, 255, 0.02));
    display: flex;
    flex-direction: column;
    gap: 0.85rem;
  }
  .ext-perm-card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .ext-perm-title {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.95rem;
  }
  .btn-link-reset {
    background: none;
    border: none;
    color: var(--accent, #5ea2ff);
    cursor: pointer;
    font-size: 0.85rem;
    padding: 0.2rem 0.4rem;
  }
  .btn-link-reset:hover {
    text-decoration: underline;
  }
  .ext-effective-box {
    background: var(--surface-subtle, rgba(255, 255, 255, 0.03));
    border: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.08));
    border-radius: 6px;
    padding: 0.65rem 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }
  .ext-effective-label {
    font-size: 0.78rem;
    opacity: 0.75;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .ext-effective-pills {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  .ext-eff-pill {
    font-size: 0.82rem;
    padding: 0.25rem 0.55rem;
    border-radius: 999px;
    background: rgba(80, 180, 120, 0.14);
    border: 1px solid rgba(80, 180, 120, 0.3);
  }
  .ext-eff-pill.denied {
    background: rgba(230, 80, 80, 0.14);
    border-color: rgba(230, 80, 80, 0.3);
  }
  .ext-restricted-note {
    margin: 0;
    font-size: 0.85rem;
    color: var(--danger, #ff6b6b);
  }
  .ext-classes-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
    gap: 0.75rem;
  }
  .ext-class-box {
    border: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.08));
    border-radius: 6px;
    padding: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .ext-class-box h5 {
    margin: 0;
    font-size: 0.9rem;
  }
  .ext-field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.82rem;
  }
  .ext-field span {
    opacity: 0.8;
  }
  .ext-quota-row {
    display: flex;
    gap: 0.4rem;
  }
  .ext-quota-row input {
    width: 100%;
    min-width: 0;
  }
</style>
