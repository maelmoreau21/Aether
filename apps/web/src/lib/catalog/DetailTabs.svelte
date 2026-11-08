<script lang="ts" module>
  export type DetailTab = { id: string; label: string };
</script>

<script lang="ts">
  // Onglets segmentés de la fiche (Épisodes / Détails / Distribution) :
  // remplacent le long défilement sous l'en-tête. Motif ARIA « tabs » avec
  // activation automatique au clavier (flèches, Début, Fin).
  let { tabs, active = $bindable(), label, id }: { tabs: DetailTab[]; active: string; label: string; id: string } = $props();

  let buttons = $state<HTMLButtonElement[]>([]);

  function select(index: number) {
    const tab = tabs[index];
    if (!tab) return;
    active = tab.id;
    buttons[index]?.focus();
  }

  function onKeydown(event: KeyboardEvent, index: number) {
    const last = tabs.length - 1;
    const target = {
      ArrowRight: index === last ? 0 : index + 1,
      ArrowLeft: index === 0 ? last : index - 1,
      Home: 0,
      End: last
    }[event.key];
    if (target === undefined) return;
    event.preventDefault();
    select(target);
  }
</script>

<div class="tabs-scroll">
  <div class="tabs" role="tablist" aria-label={label}>
    {#each tabs as tab, index (tab.id)}
      <button
        bind:this={buttons[index]}
        type="button"
        role="tab"
        id="{id}-tab-{tab.id}"
        aria-controls="{id}-panel-{tab.id}"
        aria-selected={active === tab.id}
        tabindex={active === tab.id ? 0 : -1}
        onclick={() => (active = tab.id)}
        onkeydown={event => onKeydown(event, index)}
      >{tab.label}</button>
    {/each}
  </div>
</div>

<style>
  .tabs-scroll {
    max-width: 100%;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .tabs-scroll::-webkit-scrollbar { display: none; }

  .tabs {
    display: inline-flex;
    gap: 4px;
    padding: 4px;
    border: 1px solid var(--line);
    border-radius: var(--r-pill);
    background: var(--pn);
  }

  [role='tab'] {
    flex: 0 0 auto;
    min-height: 44px;
    padding: 0 20px;
    border: 0;
    border-radius: var(--r-pill);
    background: transparent;
    color: var(--mu);
    font: inherit;
    font-size: 14px;
    font-weight: 800;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color var(--t-fast) var(--ease), color var(--t-fast) var(--ease);
  }

  [role='tab']:hover { color: var(--tx); }

  [role='tab'][aria-selected='true'] {
    background: var(--btn);
    color: var(--btn-tx);
  }

  [role='tab']:focus-visible {
    outline: 2px solid var(--ac);
    outline-offset: 2px;
  }

  @media (max-width: 640px) {
    [role='tab'] { padding: 0 14px; font-size: 13px; }
  }
</style>
