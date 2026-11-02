<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import { history, loadHistory, markWatched } from './watch-state';
  import Icon from '../ui/Icon.svelte';

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
      error = e instanceof Error ? e.message : 'Statut non enregistré.';
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
    aria-label={saving ? 'Enregistrement…' : isWatched ? 'Marquer non vu' : 'Marquer comme vu'}
    title={saving ? 'Enregistrement…' : isWatched ? 'Marquer non vu' : 'Marquer comme vu'}
    disabled={saving}
    onclick={toggle}
  >
    <span class="check-circle" aria-hidden="true">
      <Icon name="check" size={16} />
    </span>
    {#if !compact}
      <span class="watch-label">
        {#if saving}
          Enregistrement…
        {:else if isWatched}
          Marquer non vu
        {:else}
          Marquer comme vu
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
    gap: 8px;
    min-height: 44px;
    padding: 0 16px 0 12px;
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-pill);
    background: var(--color-glass-raised);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    color: var(--color-text);
    font-size: 13.5px;
    font-weight: 650;
    cursor: pointer;
    transition: all var(--transition-fast);
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
    border: 1.5px solid var(--color-line-strong);
    background: rgba(255, 255, 255, 0.04);
    color: var(--color-muted);
    transition: all var(--transition-fast);
  }

  .watch-toggle-btn:hover:not(:disabled) {
    background: var(--color-raised-hover);
    border-color: var(--color-text);
    color: var(--color-text-bright);
    transform: translateY(-1px);
  }

  .watch-toggle-btn:hover:not(:disabled) .check-circle {
    border-color: var(--color-text-bright);
    color: var(--color-text-bright);
  }

  .watch-toggle-btn.is-watched {
    background: rgba(16, 185, 129, 0.16);
    border-color: rgba(16, 185, 129, 0.5);
    color: #34D399;
  }

  .watch-toggle-btn.is-watched .check-circle {
    background: #10B981;
    border-color: #10B981;
    color: #ffffff;
    box-shadow: 0 0 10px rgba(16, 185, 129, 0.45);
  }

  .watch-toggle-btn:disabled {
    opacity: 0.6;
    cursor: wait;
  }

  small {
    flex-basis: 100%;
    color: var(--color-danger);
    font-size: 12px;
  }
</style>
