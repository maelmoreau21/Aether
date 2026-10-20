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
  <button class="heart" disabled={busy || !loaded} aria-label={value.favorite ? 'Retirer des favoris' : 'Ajouter aux favoris'} title={value.favorite ? 'Retirer des favoris' : 'Ajouter aux favoris'} aria-pressed={value.favorite} onclick={() => save({...value,favorite:!value.favorite})}>{value.favorite ? '♥' : '♡'}</button>
  <div class="stars" role="group" aria-label="Ma note privée sur 5, par demi-étoile">
    {#each [1,2,3,4,5] as star (star)}
      <span class="star">
        <span class="outline" aria-hidden="true">★</span>
        <span class="fill" class:full={(value.rating ?? 0) >= star} class:half-filled={value.rating === star - 0.5} aria-hidden="true">★</span>
        {#each [star - 0.5, star] as rating (rating)}
          <button class="half" disabled={busy || !loaded} aria-label={`${rating.toLocaleString('fr-FR')} sur 5`} title={`${rating.toLocaleString('fr-FR')} sur 5 — recliquer pour effacer`} aria-pressed={value.rating === rating} onclick={() => save({...value,rating:value.rating === rating ? null : rating})}></button>
        {/each}
      </span>
    {/each}
  </div>
  {#if error}<span role="alert">{error}</span>{/if}
</div>
<style>
.personal-feedback { display:flex; flex-wrap:wrap; gap:16px; align-items:center; margin-block:12px; }
button { color:var(--text-secondary); border:0; background:transparent; cursor:pointer; }
button:disabled { opacity:.5; cursor:wait; }
button:focus-visible { outline:2px solid var(--accent-primary); outline-offset:2px; border-radius:3px; }
.heart { font-size:32px; width:44px; height:44px; padding:0; }
.heart[aria-pressed="true"] { color:#ed718b; }
.stars { display:flex; }
.star { position:relative; display:flex; width:32px; height:44px; font-size:30px; line-height:44px; }
.outline,.fill { position:absolute; inset:0; pointer-events:none; }
.outline { color:var(--text-secondary); opacity:.5; }
.fill { width:0; color:#f5c45b; overflow:hidden; white-space:nowrap; }
.fill.full { width:100%; }
.fill.half-filled { width:50%; }
.half { z-index:1; width:50%; height:100%; padding:0; }
.half:hover { background:rgba(245,196,91,.15); }
span[role="alert"] { color:var(--status-error); }
</style>
