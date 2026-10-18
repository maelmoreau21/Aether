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
  <span>{progress?.watched ? 'Vu' : progress?.position ? `En cours · ${Math.floor(progress.position / 60)} min` : 'Non vu'}</span>
  <button disabled={saving} onclick={toggle}>{saving ? 'Enregistrement…' : progress?.watched ? 'Marquer non vu' : 'Marquer comme vu'}</button>
  {#if error}<small role="alert">{error}</small>{/if}
</div>
<style>
  .watch-status { display:flex; flex-wrap:wrap; align-items:center; gap:12px; margin:12px 0; font-size:12px; }
  button { min-height:36px; padding:8px 12px; border:1px solid #ffffff30; border-radius:6px; background:transparent; color:inherit; }
  small { flex-basis:100%; color:#efb4c4; }
</style>
