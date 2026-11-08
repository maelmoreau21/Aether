<script lang="ts">
  import { untrack } from 'svelte';
  import { resolve } from '$app/paths';
  import Icon, { type IconName } from '../ui/Icon.svelte';
  import { adminRequest } from './request';
  import type { Library } from './types';
  import { t, fmt, type TranslationKey } from '../i18n';
  import { agoLabel, durationLabel } from './task-schedule';
  import {
    accessibleLibraries, automationModes, effectiveRule, errorKey, intervalChoices, needsSettings, policyFor, ruleSummary,
    runCounts, runGroup, runStatusKeys,
    type AutomationExtension, type AutomationMode, type AutomationPolicy, type AutomationRun, type MetadataSource, type ProviderQuota, type RunGroup
  } from './automation-rules';

  let { active = false, libraries }: { active?: boolean; libraries: Library[] } = $props();

  let installed = $state<AutomationExtension[]>([]);
  let policies = $state<AutomationPolicy[]>([]);
  let runs = $state<AutomationRun[]>([]);
  let quotas = $state<ProviderQuota[]>([]);
  let sources = $state<MetadataSource[]>([]);
  let serverNow = $state(Math.floor(Date.now() / 1000));
  let loaded = $state(false);
  let loadError = $state('');
  let error = $state('');
  let notice = $state('');
  let filter = $state<'all' | RunGroup>('all');
  let extensionFilter = $state('');
  let visible = $state(30);

  // Rule editor (one extension, one row per library it may read).
  type Draft = { libraryId: string; name: string; mode: AutomationMode; priority: number; intervalSeconds: number; nextRunAt: number };
  let editor = $state<HTMLDialogElement>();
  let editing = $state<AutomationExtension | null>(null);
  let drafts = $state<Draft[]>([]);
  let saving = $state(false);
  let running = $state<Record<string, boolean>>({});

  const usable = $derived(libraries.filter(library => !library.id.startsWith('fixture-')));
  const extensions = $derived(installed.filter(extension => extension.enabled && extension.manifest.permissions.includes('metadata.read')));
  const names = $derived(Object.fromEntries(installed.map(extension => [extension.manifest.id, extension.manifest.name])) as Record<string, string>);
  const counts = $derived(runCounts(runs));
  const filtered = $derived(runs.filter(run => (filter === 'all' || runGroup(run.status) === filter) && (!extensionFilter || run.extensionId === extensionFilter)));
  const historyExtensions = $derived([...new Set(runs.map(run => run.extensionId))]);
  const pendingQuotas = $derived(quotas.filter(quota => quota.nextAllowedAt > serverNow));

  const modeKeys: Record<AutomationMode, { name: TranslationKey; hint: TranslationKey }> = {
    automatic: { name: 'automation_mode_automatic', hint: 'automation_mode_automatic_hint' },
    scheduled: { name: 'automation_mode_scheduled', hint: 'automation_mode_scheduled_hint' },
    manual: { name: 'automation_mode_manual', hint: 'automation_mode_manual_hint' },
    disabled: { name: 'automation_mode_disabled', hint: 'automation_mode_disabled_hint' }
  };
  const groupIcons: Record<RunGroup, IconName> = { active: 'refresh', ok: 'check', failed: 'alert', cancelled: 'close' };
  const filters = [
    { id: 'all', key: 'automation_filter_all' }, { id: 'active', key: 'automation_filter_active' },
    { id: 'ok', key: 'automation_filter_ok' }, { id: 'failed', key: 'automation_filter_failed' }
  ] as const;

  function message(cause: unknown, fallback: TranslationKey) { return cause instanceof Error ? cause.message : $t(fallback); }
  function flash(text: string) { notice = text; setTimeout(() => { if (notice === text) notice = ''; }, 4000); }
  function libraryName(run: AutomationRun) {
    return run.libraryName || usable.find(library => library.id === (run.rootLibraryId ?? run.libraryId))?.name || run.libraryId;
  }
  function intervalName(seconds: number) {
    return seconds % 86400 === 0 ? $t('automation_every_days', { count: seconds / 86400 })
      : seconds % 3600 === 0 ? $t('automation_every_hours', { count: seconds / 3600 })
      : $t('automation_every_minutes', { count: Math.round(seconds / 60) });
  }

  async function loadHistory() {
    const recent = await adminRequest<{ now?: number; tasks: AutomationRun[]; quotas: ProviderQuota[]; sources: MetadataSource[] }>('/api/v1/admin/tasks/automation/history');
    runs = recent.tasks; quotas = recent.quotas; sources = recent.sources;
    serverNow = recent.now ?? Math.floor(Date.now() / 1000);
  }
  async function refresh() {
    loadError = '';
    try {
      const [all, configured] = await Promise.all([
        adminRequest<{ extensions: AutomationExtension[] }>('/api/v1/admin/extensions'),
        adminRequest<{ policies: AutomationPolicy[] }>('/api/v1/admin/tasks/automation/policies'),
        loadHistory()
      ]);
      installed = all.extensions; policies = configured.policies;
    } catch (cause) { loadError = message(cause, 'loading_impossible'); }
    finally { loaded = true; }
  }

  // Load once, then follow the history while the tab is open; quicker while work is queued.
  $effect(() => {
    if (!active) return;
    untrack(() => void refresh());
    let timer: ReturnType<typeof setTimeout>;
    const tick = () => {
      timer = setTimeout(async () => {
        if (document.visibilityState === 'visible' && !saving) { try { await loadHistory(); } catch { /* the next tick retries */ } }
        tick();
      }, counts.active ? 5000 : 20000);
    };
    tick();
    return () => clearTimeout(timer);
  });

  function configure(extension: AutomationExtension) {
    editing = extension; error = '';
    drafts = accessibleLibraries(extension, usable).map(library => {
      const rule = effectiveRule(policies, extension.manifest.id, library.id);
      return { libraryId: library.id, name: library.name, mode: rule.mode, priority: rule.priority, intervalSeconds: rule.intervalSeconds, nextRunAt: rule.nextRunAt };
    });
    editor?.showModal();
  }
  function changed(draft: Draft) {
    if (!editing) return false;
    const rule = effectiveRule(policies, editing.manifest.id, draft.libraryId);
    return rule.mode !== draft.mode || rule.priority !== draft.priority || (draft.mode === 'scheduled' && rule.intervalSeconds !== draft.intervalSeconds);
  }
  const dirty = $derived(drafts.filter(changed));

  async function save(event: SubmitEvent) {
    event.preventDefault();
    const extension = editing;
    if (!extension || saving) return;
    if (!dirty.length) { editor?.close(); return; }
    saving = true; error = '';
    try {
      // Only edited rows are saved: saving an automatic rule queues a full pass over the library.
      for (const draft of dirty) {
        const previous = policies.find(p => p.extensionId === extension.manifest.id && p.libraryId === draft.libraryId);
        const policy = policyFor(previous, extension, draft.libraryId, draft);
        await adminRequest('/api/v1/admin/tasks/automation/policies', { method: 'PUT', body: JSON.stringify(policy) });
        policies = [...policies.filter(p => p !== previous), policy];
      }
      editor?.close();
      flash($t('automation_rules_saved', { count: dirty.length }));
      void loadHistory().catch(() => undefined);
    } catch (cause) { error = message(cause, 'saving_failed'); }
    finally { saving = false; }
  }

  async function run(extensionId: string, libraryId: string, missingOnly: boolean) {
    const key = `${extensionId}:${libraryId}:${missingOnly}`;
    if (running[key]) return;
    running[key] = true; error = '';
    try {
      await adminRequest('/api/v1/admin/tasks/automation/run', { method: 'POST', body: JSON.stringify({ extensionId, libraryId, idempotencyKey: crypto.randomUUID(), missingOnly }) });
      flash(missingOnly ? $t('missing_items_scheduled') : $t('request_added_to_the_queue_unchanged'));
      setTimeout(() => void loadHistory().catch(() => undefined), 1500);
    } catch (cause) { error = message(cause, 'admission_failed'); }
    finally { running[key] = false; }
  }
</script>

<section aria-labelledby="automation-heading">
  <h2 id="automation-heading" class="sr-only">{$t('metadata_automation')}</h2>
  <p class="description">{$t('automation_intro')}</p>
  {#if loadError}<p class="feedback error" role="alert">{loadError} <button class="ghost" onclick={() => void refresh()}><Icon name="refresh" size={16} />{$t('request_retry')}</button></p>{/if}

  <div class="block" role="region" aria-labelledby="rules-heading">
    <h3 id="rules-heading" class="block-title">{$t('automation_rules')}</h3>
    {#if !loaded}
      <p class="empty">{$t('loading_settings_short')}</p>
    {:else if !extensions.length}
      <div class="empty-card">
        <p>{$t('no_active_metadata_extension')}</p>
        <a class="link" href={resolve('admin/libraries/#extensions-section')}>{$t('automation_manage_extensions')}</a>
      </div>
    {:else}
      <ul class="rules">
        {#each extensions as extension (extension.manifest.id)}
          {@const reachable = accessibleLibraries(extension, usable)}
          <li class="rule">
            <span class="rule-icon" aria-hidden="true"><Icon name="sparkles" size={18} /></span>
            <div class="rule-text">
              <span class="rule-name">{extension.manifest.name}</span>
              {#if reachable.length}
                <span class="rule-modes">
                  {#each ruleSummary(policies, extension.manifest.id, reachable) as group (group.mode)}
                    <span class="mode-pill" data-mode={group.mode}>{$t(modeKeys[group.mode].name)}<span class="pill-libraries">{group.names.length === reachable.length ? $t('automation_all_libraries') : group.names.join(', ')}</span></span>
                  {/each}
                </span>
              {:else}
                <span class="rule-warning"><Icon name="info" size={14} />{$t('automation_no_library_access')}</span>
              {/if}
            </div>
            <button class="secondary" disabled={!reachable.length} onclick={() => configure(extension)}><Icon name="settings" size={16} /><span>{$t('automation_configure')}</span></button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <div class="block" role="region" aria-labelledby="runs-heading">
    <header class="block-header">
      <h3 id="runs-heading" class="block-title">{$t('latest_runs')}</h3>
      <button class="icon" aria-label={$t('refresh')} title={$t('refresh')} onclick={() => void loadHistory().catch(cause => { error = message(cause, 'loading_impossible'); })}><Icon name="refresh" size={18} /></button>
    </header>
    {#if runs.length}
      <div class="toolbar">
        <div class="chips" role="group" aria-label={$t('automation_filter_label')}>
          {#each filters as option (option.id)}
            <button class="chip" aria-pressed={filter === option.id} data-filter={option.id} onclick={() => { filter = option.id; visible = 30; }}>
              {$t(option.key)}<span class="count">{counts[option.id]}</span>
            </button>
          {/each}
        </div>
        {#if historyExtensions.length > 1}
          <select bind:value={extensionFilter} aria-label={$t('automation_filter_extension')} onchange={() => { visible = 30; }}>
            <option value="">{$t('automation_all_extensions')}</option>
            {#each historyExtensions as id (id)}<option value={id}>{names[id] ?? id}</option>{/each}
          </select>
        {/if}
      </div>
      <ul class="runs">
        {#each filtered.slice(0, visible) as item (item.id)}
          {@const group = runGroup(item.status)}
          <li class="run" data-group={group}>
            <span class="status-icon" data-group={group} aria-hidden="true"><Icon name={groupIcons[group]} size={16} /></span>
            <div class="run-text">
              <div class="run-title">
                <span class="run-target">{item.target || names[item.extensionId] || item.extensionId}</span>
                <span class="run-status" data-group={group}>{runStatusKeys[item.status] ? $t(runStatusKeys[item.status]) : item.status}</span>
              </div>
              <div class="run-meta">
                <span>{names[item.extensionId] ?? item.extensionId}</span>
                <span>{libraryName(item)}</span>
                {#if item.finishedAt ?? item.createdAt}<span title={$fmt.dateTime(item.finishedAt ?? item.createdAt)}>{agoLabel(item.finishedAt ?? item.createdAt ?? serverNow, serverNow, $t)}</span>{/if}
                {#if item.durationSeconds != null}<span>{durationLabel(item.durationSeconds)}</span>{/if}
                {#if item.attempt > 1}<span>{$t('automation_attempts', { count: item.attempt })}</span>{/if}
              </div>
              {#if item.error && group === 'failed'}
                <p class="run-error">
                  {$t(errorKey(item.error))}
                  {#if needsSettings(item.error)}<a class="link" href={resolve('admin/libraries/#extensions-section')}>{$t('automation_open_extension_settings')}</a>{/if}
                  <code>{item.error}</code>
                </p>
              {/if}
            </div>
          </li>
        {:else}
          <li class="empty">{$t('automation_filter_empty')}</li>
        {/each}
      </ul>
      {#if filtered.length > visible}<button class="ghost more" onclick={() => { visible += 30; }}>{$t('automation_show_more', { count: filtered.length - visible })}</button>{/if}
    {:else if loaded}
      <p class="empty-card">{$t('automation_no_runs')}</p>
    {/if}
  </div>

  {#if quotas.length || sources.length}
    <div class="extras">
      {#if quotas.length}
        <details class="block">
          <summary>{$t('automation_provider_limits')}{#if pendingQuotas.length}<span class="count">{pendingQuotas.length}</span>{/if}</summary>
          <ul class="plain">
            {#each quotas as quota (quota.provider)}
              <li>
                <strong>{quota.provider}</strong>
                <span>{$t('automation_quota_rule', { seconds: quota.minIntervalSeconds, parallel: quota.maxConcurrent })}</span>
                <span class:waiting={quota.nextAllowedAt > serverNow}>{quota.nextAllowedAt > serverNow ? $t('automation_quota_next', { date: $fmt.dateTime(quota.nextAllowedAt) }) : $t('automation_quota_ready')}</span>
              </li>
            {/each}
          </ul>
        </details>
      {/if}
      {#if sources.length}
        <details class="block">
          <summary>{$t('recent_provenance')}</summary>
          <ul class="plain">
            {#each sources.slice(0, 20) as source (`${source.workId}:${source.field}`)}
              <li>
                <strong>{source.workTitle || source.workId}</strong>
                <span>{$t('automation_source_line', { field: source.field, extension: names[source.extensionId] ?? source.extensionId, provider: source.providerId || '—' })}</span>
                <span title={$fmt.dateTime(source.updatedAt)}>{agoLabel(source.updatedAt, serverNow, $t)}</span>
              </li>
            {/each}
          </ul>
        </details>
      {/if}
    </div>
  {/if}

  {#if error && !editor?.open}<p class="feedback error" role="alert">{error}</p>{/if}
  {#if notice}<p class="feedback" role="status">{notice}</p>{/if}
</section>

<dialog bind:this={editor} aria-labelledby="rule-editor-title" oncancel={(event) => { if (saving) event.preventDefault(); }} onclose={() => { editing = null; }}>
  <form onsubmit={save}>
    <header>
      <h2 id="rule-editor-title">{editing ? $t('automation_configure_title', { name: editing.manifest.name }) : ''}</h2>
      <p>{$t('automation_configure_hint')}</p>
    </header>
    <ul class="drafts">
      {#each drafts as draft (draft.libraryId)}
        <li class="draft" class:changed={changed(draft)}>
          <div class="draft-head">
            <strong>{draft.name}</strong>
            {#if editing}
              {@const extensionId = editing.manifest.id}
              <span class="draft-actions">
                <button type="button" class="ghost small" disabled={draft.mode === 'disabled' || running[`${extensionId}:${draft.libraryId}:true`]} title={$t('automation_run_missing_hint')} onclick={() => void run(extensionId, draft.libraryId, true)}>{$t('automation_run_missing')}</button>
                <button type="button" class="ghost small" disabled={draft.mode === 'disabled' || running[`${extensionId}:${draft.libraryId}:false`]} title={$t('automation_run_all_hint')} onclick={() => void run(extensionId, draft.libraryId, false)}><Icon name="play" size={14} />{$t('automation_run_all')}</button>
              </span>
            {/if}
          </div>
          <div class="draft-fields">
            <label>{$t('trigger')}
              <select bind:value={draft.mode} disabled={saving}>
                {#each automationModes as mode (mode)}<option value={mode}>{$t(modeKeys[mode].name)}</option>{/each}
              </select>
            </label>
            {#if draft.mode === 'scheduled'}
              <label>{$t('automation_interval')}
                <select bind:value={draft.intervalSeconds} disabled={saving}>
                  {#each intervalChoices(draft.intervalSeconds) as seconds (seconds)}<option value={seconds}>{intervalName(seconds)}</option>{/each}
                </select>
              </label>
            {/if}
            <label class="priority">{$t('priority')}
              <input type="number" min="0" max="1000" step="1" bind:value={draft.priority} disabled={saving} required />
            </label>
          </div>
          <small>{$t(modeKeys[draft.mode].hint)}{#if draft.mode === 'scheduled' && draft.nextRunAt > serverNow && !changed(draft)} · {$t('sched_next', { date: $fmt.dateTime(draft.nextRunAt) })}{/if}</small>
        </li>
      {/each}
    </ul>
    <p class="hint">{$t('automation_priority_hint')}</p>
    {#if error}<p class="feedback error" role="alert">{error}</p>{/if}
    {#if notice}<p class="feedback" role="status">{notice}</p>{/if}
    <div class="dialog-actions">
      <button type="button" class="secondary" disabled={saving} onclick={() => editor?.close()}>{$t('sched_cancel')}</button>
      <button type="submit" class="primary" disabled={saving || !dirty.length}>{saving ? $t('saving') : dirty.length ? $t('automation_save_count', { count: dirty.length }) : $t('save')}</button>
    </div>
  </form>
</dialog>

<style>
  section { display: grid; gap: 24px; min-width: 0; color: var(--color-text); font-family: var(--font-sans); }
  .description { max-width: 70ch; margin: 0; font-size: 14px; line-height: 1.5; color: var(--color-muted); }
  .block { display: grid; gap: 8px; min-width: 0; }
  .block-header { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .block-title { margin: 0; padding: 0 4px; font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: var(--color-muted); }
  .empty, .empty-card { margin: 0; font-size: 14px; line-height: 1.5; color: var(--color-muted); }
  .empty-card { display: grid; gap: 8px; justify-items: start; padding: 16px 20px; border-radius: var(--radius-md); background: var(--color-surface); }
  .link { color: var(--color-accent); font-size: 14px; font-weight: 600; text-underline-offset: 3px; }

  /* Règles */
  .rules, .runs { display: grid; margin: 0; padding: 0; list-style: none; background: var(--color-surface); border-radius: var(--radius-md); overflow: hidden; }
  .rule { display: flex; align-items: center; gap: 14px; padding: 14px 16px; border-top: 1px solid var(--color-line-subtle); }
  .rule:first-child, .run:first-child { border-top: 0; }
  .rule-icon { display: grid; place-items: center; flex: none; width: 36px; height: 36px; border-radius: var(--radius-full); background: var(--color-accent-soft); color: var(--color-accent); }
  .rule-text { display: grid; gap: 6px; flex: 1; min-width: 0; }
  .rule-name { font-size: 15px; font-weight: 600; }
  .rule-modes { display: flex; flex-wrap: wrap; gap: 6px; }
  .mode-pill { display: inline-flex; align-items: baseline; gap: 6px; max-width: 100%; padding: 3px 10px; border-radius: var(--radius-full); background: var(--color-raised); font-size: 12px; font-weight: 600; color: var(--color-text); }
  .mode-pill[data-mode='automatic'] { background: var(--color-success-soft); }
  .mode-pill[data-mode='scheduled'] { background: var(--color-accent-soft); }
  .mode-pill[data-mode='disabled'] { color: var(--color-muted); }
  .pill-libraries { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 400; color: var(--color-muted); }
  .rule-warning { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; color: var(--color-warning); }

  /* Historique */
  .toolbar { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px 12px; }
  .chips { display: flex; flex-wrap: wrap; gap: 8px; }
  .chip { min-height: 36px; padding: 6px 14px; border-radius: var(--radius-full); color: var(--color-muted); font-weight: 600; }
  .chip[aria-pressed='true'] { border-color: transparent; background: var(--color-accent-soft); color: var(--color-accent); }
  .count { min-width: 20px; padding: 1px 7px; border-radius: var(--radius-full); background: var(--color-raised); color: var(--color-text); font-size: 12px; font-variant-numeric: tabular-nums; text-align: center; }
  .chip[data-filter='failed'] .count { background: var(--color-danger-soft); color: var(--color-danger); }
  .chip[data-filter='active'] .count { background: var(--color-accent-soft); color: var(--color-accent); }
  .toolbar select { flex: 0 1 240px; }
  .run { display: flex; align-items: flex-start; gap: 12px; padding: 12px 16px; border-top: 1px solid var(--color-line-subtle); }
  .status-icon { display: grid; place-items: center; flex: none; width: 30px; height: 30px; margin-top: 1px; border-radius: var(--radius-full); background: var(--color-raised); color: var(--color-muted); }
  .status-icon[data-group='ok'] { background: var(--color-success-soft); color: var(--color-success); }
  .status-icon[data-group='failed'] { background: var(--color-danger-soft); color: var(--color-danger); }
  .status-icon[data-group='active'] { background: var(--color-accent-soft); color: var(--color-accent); }
  .run[data-group='active'] .status-icon :global(svg) { animation: spin 1.6s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .run-text { display: grid; gap: 3px; flex: 1; min-width: 0; }
  .run-title { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
  .run-target { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 14px; font-weight: 600; }
  .run-status { flex: none; font-size: 12px; font-weight: 600; color: var(--color-muted); }
  .run-status[data-group='ok'] { color: var(--color-success); }
  .run-status[data-group='failed'] { color: var(--color-danger); }
  .run-status[data-group='active'] { color: var(--color-accent); }
  .run-meta { display: flex; flex-wrap: wrap; gap: 2px 0; font-size: 13px; line-height: 1.4; color: var(--color-muted); font-variant-numeric: tabular-nums; }
  .run-meta > span + span::before { content: '·'; margin: 0 8px; color: var(--color-subtle); }
  .run-error { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; margin: 6px 0 0; padding: 8px 12px; border-radius: var(--radius-sm); background: var(--color-danger-soft); font-size: 13px; line-height: 1.45; overflow-wrap: anywhere; }
  .run-error .link { font-size: 13px; }
  .run-error code { font-family: var(--font-mono); font-size: 11.5px; color: var(--color-muted); }
  .runs > .empty { padding: 16px; }
  .more { justify-self: center; }

  /* Annexes */
  .extras { display: grid; gap: 8px; }
  details.block { padding: 0 16px; border-radius: var(--radius-md); background: var(--color-surface); }
  summary { display: flex; align-items: center; gap: 8px; min-height: 48px; cursor: pointer; font-size: 14px; font-weight: 600; }
  summary:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; border-radius: var(--radius-xs); }
  .plain { display: grid; margin: 0 0 12px; padding: 0; list-style: none; }
  .plain li { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 12px; padding: 8px 0; border-top: 1px solid var(--color-line-subtle); font-size: 13px; color: var(--color-muted); overflow-wrap: anywhere; }
  .plain strong { color: var(--color-text); font-weight: 600; }
  .plain .waiting { color: var(--color-warning); }

  /* Boutons */
  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 40px; padding: 8px 16px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: transparent; color: var(--color-text); font: inherit; font-size: 13px; cursor: pointer; transition: background-color var(--transition-fast), color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-raised); }
  button:focus-visible, .link:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .primary { border: 0; background: var(--color-accent-solid); color: var(--color-on-accent); font-weight: 600; }
  .primary:hover:not(:disabled) { background: var(--color-accent-hover); }
  .secondary { flex: none; font-weight: 600; }
  .ghost { border-color: transparent; color: var(--color-accent); font-weight: 600; }
  .ghost:hover:not(:disabled) { background: var(--color-accent-soft); }
  .small { min-height: 34px; padding: 4px 10px; font-size: 12px; }
  .icon { width: 40px; padding: 0; border: 0; border-radius: var(--radius-full); color: var(--color-muted); }
  .icon:hover:not(:disabled) { background: var(--color-accent-soft); color: var(--color-accent); }

  select, input { box-sizing: border-box; width: 100%; min-width: 0; min-height: 40px; padding: 8px 12px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; font-size: 14px; }
  select:focus-visible, input:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }

  .feedback { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; margin: 0; padding: 12px 16px; border-radius: var(--radius-sm); background: var(--color-success-soft); font-size: 14px; line-height: 1.5; }
  .feedback.error { background: var(--color-danger-soft); }

  /* Éditeur */
  dialog { width: min(640px, calc(100vw - 32px)); max-height: calc(100dvh - 40px); overflow-y: auto; padding: 24px; border: 0; border-radius: var(--radius-xl); background: var(--color-surface-elevated); color: var(--color-text); font-family: var(--font-sans); box-shadow: var(--shadow-lg); }
  dialog::backdrop { background: var(--color-overlay); }
  dialog form { display: grid; gap: 16px; }
  dialog header { display: grid; gap: 6px; }
  dialog h2 { margin: 0; font-size: 20px; font-weight: 700; }
  dialog header p, .hint { margin: 0; font-size: 13px; line-height: 1.5; color: var(--color-muted); }
  .drafts { display: grid; gap: 10px; margin: 0; padding: 0; list-style: none; }
  .draft { display: grid; gap: 10px; padding: 14px 16px; border: 1px solid var(--color-line-subtle); border-radius: var(--radius-md); background: var(--color-surface); transition: border-color var(--transition-fast); }
  .draft.changed { border-color: var(--color-accent); }
  .draft-head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; }
  .draft-head strong { font-size: 15px; }
  .draft-actions { display: flex; flex-wrap: wrap; gap: 4px; margin-right: -8px; }
  .draft-fields { display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 10px; }
  .draft-fields label { display: grid; gap: 6px; font-size: 12px; color: var(--color-muted); }
  .draft-fields .priority { max-width: 140px; }
  .draft small { font-size: 12.5px; line-height: 1.45; color: var(--color-muted); }
  .dialog-actions { display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 12px; }

  @media (max-width: 640px) {
    .rule { flex-wrap: wrap; padding-inline: 14px; }
    .rule .secondary { flex: 1 1 100%; }
    .run { padding-inline: 12px; }
    .run-title { flex-wrap: wrap; gap: 2px 8px; }
    .run-target { white-space: normal; }
    select, input { font-size: 16px; }
    .toolbar select { flex: 1 1 100%; }
    dialog { width: 100vw; max-width: 100vw; margin: auto 0 0; padding: 20px 16px calc(20px + env(safe-area-inset-bottom, 0px)); border-radius: var(--radius-xl) var(--radius-xl) 0 0; }
    .draft-fields .priority { max-width: none; }
    .dialog-actions button { flex: 1 1 auto; }
  }
  @media (prefers-reduced-motion: reduce) {
    button, .draft { transition: none; }
    .run[data-group='active'] .status-icon :global(svg) { animation: none; }
  }
</style>
