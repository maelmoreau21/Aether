<script lang="ts">
  import { session, getSessionGeneration } from '../auth/session';
  import { t, fmt } from '../i18n';
  type Feedback = {favorite:boolean;rating:number|null};
  let { workId, compact = false }: { workId:string; compact?: boolean } = $props();
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
      .then(async response=>{if(!response.ok)throw new Error($t('your_preferences_are_unavailable'));return await response.json();})
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
          if(!response.ok)throw new Error($t('preference_not_saved_try_again'));
          const result:Feedback=await response.json();
          if(current!==epoch||generation!==getSessionGeneration())return;
          confirmed=result;if(!pending)value=result;
        } catch(cause) {
          if(current!==epoch||generation!==getSessionGeneration())return;
          if(!pending){value=confirmed;error=cause instanceof Error?cause.message:$t('saving_failed');}
        }
      }
    } finally { if(current===epoch){processing=false;saving=false;} }
  }
</script>
<div class="personal-feedback" class:compact aria-label={$t('your_private_preferences')} aria-busy={saving}>
  <button
    type="button"
    class="favorite-btn"
    class:compact
    disabled={!loaded}
    aria-label={value.favorite ? $t('remove_from_favourites') : $t('add_to_favourites')}
    title={value.favorite ? $t('remove_from_favourites') : $t('add_to_favourites')}
    aria-pressed={value.favorite}
    onclick={() => save({ ...value, favorite: !value.favorite })}
  >
    <svg class="heart-glyph" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true">
      <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78Z" stroke-width="2" stroke-linejoin="round" />
    </svg>
    {#if !compact}<span class="fav-label">{value.favorite ? 'Favori' : 'Favoris'}</span>{/if}
  </button>

  <div class="rating-pill" class:rated={Boolean(value.rating)}>
    <div class="stars" role="group" aria-label={$t('my_private_rating_out_of_5')} onpointerleave={() => (preview = null)}>
      {#each [1, 2, 3, 4, 5] as star (star)}
        <span class="star">
          <span class="outline" aria-hidden="true">★</span>
          <span class="fill" class:full={shownRating >= star} class:half-filled={shownRating === star - 0.5} aria-hidden="true">★</span>
          {#each [star - 0.5, star] as rating (rating)}
            <button
              type="button"
              class="half"
              disabled={!loaded}
              aria-label={$t('rating_out_of_five', { value: $fmt.number(rating) })}
              title={$t('rating_out_of_five_clear', { value: $fmt.number(rating) })}
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
    <!-- The compact pill keeps the value's room even with no rating: rating or previewing a
         star never changes its width, so the actions around it never reflow under the pointer
         (in a bottom-aligned hero, a reflow would move the stars away from it). -->
    {#if shownRating}
      <span class="rating-value">{$fmt.number(shownRating)}</span>
    {:else if compact}
      <span class="rating-value" aria-hidden="true"></span>
    {:else}
      <span class="rating-placeholder">{$t('rate')}</span>
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
    color: var(--tx);
    border: 0;
    background: transparent;
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.45;
    cursor: wait;
  }

  button:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 3px;
    border-radius: var(--r-xs);
  }

  .favorite-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px 0 12px;
    border-radius: 14px;
    background: var(--pn);
    border: 1px solid transparent;
    color: var(--tx);
    font-size: 14px;
    font-weight: 700;
    transition: background-color var(--t-fast) var(--ease), color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }

  .favorite-btn:focus-visible {
    border-radius: 14px;
  }

  .favorite-btn:hover:not(:disabled) {
    background: var(--pn-2);
  }

  .favorite-btn:active:not(:disabled) {
    transform: scale(0.98);
  }

  .heart-glyph {
    flex: none;
    fill: transparent;
    stroke: currentColor;
    color: var(--tx);
    transition: fill var(--t-fast) var(--ease), color var(--t-fast) var(--ease), transform var(--t-med) var(--ease);
  }

  .favorite-btn[aria-pressed="true"] .heart-glyph {
    color: var(--ac);
    fill: var(--ac);
    animation: heart-pop var(--t-slow) var(--ease);
  }

  @keyframes heart-pop {
    0% { transform: scale(1); }
    40% { transform: scale(1.28); }
    100% { transform: scale(1); }
  }

  .favorite-btn.compact {
    width: 44px;
    padding: 0;
    justify-content: center;
  }

  .personal-feedback.compact { gap: 10px; flex-wrap: nowrap; }
  .personal-feedback.compact .rating-pill { padding: 0 12px 0 8px; }
  .personal-feedback.compact .star { width: 22px; font-size: 19px; }

  .rating-pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 14px 0 8px;
    border-radius: 14px;
    background: var(--pn);
  }

  .stars {
    display: flex;
    align-items: center;
  }

  .star {
    position: relative;
    display: flex;
    width: 26px;
    height: 40px;
    font-size: 21px;
    line-height: 40px;
  }

  .outline,
  .fill {
    position: absolute;
    inset: 0;
    pointer-events: none;
    text-align: center;
  }

  .outline {
    color: var(--line-2);
  }

  .fill {
    color: var(--ac);
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
    min-height: 0;
    padding: 0;
    background: transparent;
  }

  .rating-pill.rated { background: var(--ac-soft); }

  .rating-value {
    font-size: 13px;
    font-weight: 700;
    color: var(--tx);
    font-variant-numeric: tabular-nums;
    min-width: 32px;
  }

  .rating-placeholder {
    font-size: 13px;
    font-weight: 600;
    color: var(--mu);
  }

  .feedback-error {
    flex-basis: 100%;
    font-size: 13px;
    color: var(--err);
  }
</style>
