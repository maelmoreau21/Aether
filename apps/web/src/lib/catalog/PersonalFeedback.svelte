<script lang="ts">
  import { session, getSessionGeneration } from '../auth/session';
  let { workId }: { workId: string } = $props();
  let value = $state<{favorite:boolean;rating:number|null}>({favorite:false,rating:null});
  let busy = $state(true);
  let loaded = $state(false);
  let error = $state('');
  $effect(() => {
    const id = workId;
    const account = $session?.user.id;
    const abort = new AbortController();
    value = {favorite:false,rating:null}; busy = true; loaded = false; error = '';
    if (account) void fetch(`/api/v1/feedback/${encodeURIComponent(id)}`, { signal:AbortSignal.any([abort.signal,AbortSignal.timeout(10000)]), cache:'no-store' })
      .then(async r => { if (!r.ok) throw new Error('Vos préférences sont indisponibles.'); return await r.json(); })
      .then(v => { if (!abort.signal.aborted) { value = v; loaded = true; } })
      .catch(e => { if (!abort.signal.aborted) error = e.message; })
      .finally(() => { if (!abort.signal.aborted) busy = false; });
    return () => abort.abort();
  });
  async function save(next: typeof value) {
    const generation = getSessionGeneration(); const id = workId;
    busy = true; error = '';
    try {
      const r = await fetch(`/api/v1/feedback/${encodeURIComponent(id)}`, { method:'PUT', headers:{'Content-Type':'application/json','X-CSRF-Token':$session?.csrfToken ?? ''}, body:JSON.stringify(next), signal:AbortSignal.timeout(10000) });
      if (!r.ok) throw new Error('Préférence non enregistrée. Réessayez.');
      const result = await r.json();
      if (generation === getSessionGeneration() && id === workId) value = result;
    } catch (e) { if (generation === getSessionGeneration() && id === workId) error = e instanceof Error ? e.message : 'Enregistrement impossible.'; }
    finally { if (generation === getSessionGeneration() && id === workId) busy = false; }
  }
</script>
<div class="personal-feedback" aria-label="Vos préférences privées">
  <button disabled={busy || !loaded} aria-pressed={value.favorite} onclick={() => save({...value,favorite:!value.favorite})}>{value.favorite ? '♥ Favori' : '♡ Ajouter aux favoris'}</button>
  <label>Ma note privée
    <select aria-label="Ma note privée" disabled={busy || !loaded} value={value.rating ?? ''} onchange={e => save({...value,rating:e.currentTarget.value ? Number(e.currentTarget.value) : null})}>
      <option value="">Non noté</option>
      {#each [1,2,3,4,5] as rating (rating)}<option value={rating}>{rating} / 5</option>{/each}
    </select>
  </label>
  {#if error}<span role="alert">{error}</span>{/if}
</div>
<style>
.personal-feedback { display:flex; flex-wrap:wrap; gap:12px; align-items:center; margin-block:12px; }
button,select { background:var(--bg-surface);color:var(--text-primary);border:1px solid var(--border-subtle);border-radius:6px;padding:10px; }
button[aria-pressed="true"] { color:var(--accent-primary);border-color:var(--accent-primary); }
select { width:auto;min-width:90px; }
label { white-space:nowrap;display:flex;gap:8px;align-items:center; font-size:13px; } span { color:var(--status-error); }
</style>
