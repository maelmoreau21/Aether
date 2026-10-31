<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import { history, loadHistory, markWatched } from './watch-state';
  let { workId }: { workId: string } = $props();
  let error = $state('');
  let saving = $state(false);
  const progress = $derived($history.find(item => item.workId === workId));
  onMount(() => { void loadHistory().catch(e => error = String(e.message)); });
  $effect(() => { if ($session) void loadHistory().catch(e => error = String(e.message)); });
  async function toggle() {
    saving = true; error = '';
    try { await markWatched(workId, !progress?.watched); }
    catch (e) { error = e instanceof Error ? e.message : 'Statut non enregistré.'; }
    finally { saving = false; }
  }
</script>
<div class="watch-status">
  <span class="status-badge">{progress?.watched ? 'Vu' : progress?.position ? `En cours · ${Math.floor(progress.position / 60)} min` : 'Non vu'}</span>
  <button
    type="button"
    class="watch-toggle-btn"
    class:is-watched={progress?.watched}
    disabled={saving}
    onclick={toggle}
  >
    {saving ? 'Enregistrement…' : progress?.watched ? 'Marquer non vu' : 'Marquer comme vu'}
  </button>
  {#if error}<small role="alert">{error}</small>{/if}
</div>
<style>
  .watch-status {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
  }
  .status-badge {
    font-size: 13px;
    color: var(--color-muted);
    font-weight: 500;
  }
  .watch-toggle-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px;
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-pill);
    background: var(--color-glass-raised);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    color: var(--color-text);
    font-size: 13px;
    font-weight: 600;
    transition: all var(--transition-fast);
  }
  .watch-toggle-btn:hover:not(:disabled) {
    background: var(--color-raised-hover);
    border-color: var(--color-text);
    color: var(--color-text-bright);
    transform: translateY(-1px);
  }
  .watch-toggle-btn.is-watched {
    background: rgba(16, 185, 129, 0.14);
    border-color: rgba(16, 185, 129, 0.4);
    color: #34D399;
  }
  small {
    flex-basis: 100%;
    color: var(--status-error);
    font-size: 12px;
  }
</style>
