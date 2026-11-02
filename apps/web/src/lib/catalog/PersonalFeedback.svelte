<script lang="ts">
  import { session, getSessionGeneration } from '../auth/session';
  type Feedback = {favorite:boolean;rating:number|null};
  let { workId }: { workId:string } = $props();
  let value = $state<Feedback>({favorite:false,rating:null});
  let loaded = $state(false); let saving = $state(false); let error = $state(''); let preview = $state<number|null>(null);
  let confirmed:Feedback = {favorite:false,rating:null};
  let pending:Feedback|null = null;
  let epoch = 0; let processing = false;
  const shownRating = $derived(preview ?? value.rating ?? 0);
  $effect(() => {
    const id=workId; const account=$session?.user.id; const abort=new AbortController();
    const current=++epoch; pending=null;processing=false;value={favorite:false,rating:null};confirmed={favorite:false,rating:null};loaded=false;saving=false;error='';preview=null;
    if(account)void fetch(`/api/v1/feedback/${encodeURIComponent(id)}`,{signal:AbortSignal.any([abort.signal,AbortSignal.timeout(10000)]),cache:'no-store'})
      .then(async response=>{if(!response.ok)throw new Error('Vos préférences sont indisponibles.');return await response.json();})
      .then(result=>{if(!abort.signal.aborted&&current===epoch){value=result;confirmed=result;loaded=true;}})
      .catch(cause=>{if(!abort.signal.aborted&&current===epoch)error=cause.message;});
    return()=>abort.abort();
  });
  async function save(next:Feedback) {
    if(!loaded)return;
    value=next;pending=next;error='';
    if(processing)return;
    const current=epoch; const generation=getSessionGeneration(); const id=workId; const csrf=$session?.csrfToken??'';
    processing=true;saving=true;
    try {
      while(pending&&current===epoch&&generation===getSessionGeneration()){
        const requested=pending;pending=null;
        try {
          const response=await fetch(`/api/v1/feedback/${encodeURIComponent(id)}`,{method:'PUT',headers:{'Content-Type':'application/json','X-CSRF-Token':csrf},body:JSON.stringify(requested),signal:AbortSignal.timeout(10000)});
          if(!response.ok)throw new Error('Préférence non enregistrée. Réessayez.');
          const result:Feedback=await response.json();
          if(current!==epoch||generation!==getSessionGeneration())return;
          confirmed=result;if(!pending)value=result;
        } catch(cause) {
          if(current!==epoch||generation!==getSessionGeneration())return;
          if(!pending){value=confirmed;error=cause instanceof Error?cause.message:'Enregistrement impossible.';}
        }
      }
    } finally { if(current===epoch){processing=false;saving=false;} }
  }
</script>
<div class="personal-feedback" aria-label="Vos préférences privées" aria-busy={saving}>
  <button
    type="button"
    class="favorite-btn"
    disabled={!loaded}
    aria-label={value.favorite ? 'Retirer des favoris' : 'Ajouter aux favoris'}
    title={value.favorite ? 'Retirer des favoris' : 'Ajouter aux favoris'}
    aria-pressed={value.favorite}
    onclick={() => save({ ...value, favorite: !value.favorite })}
  >
    <span class="heart-glyph" aria-hidden="true">{value.favorite ? '♥' : '♡'}</span>
    <span class="fav-label">{value.favorite ? 'Favori' : 'Favoris'}</span>
  </button>

  <div class="rating-pill">
    <div class="stars" role="group" aria-label="Ma note privée sur 5, par demi-étoile" onpointerleave={() => (preview = null)}>
      {#each [1, 2, 3, 4, 5] as star (star)}
        <span class="star">
          <span class="outline" aria-hidden="true">★</span>
          <span class="fill" class:full={shownRating >= star} class:half-filled={shownRating === star - 0.5} aria-hidden="true">★</span>
          {#each [star - 0.5, star] as rating (rating)}
            <button
              type="button"
              class="half"
              disabled={!loaded}
              aria-label={`${rating.toLocaleString('fr-FR')} sur 5`}
              title={`${rating.toLocaleString('fr-FR')} sur 5 — recliquer pour effacer`}
              aria-pressed={value.rating === rating}
              onpointerenter={() => (preview = rating)}
              onfocus={() => (preview = rating)}
              onblur={() => (preview = null)}
              onclick={() => {
                preview = null;
                void save({ ...value, rating: value.rating === rating ? null : rating });
              }}
            ></button>
          {/each}
        </span>
      {/each}
    </div>
    {#if shownRating}
      <span class="rating-value">{shownRating.toLocaleString('fr-FR')}/5</span>
    {:else}
      <span class="rating-placeholder">Noter</span>
    {/if}
  </div>
  {#if error}<span class="feedback-error" role="alert">{error}</span>{/if}
</div>

<style>
  .personal-feedback {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: center;
  }

  button {
    color: var(--color-text);
    border: 0;
    background: transparent;
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.5;
    cursor: wait;
  }

  button:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
    border-radius: var(--radius-xs);
  }

  .favorite-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px;
    border-radius: var(--radius-pill);
    background: var(--color-glass-raised);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    border: 1px solid var(--color-line-strong);
    color: var(--color-text-bright);
    font-size: 13.5px;
    font-weight: 650;
    transition: all var(--transition-fast);
  }

  .favorite-btn:hover:not(:disabled) {
    background: var(--color-raised-hover);
    border-color: var(--color-text);
    transform: translateY(-1px);
  }

  .favorite-btn[aria-pressed="true"] {
    background: rgba(239, 68, 68, 0.16);
    border-color: rgba(239, 68, 68, 0.45);
    color: #F87171;
  }

  .heart-glyph {
    font-size: 18px;
    line-height: 1;
  }

  .rating-pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 14px;
    border-radius: var(--radius-pill);
    background: var(--color-glass-raised);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    border: 1px solid var(--color-line-strong);
  }

  .stars {
    display: flex;
    align-items: center;
  }

  .star {
    position: relative;
    display: flex;
    width: 26px;
    height: 36px;
    font-size: 22px;
    line-height: 36px;
  }

  .outline,
  .fill {
    position: absolute;
    inset: 0;
    pointer-events: none;
    text-align: center;
  }

  .outline {
    color: var(--color-muted);
    opacity: 0.45;
  }

  .fill {
    color: #F59E0B;
    clip-path: inset(0 100% 0 0);
  }

  .fill.full {
    clip-path: inset(0);
  }

  .fill.half-filled {
    clip-path: inset(0 50% 0 0);
  }

  .half {
    z-index: 1;
    width: 50%;
    height: 100%;
    padding: 0;
    background: transparent;
  }

  .rating-value {
    font-size: 12.5px;
    font-weight: 700;
    color: #FBBF24;
    min-width: 32px;
  }

  .rating-placeholder {
    font-size: 12px;
    font-weight: 600;
    color: var(--color-muted);
  }

  .feedback-error {
    flex-basis: 100%;
    font-size: 12px;
    color: var(--color-danger);
  }
</style>
