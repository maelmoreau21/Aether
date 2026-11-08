<script lang="ts">
  import Icon from '../ui/Icon.svelte';
  import { adminRequest } from './request';
  import { t, fmt, type TranslationKey } from '../i18n';
  import { fileSizeLabel, logFileDay, logLevelRank, logLevels, parseLogText, retentionChoices, type LogFile, type LogLevel, type LogLine, type LogSettings } from './log-files';

  let { active = false }: { active?: boolean } = $props();

  let settings = $state<LogSettings | null>(null);
  let level = $state<LogLevel>('info');
  let retention = $state(14);
  let files = $state<LogFile[]>([]);
  let loaded = $state(false);
  let saving = $state(false);
  let error = $state('');
  let notice = $state('');
  let attempted = false;

  let opened = $state<string | null>(null);
  let lines = $state<LogLine[]>([]);
  let truncated = $state(false);
  let reading = $state(false);
  let readError = $state('');
  let minimum = $state<LogLevel>('debug');
  let search = $state('');

  const levelKeys: Record<LogLevel, { name: TranslationKey; hint: TranslationKey }> = {
    debug: { name: 'logs_level_debug', hint: 'logs_level_debug_hint' },
    info: { name: 'logs_level_info', hint: 'logs_level_info_hint' },
    warning: { name: 'logs_level_warning_only', hint: 'logs_level_warning_hint' },
    error: { name: 'logs_level_error_only', hint: 'logs_level_error_hint' }
  };
  const badges = { debug: 'logs_badge_debug', info: 'logs_badge_info', warning: 'logs_badge_warning', error: 'logs_badge_error' } as const;
  const dirty = $derived(!!settings && (settings.level !== level || settings.retentionDays !== retention));
  const today = $derived(new Date().toISOString().slice(0, 10));
  const needle = $derived(search.trim().toLowerCase());
  const shown = $derived(lines.filter(line => logLevelRank[line.level] >= logLevelRank[minimum] && (!needle || line.message.toLowerCase().includes(needle))));

  function message(cause: unknown, fallback: TranslationKey) { return cause instanceof Error ? cause.message : $t(fallback); }
  function flash(text: string) { notice = text; setTimeout(() => { if (notice === text) notice = ''; }, 4000); }

  async function load() {
    error = '';
    try {
      const [current, listing] = await Promise.all([
        adminRequest<LogSettings>('/api/v1/admin/logs/settings'),
        adminRequest<{ files: LogFile[] }>('/api/v1/admin/logs/files')
      ]);
      settings = current; level = current.level; retention = current.retentionDays;
      files = listing.files;
    } catch (cause) { error = message(cause, 'logs_load_failed'); }
    finally { loaded = true; }
  }
  $effect(() => { if (active && !attempted) { attempted = true; void load(); } });

  async function save() {
    if (!dirty || saving) return;
    saving = true; error = '';
    try {
      settings = await adminRequest<LogSettings>('/api/v1/admin/logs/settings', { method: 'PUT', body: JSON.stringify({ level, retentionDays: retention }) });
      level = settings.level; retention = settings.retentionDays;
      flash($t('logs_settings_saved'));
      const listing = await adminRequest<{ files: LogFile[] }>('/api/v1/admin/logs/files');
      files = listing.files;
    } catch (cause) { error = message(cause, 'saving_failed'); }
    finally { saving = false; }
  }

  async function open(name: string) {
    opened = name; reading = true; readError = ''; lines = []; truncated = false; search = ''; minimum = 'debug';
    try {
      const response = await fetch(`/api/v1/admin/logs/files/${encodeURIComponent(name)}`, { credentials: 'same-origin', cache: 'no-store' });
      if (!response.ok) throw new Error($t('logs_file_read_failed'));
      truncated = response.headers.get('x-log-truncated') === '1';
      lines = parseLogText(await response.text());
    } catch (cause) { readError = message(cause, 'logs_file_read_failed'); }
    finally { reading = false; }
  }

  function dayLabel(file: LogFile) {
    const day = logFileDay(file.name);
    if (!day) return file.name;
    return day === today ? $t('logs_today') : $fmt.date(`${day}T12:00:00Z`, 'long');
  }
  function clock(stamp: string) { return stamp ? stamp.slice(11, 19) : ''; }
  const downloadHref = (name: string) => `/api/v1/admin/logs/files/${encodeURIComponent(name)}?download=true`;
</script>

<section aria-labelledby="log-files-heading">
  <h2 id="log-files-heading" class="sr-only">{$t('logs_files_title')}</h2>

  {#if opened}
    <button class="back" onclick={() => { opened = null; }}><Icon name="back" size={20} /><span>{$t('logs_back_to_files')}</span></button>
    <header class="viewer-header">
      <div>
        <h3 class="viewer-title">{opened}</h3>
        <p class="description">{reading ? $t('logs_loading') : $t('logs_file_lines', { count: lines.length })}{#if truncated} · {$t('logs_file_truncated')}{/if}</p>
      </div>
      <div class="actions">
        <button class="icon" aria-label={$t('refresh')} title={$t('refresh')} disabled={reading} onclick={() => opened && void open(opened)}><Icon name="refresh" size={18} /></button>
        <a class="button" href={downloadHref(opened)} download={opened}><Icon name="download" size={18} /><span>{$t('download')}</span></a>
      </div>
    </header>
    <div class="toolbar">
      <div class="chips" role="group" aria-label={$t('logs_level_label')}>
        {#each logLevels as option (option)}
          <button class="chip" aria-pressed={minimum === option} onclick={() => { minimum = option; }}>{option === 'debug' ? $t('logs_level_all') : $t(levelKeys[option].name)}</button>
        {/each}
      </div>
      <label class="search">
        <Icon name="search" size={16} />
        <input type="search" bind:value={search} placeholder={$t('logs_search')} aria-label={$t('logs_search')} />
      </label>
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="viewport" role="log" aria-label={opened} tabindex="0">
      {#if reading}<p class="empty">{$t('logs_loading')}</p>
      {:else if readError}<p class="empty" role="alert">{readError}</p>
      {:else if !lines.length}<p class="empty">{$t('logs_file_empty')}</p>
      {:else if !shown.length}<p class="empty">{$t('logs_no_match')}</p>
      {:else}
        {#each shown as line (line.index)}
          <div class="line" data-level={line.level}>
            <time datetime={line.stamp || undefined}>{clock(line.stamp)}</time>
            <span class="badge">{$t(badges[line.level])}</span>
            <span class="message">{line.message}</span>
          </div>
        {/each}
      {/if}
    </div>
  {:else}
    <p class="description">{$t('logs_files_intro')}</p>

    <div class="card" role="region" aria-labelledby="log-level-heading">
      <header class="card-header"><h3 id="log-level-heading">{$t('logs_level_setting')}</h3></header>
      <fieldset class="levels" disabled={!settings || saving}>
        <legend class="sr-only">{$t('logs_level_setting')}</legend>
        {#each logLevels as option (option)}
          <label class="level" class:selected={level === option}>
            <input type="radio" name="log-level" value={option} bind:group={level} />
            <span class="level-text">
              <strong>{$t(levelKeys[option].name)}{#if option === 'info'}<em>{$t('logs_recommended')}</em>{/if}</strong>
              <small>{$t(levelKeys[option].hint)}</small>
            </span>
          </label>
        {/each}
      </fieldset>
      <div class="settings-foot">
        <label class="retention">{$t('logs_retention')}
          <select bind:value={retention} disabled={!settings || saving}>
            {#each retentionChoices(settings?.retentionDays ?? retention) as days (days)}<option value={days}>{$t('logs_retention_days', { count: days })}</option>{/each}
          </select>
        </label>
        <button class="primary" disabled={!dirty || saving} onclick={() => void save()}>{saving ? $t('saving') : $t('save')}</button>
      </div>
      {#if settings && !settings.filesEnabled}<p class="warning" role="note">{$t('logs_files_disabled')}</p>
      {:else if settings?.directory}<p class="meta">{$t('logs_directory', { path: settings.directory })}</p>{/if}
    </div>

    <div class="card" role="region" aria-labelledby="log-list-heading">
      <header class="card-header">
        <h3 id="log-list-heading">{$t('logs_files_title')}</h3>
        <button class="icon" aria-label={$t('refresh')} title={$t('refresh')} onclick={() => void load()}><Icon name="refresh" size={18} /></button>
      </header>
      {#if !loaded}<p class="empty">{$t('logs_loading')}</p>
      {:else if !files.length}<p class="empty">{$t('logs_no_files')}</p>
      {:else}
        <ul class="files">
          {#each files as file (file.name)}
            <li>
              <button class="file" onclick={() => void open(file.name)}>
                <span class="file-icon" aria-hidden="true"><Icon name="file" size={18} /></span>
                <span class="file-text">
                  <span class="file-day">{dayLabel(file)}</span>
                  <span class="file-meta">{file.name} · {fileSizeLabel(file.size, $fmt.tag)}</span>
                </span>
              </button>
              <a class="icon-link" href={downloadHref(file.name)} download={file.name} aria-label={$t('logs_download_file', { name: file.name })} title={$t('download')}><Icon name="download" size={18} /></a>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
  {#if error}<p class="feedback error" role="alert">{error}</p>{/if}
  {#if notice}<p class="feedback" role="status">{notice}</p>{/if}
</section>

<style>
  section { display: grid; gap: 16px; min-width: 0; color: var(--color-text); font-family: var(--font-sans); }
  .description { max-width: 70ch; margin: 0; font-size: 14px; line-height: 1.5; color: var(--color-muted); }

  .card { min-width: 0; padding: 4px 20px 16px; background: var(--color-surface); border-radius: var(--radius-md); }
  .card-header { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 52px; }
  .card-header h3 { margin: 0; font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: var(--color-muted); }

  .levels { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 220px), 1fr)); gap: 8px; margin: 0; padding: 0; border: 0; }
  .levels:disabled { opacity: 0.6; }
  .level { display: flex; align-items: flex-start; gap: 10px; padding: 12px 14px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); cursor: pointer; transition: border-color var(--transition-fast), background-color var(--transition-fast); }
  .level:hover { background: var(--color-surface-hover); }
  .level.selected { border-color: var(--color-accent); background: var(--color-accent-soft); }
  .level:focus-within { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .level input { margin: 3px 0 0; accent-color: var(--color-accent); }
  .level-text { display: grid; gap: 4px; min-width: 0; }
  .level-text strong { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; font-size: 14px; font-weight: 600; }
  .level-text em { padding: 1px 8px; border-radius: var(--radius-full); background: var(--color-raised); color: var(--color-muted); font-size: 11px; font-style: normal; font-weight: 600; }
  .level-text small { font-size: 13px; line-height: 1.45; color: var(--color-muted); }
  .settings-foot { display: flex; flex-wrap: wrap; align-items: end; justify-content: space-between; gap: 12px; margin-top: 16px; }
  .retention { display: grid; gap: 6px; flex: 0 1 260px; font-size: 13px; color: var(--color-muted); }
  select, input[type='search'] { box-sizing: border-box; width: 100%; min-width: 0; min-height: 40px; padding: 8px 12px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: var(--color-raised); color: var(--color-text); font: inherit; font-size: 14px; }
  select:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .meta, .warning, .empty { margin: 12px 0 0; font-size: 13px; line-height: 1.5; color: var(--color-muted); overflow-wrap: anywhere; }
  .warning { padding: 10px 14px; border-radius: var(--radius-sm); background: var(--color-warning-soft); color: var(--color-text); }
  .empty { margin: 0; padding: 8px 0 4px; font-size: 14px; }

  .files { display: grid; margin: 0 -8px; padding: 0; list-style: none; }
  .files li { display: flex; align-items: center; gap: 4px; border-top: 1px solid var(--color-line-subtle); }
  .files li:first-child { border-top: 0; }
  .file { flex: 1; justify-content: flex-start; gap: 12px; min-width: 0; min-height: 56px; padding: 8px; border: 0; border-radius: var(--radius-sm); text-align: left; }
  .file-icon { display: grid; place-items: center; flex: none; width: 34px; height: 34px; border-radius: var(--radius-full); background: var(--color-raised); color: var(--color-muted); }
  .file-text { display: grid; gap: 2px; min-width: 0; }
  .file-day { font-size: 15px; font-weight: 600; color: var(--color-text); }
  .file-meta { font-size: 13px; color: var(--color-muted); overflow-wrap: anywhere; font-variant-numeric: tabular-nums; }

  button, .button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 40px; padding: 8px 16px; border: 1px solid var(--color-line); border-radius: var(--radius-sm); background: transparent; color: var(--color-text); font: inherit; font-size: 13px; text-decoration: none; cursor: pointer; transition: background-color var(--transition-fast), color var(--transition-fast); }
  button:hover:not(:disabled), .button:hover { background: var(--color-raised); }
  button:focus-visible, .button:focus-visible, .icon-link:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .primary { border: 0; background: var(--color-accent-solid); color: var(--color-on-accent); font-weight: 600; }
  .primary:hover:not(:disabled) { background: var(--color-accent-hover); }
  .icon, .icon-link { display: inline-grid; place-items: center; flex: none; width: 40px; min-height: 40px; height: 40px; padding: 0; border: 0; border-radius: var(--radius-full); color: var(--color-muted); }
  .icon-link:hover, .icon:hover:not(:disabled) { background: var(--color-accent-soft); color: var(--color-accent); }
  .back { justify-self: start; margin: 0 0 -4px -8px; padding: 0 12px 0 8px; border: 0; border-radius: var(--radius-full); color: var(--color-muted); font-size: 14px; }
  .back:hover { color: var(--color-text); }

  .viewer-header { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px; }
  .viewer-title { margin: 0 0 4px; font-family: var(--font-mono); font-size: 18px; font-weight: 600; overflow-wrap: anywhere; }
  .actions { display: flex; align-items: center; gap: 8px; }
  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; }
  .chips { display: flex; flex-wrap: wrap; gap: 8px; }
  .chip { min-height: 36px; padding: 6px 14px; border-radius: var(--radius-full); color: var(--color-muted); font-weight: 600; }
  .chip[aria-pressed='true'] { border-color: transparent; background: var(--color-accent-soft); color: var(--color-accent); }
  .search { display: flex; align-items: center; gap: 8px; flex: 1 1 220px; min-width: 0; padding: 0 12px; border: 1px solid var(--color-line); border-radius: var(--radius-full); background: var(--color-raised); color: var(--color-muted); }
  .search:focus-within { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .search input, .search input:focus { flex: 1; min-height: 36px; padding: 0; border: 0; background: transparent; box-shadow: none; outline: none; }

  .viewport { height: min(66vh, 680px); min-height: 280px; overflow: auto; padding: 8px 0; border-radius: var(--radius-md); background: var(--color-surface); font-family: var(--font-mono); font-size: 12.5px; line-height: 1.55; }
  .viewport:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .viewport .empty { padding: 24px 16px; font-family: var(--font-sans); }
  .line { display: grid; grid-template-columns: auto auto 1fr; align-items: baseline; gap: 12px; padding: 3px 16px; border-left: 3px solid transparent; }
  .line:hover { background: var(--color-surface-hover); }
  .line[data-level='warning'] { border-left-color: var(--color-warning); }
  .line[data-level='error'] { border-left-color: var(--color-danger); background: color-mix(in srgb, var(--color-danger-soft) 45%, transparent); }
  .line[data-level='debug'] .message { color: var(--color-muted); }
  time { color: var(--color-muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .badge { min-width: 52px; font-family: var(--font-sans); font-size: 11px; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; color: var(--color-muted); }
  .line[data-level='warning'] .badge { color: var(--color-warning); }
  .line[data-level='error'] .badge { color: var(--color-danger); }
  .message { min-width: 0; white-space: pre-wrap; overflow-wrap: anywhere; color: var(--color-text); }

  .feedback { margin: 0; padding: 12px 16px; border-radius: var(--radius-sm); background: var(--color-success-soft); font-size: 14px; }
  .feedback.error { background: var(--color-danger-soft); }

  @media (max-width: 640px) {
    .card { padding-inline: 16px; }
    .settings-foot > * { flex: 1 1 100%; }
    select, input[type='search'] { font-size: 16px; }
    .search { order: 3; flex-basis: 100%; }
    .line { grid-template-columns: auto 1fr; gap: 2px 10px; padding-inline: 12px; }
    .line .message { grid-column: 1 / -1; }
  }
  @media (prefers-reduced-motion: reduce) { button, .button, .level { transition: none; } }
</style>
