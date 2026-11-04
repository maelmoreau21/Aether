<script lang="ts">
  import type { ProviderPreference } from './types';
  let { value = $bindable(), label, id, suggestions = [] }: {
    value: ProviderPreference[]; label: string; id: string; suggestions?: { id: string; name: string }[];
  } = $props();
  function move(index: number, offset: number) {
    const next = [...value];
    [next[index], next[index + offset]] = [next[index + offset], next[index]];
    value = next;
  }
</script>
<section aria-labelledby={`${id}-heading`}>
  <h4 id={`${id}-heading`}>{label}</h4>
  <p>Ordre de priorité enregistré pour le futur traitement. Ajouter un identifiant n’installe pas le fournisseur et ne lance aucune requête.</p>
  <ol>
    {#each value as provider, index (index)}
      <li>
        <label class="enabled"><input type="checkbox" bind:checked={provider.enabled} aria-label={`Activer ${label} ${index + 1}`} /><span>{index + 1}</span></label>
        <div class="provider-id"><label for={`${id}-${index}`}>Identifiant du fournisseur {index + 1}</label><input id={`${id}-${index}`} bind:value={provider.id} list={`${id}-suggestions`} required maxlength="100" pattern="[A-Za-z0-9._-]+" placeholder="Identifiant de l’extension" autocomplete="off" /></div>
        <div class="row-actions"><button type="button" aria-label={`Monter ${label} ${index + 1}`} disabled={index === 0} onclick={() => move(index, -1)}>↑</button><button type="button" aria-label={`Descendre ${label} ${index + 1}`} disabled={index === value.length - 1} onclick={() => move(index, 1)}>↓</button><button type="button" aria-label={`Retirer ${label} ${index + 1}`} onclick={() => value = value.filter((_, i) => i !== index)}>Retirer</button></div>
      </li>
    {/each}
  </ol>
  <datalist id={`${id}-suggestions`}>{#each suggestions as suggestion (suggestion.id)}<option value={suggestion.id}>{suggestion.name}</option>{/each}</datalist>
  {#if value.length === 0}<p class="empty">Aucun fournisseur sélectionné.</p>{/if}
  <button type="button" disabled={value.length >= 32} onclick={() => value = [...value, { id: '', enabled: true }]}>Ajouter un fournisseur — {label}</button>
</section>
<style>
  section { min-width: 0; margin-top: 20px; padding-top: 18px; border-top: 1px solid var(--line); font-family: var(--font); color: var(--tx); }
  h4 { margin: 0; font-size: 15px; font-weight: 700; }
  p { margin: 6px 0 12px; font-size: 13px; line-height: 1.6; color: var(--mu); }
  ol { display: grid; margin: 0 0 12px; padding: 0; list-style: none; }
  li { display: flex; align-items: flex-end; gap: 14px; min-width: 0; padding: 12px 0; border-bottom: 1px solid var(--line); }
  li:first-child { border-top: 1px solid var(--line); }
  .enabled { display: flex; align-items: center; gap: 10px; min-height: 46px; font-size: 14px; font-weight: 700; font-variant-numeric: tabular-nums; cursor: pointer; }
  .provider-id { flex: 1; min-width: 0; }
  .provider-id label { display: block; margin-bottom: 6px; font-size: 13px; font-weight: 700; color: var(--mu); }
  input:not([type='checkbox']) { box-sizing: border-box; width: 100%; min-width: 0; height: 46px; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background-color: var(--pn); color: var(--tx); font-family: var(--font-mono); font-size: 14px; box-shadow: none; transition: border-color var(--t-fast) ease, background-color var(--t-fast) ease; }
  input:not([type='checkbox']):hover { background-color: var(--pn-2); }
  input:not([type='checkbox']):focus { outline: none; border-color: var(--ac); background-color: var(--bg-3); }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  .row-actions { display: flex; gap: 6px; }
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 44px; padding: 0 16px; border: 0; border-radius: var(--r-md); background: var(--pn-2); color: var(--tx); font: inherit; font-size: 14px; font-weight: 700; cursor: pointer; transition: background-color var(--t-fast) ease, color var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  button:hover:not(:disabled) { background: var(--pn-3); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .row-actions button { width: 44px; height: 44px; padding: 0; border-radius: 14px; background: var(--pn); font-size: 17px; }
  .row-actions button:hover:not(:disabled) { background: var(--pn-2); }
  .row-actions button:last-child { width: auto; padding: 0 14px; border-radius: var(--r-md); background: transparent; color: var(--mu); font-size: 14px; }
  .row-actions button:last-child:hover:not(:disabled) { background: var(--pn); color: var(--tx); }
  .empty { margin: 0 0 12px; }
  @media (max-width: 640px) {
    li { flex-wrap: wrap; }
    .provider-id { flex-basis: calc(100% - 70px); }
    .row-actions { margin-left: auto; }
    input:not([type='checkbox']) { font-size: 16px; }
  }
  @media (prefers-reduced-motion: reduce) { button, input { transition: none; } }
</style>
