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
  section { border-top:1px solid var(--border-subtle);padding-top:18px;margin-top:20px;min-width:0; }
  h4 { margin:0;font-size:15px; } p { font-size:12px;line-height:1.6;color:var(--text-secondary); }
  ol { padding:0;list-style:none;display:grid;gap:10px; } li { display:flex;align-items:center;gap:14px;padding:12px;background:var(--bg-main);border:1px solid var(--border-subtle);border-radius:var(--radius-md);min-width:0; }
  .enabled { display:flex;align-items:center;gap:8px; } .provider-id { flex:1;min-width:0; } .provider-id label { display:block;font-size:12px;margin-bottom:6px;color:var(--text-secondary); }
  input:not([type=checkbox]) { width:100%;box-sizing:border-box;min-width:0;padding:10px;border:1px solid var(--border-subtle);border-radius:var(--radius-md);background:var(--bg-surface);color:var(--text-primary);font:inherit; }
  .row-actions { display:flex;gap:6px; } button { min-height:40px;padding:8px 12px;background:var(--bg-elevated);color:var(--text-primary);border:1px solid var(--border-subtle);border-radius:var(--radius-md);cursor:pointer;font:inherit;font-size:12px; } button:disabled { opacity:.4;cursor:default; }
  input[type=checkbox] { accent-color:var(--color-play);width:18px;height:18px; }
  @media(max-width:600px) { li { flex-wrap:wrap; }.provider-id { flex-basis:calc(100% - 70px); }.row-actions { margin-left:auto; } }
</style>
