<script lang="ts">
  import { tick, untrack } from 'svelte';
  import Icon from '../ui/Icon.svelte';
  import { adminRequest } from './request';
  import { t, fmt } from '../i18n';

  let { active = false }: { active?: boolean } = $props();

  type Level = 'info' | 'warning' | 'error';
  type Entry = { seq: number; at: number; level: Level; message: string };
  const MAX_KEPT = 2000;

  let entries = $state<Entry[]>([]);
  let latest = 0;
  let loaded = $state(false);
  let failed = $state(false);
  let paused = $state(false);
  let level = $state<'all' | 'warning' | 'error'>('all');
  let search = $state('');
  let notice = $state('');
  let viewport = $state<HTMLDivElement>();
  let following = true;
  let inflight = false;

  const rank: Record<Level, number> = { info: 0, warning: 1, error: 2 };
  const needle = $derived(search.trim().toLowerCase());
  const shown = $derived(entries.filter(entry =>
    (level === 'all' || rank[entry.level] >= rank[level]) && (!needle || entry.message.toLowerCase().includes(needle))));
  const counts = $derived({ warning: entries.filter(e => e.level === 'warning').length, error: entries.filter(e => e.level === 'error').length });
  const levels = [
    { id: 'all', key: 'logs_level_all' }, { id: 'warning', key: 'logs_level_warning' }, { id: 'error', key: 'logs_level_error' }
  ] as const;
  const badges = { info: 'logs_badge_info', warning: 'logs_badge_warning', error: 'logs_badge_error' } as const;

  function time(at: number) { return $fmt.dateTime(Math.floor(at / 1000)); }
  function clock(at: number) {
    const date = new Date(at);
    return [date.getHours(), date.getMinutes(), date.getSeconds()].map(part => String(part).padStart(2, '0')).join(':');
  }

  async function poll() {
    if (inflight) return;
    inflight = true;
    try {
      const page = await adminRequest<{ entries: Entry[]; latest: number }>(`/api/v1/admin/logs?after=${latest}`);
      // A restarted server counts from 1 again: start over instead of waiting for old numbers.
      if (page.latest < latest) { entries = []; latest = 0; }
      if (page.entries.length) {
        const merged = [...entries, ...page.entries];
        entries = merged.length > MAX_KEPT ? merged.slice(merged.length - MAX_KEPT) : merged;
        latest = page.entries[page.entries.length - 1].seq;
      } else latest = Math.max(latest, page.latest);
      failed = false;
      if (following) { await tick(); viewport?.scrollTo({ top: viewport.scrollHeight }); }
    } catch {
      failed = true;
    } finally {
      loaded = true;
      inflight = false;
    }
  }

  $effect(() => {
    if (!active || paused) return;
    untrack(() => void poll());
    const timer = setInterval(() => { if (document.visibilityState === 'visible') void poll(); }, 3000);
    return () => clearInterval(timer);
  });

  function onscroll() {
    if (!viewport) return;
    following = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < 40;
  }
  function text() {
    return shown.map(entry => `${new Date(entry.at).toISOString()} ${entry.level.toUpperCase().padEnd(7)} ${entry.message}`).join('\n');
  }
  async function copy() {
    try { await navigator.clipboard.writeText(text()); notice = $t('logs_copied'); }
    catch { notice = $t('logs_copy_failed'); }
    setTimeout(() => { notice = ''; }, 3000);
  }
  function download() {
    const url = URL.createObjectURL(new Blob([`${text()}\n`], { type: 'text/plain;charset=utf-8' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = `aether-server-${new Date().toISOString().replace(/[:.]/g, '-')}.log`;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
</script>

<section aria-labelledby="logs-heading">
  <!-- The active tab already names the page: the heading stays for assistive technologies. -->
  <h2 id="logs-heading" class="sr-only">{$t('logs_title')}</h2>
  <p class="description">{$t('logs_intro')}</p>

  <div class="toolbar">
    <div class="levels" role="group" aria-label={$t('logs_level_label')}>
      {#each levels as option (option.id)}
        <button class="chip" aria-pressed={level === option.id} data-level={option.id} onclick={() => { level = option.id; }}>
          {$t(option.key)}{#if option.id !== 'all'}<span class="count">{counts[option.id]}</span>{/if}
        </button>
      {/each}
    </div>
    <label class="search">
      <Icon name="search" size={16} />
      <input type="search" bind:value={search} placeholder={$t('logs_search')} aria-label={$t('logs_search')} />
    </label>
    <div class="actions">
      <button class="icon" aria-pressed={paused} aria-label={paused ? $t('logs_resume') : $t('logs_pause')} title={paused ? $t('logs_resume') : $t('logs_pause')} onclick={() => { paused = !paused; }}>
        <Icon name={paused ? 'play' : 'pause'} size={18} />
      </button>
      <button class="icon" aria-label={$t('logs_copy')} title={$t('logs_copy')} disabled={!shown.length} onclick={() => void copy()}><Icon name="copy" size={18} /></button>
      <button class="icon" aria-label={$t('logs_download')} title={$t('logs_download')} disabled={!shown.length} onclick={download}><Icon name="download" size={18} /></button>
    </div>
  </div>

  <p class="live" role="status">
    <span class="dot" class:paused class:failed aria-hidden="true"></span>
    {#if failed}{$t('logs_load_failed')}{:else if paused}{$t('logs_paused')}{:else}{$t('logs_live', { count: shown.length })}{/if}
    {#if notice}<span class="notice">· {notice}</span>{/if}
  </p>

  <!-- The scrollable log must be reachable to scroll it with the keyboard. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="viewport" bind:this={viewport} {onscroll} role="log" aria-label={$t('logs_title')} aria-live="off" tabindex="0">
    {#if !loaded}
      <p class="empty">{$t('logs_loading')}</p>
    {:else if !entries.length}
      <p class="empty">{$t('logs_empty')}</p>
    {:else if !shown.length}
      <p class="empty">{$t('logs_no_match')}</p>
    {:else}
      {#each shown as entry (entry.seq)}
        <div class="line" data-level={entry.level}>
          <time datetime={new Date(entry.at).toISOString()} title={time(entry.at)}>{clock(entry.at)}</time>
          <span class="badge">{$t(badges[entry.level])}</span>
          <span class="message">{entry.message}</span>
        </div>
      {/each}
    {/if}
  </div>
</section>

<style>
  section { display: grid; gap: 12px; min-width: 0; color: var(--color-text); font-family: var(--font-sans); }
  h2 { margin: 0; font-size: 26px; font-weight: 700; letter-spacing: -0.02em; }
  .description { max-width: 70ch; margin: 6px 0 0; font-size: 14px; line-height: 1.5; color: var(--color-muted); }

  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; margin-top: 8px; }
  .levels { display: flex; flex-wrap: wrap; gap: 8px; }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 36px; padding: 6px 14px; border: 1px solid var(--color-line); border-radius: var(--radius-full); background: transparent; color: var(--color-muted); font: inherit; font-size: 13px; font-weight: 600; cursor: pointer; transition: background-color var(--transition-fast), color var(--transition-fast); }
  button:hover:not(:disabled) { background: var(--color-raised); color: var(--color-text); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .chip[aria-pressed='true'] { border-color: transparent; background: var(--color-accent-soft); color: var(--color-accent); }
  .count { min-width: 20px; padding: 1px 7px; border-radius: var(--radius-full); background: var(--color-raised); color: var(--color-text); font-size: 12px; font-variant-numeric: tabular-nums; }
  .chip[data-level='warning'] .count { background: var(--color-warning-soft); color: var(--color-warning); }
  .chip[data-level='error'] .count { background: var(--color-danger-soft); color: var(--color-danger); }
  .search { display: flex; align-items: center; gap: 8px; flex: 1 1 220px; min-width: 0; padding: 0 12px; border: 1px solid var(--color-line); border-radius: var(--radius-full); background: var(--color-raised); color: var(--color-muted); }
  .search:focus-within { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .search input, .search input:focus { flex: 1; min-width: 0; min-height: 36px; padding: 0; border: 0; border-radius: 0; background: transparent; box-shadow: none; color: var(--color-text); font: inherit; font-size: 14px; outline: none; }
  .actions { display: flex; gap: 4px; }
  .icon { width: 40px; min-height: 40px; padding: 0; border: 0; }
  .icon[aria-pressed='true'] { background: var(--color-warning-soft); color: var(--color-warning); }

  .live { display: flex; align-items: center; gap: 8px; margin: 0; font-size: 13px; color: var(--color-muted); }
  .dot { width: 8px; height: 8px; border-radius: var(--radius-full); background: var(--color-success); box-shadow: 0 0 0 3px var(--color-success-soft); }
  .dot.paused { background: var(--color-warning); box-shadow: 0 0 0 3px var(--color-warning-soft); }
  .dot.failed { background: var(--color-danger); box-shadow: 0 0 0 3px var(--color-danger-soft); }

  .viewport { height: min(62vh, 640px); min-height: 280px; overflow: auto; padding: 8px 0; border-radius: var(--radius-md); background: var(--color-surface); font-family: var(--font-mono); font-size: 12.5px; line-height: 1.55; }
  .viewport:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .line { display: grid; grid-template-columns: auto auto 1fr; align-items: baseline; gap: 12px; padding: 3px 16px; border-left: 3px solid transparent; }
  .line:hover { background: var(--color-surface-hover); }
  .line[data-level='warning'] { border-left-color: var(--color-warning); }
  .line[data-level='error'] { border-left-color: var(--color-danger); background: color-mix(in srgb, var(--color-danger-soft) 45%, transparent); }
  time { color: var(--color-muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .badge { min-width: 52px; font-family: var(--font-sans); font-size: 11px; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; color: var(--color-muted); }
  .line[data-level='warning'] .badge { color: var(--color-warning); }
  .line[data-level='error'] .badge { color: var(--color-danger); }
  .message { min-width: 0; white-space: pre-wrap; overflow-wrap: anywhere; color: var(--color-text); }
  .empty { margin: 0; padding: 24px 16px; font-family: var(--font-sans); font-size: 14px; color: var(--color-muted); }

  @media (max-width: 640px) {
    h2 { font-size: 22px; }
    .search { order: 3; flex-basis: 100%; }
    .search input { font-size: 16px; }
    .line { grid-template-columns: auto 1fr; gap: 2px 10px; padding-inline: 12px; }
    .line .message { grid-column: 1 / -1; }
  }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
</style>
