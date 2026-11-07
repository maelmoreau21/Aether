<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import { history, loadHistory, markWatched } from './watch-state';
  import Icon from '../ui/Icon.svelte';
  import { t } from '../i18n';

  let { workId, duration, compact = false }: { workId: string; duration?: number; compact?: boolean } = $props();
  let error = $state('');
  let saving = $state(false);
  const progress = $derived($history.find(item => item.workId === workId));
  const isWatched = $derived(Boolean(progress?.watched));

  onMount(() => { void loadHistory().catch(e => error = String(e.message)); });
  $effect(() => { if ($session) void loadHistory().catch(e => error = String(e.message)); });

  async function toggle() {
    saving = true;
    error = '';
    try {
      await markWatched(workId, !isWatched, duration);
    } catch (e) {
      error = e instanceof Error ? e.message : $t('status_not_saved');
    } finally {
      saving = false;
    }
  }
</script>

<div class="watch-status">
  <button
    type="button"
    class="watch-toggle-btn"
    class:is-watched={isWatched}
    class:compact
    aria-pressed={isWatched}
    aria-label={saving ? 'Enregistrement…' : isWatched ? $t('mark_as_unwatched') : $t('mark_as_watched')}
    title={saving ? 'Enregistrement…' : isWatched ? $t('mark_as_unwatched') : $t('mark_as_watched')}
    disabled={saving}
    onclick={toggle}
  >
    <span class="check-circle" aria-hidden="true">
      <Icon name="check" size={16} />
    </span>
    {#if !compact}
      <span class="watch-label">
        {#if saving}
          {$t('saving')}
        {:else if isWatched}
          {$t('mark_as_unwatched')}
        {:else}
          {$t('mark_as_watched')}
        {/if}
      </span>
    {/if}
  </button>
  {#if error}<small role="alert">{error}</small>{/if}
</div>

<style>
  .watch-status {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .watch-toggle-btn {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 0 16px 0 10px;
    border: 1px solid transparent;
    border-radius: 14px;
    background: var(--pn);
    color: var(--tx);
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease), color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }

  .watch-toggle-btn.compact {
    width: 44px;
    padding: 0;
    justify-content: center;
  }

  .check-circle {
    display: inline-grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 1.5px solid var(--line-2);
    color: var(--mu);
    transition: background-color var(--t-fast) var(--ease), border-color var(--t-fast) var(--ease), color var(--t-fast) var(--ease);
  }

  .watch-toggle-btn:hover:not(:disabled) {
    background: var(--pn-2);
  }

  .watch-toggle-btn:active:not(:disabled) {
    transform: scale(0.98);
  }

  .watch-toggle-btn:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
  }

  .watch-toggle-btn:hover:not(:disabled) .check-circle {
    border-color: var(--tx);
    color: var(--tx);
  }

  .watch-toggle-btn.is-watched .check-circle,
  .watch-toggle-btn.is-watched:hover:not(:disabled) .check-circle {
    background: var(--ac);
    border-color: var(--ac);
    color: var(--on-ac);
  }

  .watch-toggle-btn:disabled {
    opacity: 0.45;
    cursor: wait;
  }

  small {
    flex-basis: 100%;
    color: var(--err);
    font-size: 13px;
  }
</style>
