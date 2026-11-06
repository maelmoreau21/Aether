<script lang="ts">
  import type { ProviderPreference } from './types';
  let { value = $bindable(), label, id, description = '', suggestions = [] }: {
    value: ProviderPreference[]; label: string; id: string; description?: string; suggestions?: { id: string; name: string }[];
  } = $props();
  // An empty list means « every installed provider, in the default order ». The
  // list shown always includes installed providers so nothing has to be typed.
  const rows = $derived<ProviderPreference[]>(value.length
    ? [...value, ...suggestions.filter(item => !value.some(entry => entry.id === item.id)).map(item => ({ id: item.id, enabled: false }))]
    : suggestions.map(item => ({ id: item.id, enabled: true })));
  const nameOf = (provider: string) => suggestions.find(item => item.id === provider)?.name ?? provider;
  const installed = (provider: string) => suggestions.some(item => item.id === provider);
  const commit = (next: ProviderPreference[]) => { value = next.slice(0, 32); };
  function toggle(index: number, enabled: boolean) { commit(rows.map((row, i) => i === index ? { ...row, enabled } : { ...row })); }
  function move(index: number, offset: number) {
    const next = rows.map(row => ({ ...row }));
    [next[index], next[index + offset]] = [next[index + offset], next[index]];
    commit(next);
  }
</script>
<section aria-labelledby={`${id}-heading`}>
  <div class="heading"><h4 id={`${id}-heading`}>{label}</h4>{#if value.length}<button type="button" class="reset" onclick={() => value = []}>Ordre par défaut</button>{/if}</div>
  {#if description}<p>{description}</p>{/if}
  {#if rows.length}
    <ol>
      {#each rows as provider, index (provider.id)}
        <li class:off={!provider.enabled}>
          <span class="rank">{index + 1}</span>
          <label class="name"><input type="checkbox" checked={provider.enabled} onchange={event => toggle(index, event.currentTarget.checked)} /><span>{nameOf(provider.id)}{#if !installed(provider.id)}<small>Extension non installée</small>{/if}</span></label>
          <div class="row-actions">
            <button type="button" aria-label={`Monter ${nameOf(provider.id)}`} disabled={index === 0} onclick={() => move(index, -1)}>↑</button>
            <button type="button" aria-label={`Descendre ${nameOf(provider.id)}`} disabled={index === rows.length - 1} onclick={() => move(index, 1)}>↓</button>
            {#if !installed(provider.id)}<button type="button" class="remove" onclick={() => commit(rows.filter((_, i) => i !== index))}>Retirer</button>{/if}
          </div>
        </li>
      {/each}
    </ol>
  {:else}<p class="empty">Aucune extension de ce type n’est installée. Ajoutez-en dans Extensions.</p>{/if}
</section>
<style>
  section { min-width: 0; padding: 18px 0; border-bottom: 1px solid var(--color-line-subtle); font-family: var(--font); color: var(--tx); }
  section:last-child { border-bottom: 0; }
  .heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 36px; }
  h4 { margin: 0; font-size: 13px; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; color: var(--mu); }
  p { margin: 4px 0 12px; font-size: 13px; line-height: 1.6; color: var(--mu); }
  ol { display: grid; margin: 8px 0 0; padding: 0; list-style: none; }
  li { display: flex; align-items: center; gap: 12px; min-width: 0; min-height: 56px; padding: 6px 0; border-top: 1px solid var(--color-line-subtle); transition: opacity var(--t-fast) ease; }
  li.off .name span { color: var(--mu); }
  .rank { flex: none; width: 24px; color: var(--mu); font-size: 13px; font-weight: 700; font-variant-numeric: tabular-nums; text-align: center; }
  .name { display: flex; flex: 1; align-items: center; gap: 12px; min-width: 0; font-size: 15px; font-weight: 600; cursor: pointer; overflow-wrap: anywhere; }
  small { display: block; margin-top: 2px; color: var(--mu); font-size: 12px; font-weight: 400; }
  input[type='checkbox'] { flex: none; width: 20px; height: 20px; margin: 0; accent-color: var(--ac); }
  .row-actions { display: flex; gap: 6px; }
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 44px; padding: 0 14px; border: 1px solid var(--color-line); border-radius: var(--r-sm); background: transparent; color: var(--tx); font: inherit; font-size: 14px; font-weight: 600; cursor: pointer; transition: background-color var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  .row-actions button:not(.remove) { width: 44px; padding: 0; font-size: 17px; }
  button:hover:not(:disabled) { background: var(--color-surface-hover); }
  button:active:not(:disabled) { transform: scale(0.98); }
  button:disabled { opacity: 0.35; cursor: not-allowed; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .reset, .remove { border-color: transparent; color: var(--mu); font-size: 13px; }
  .empty { margin: 8px 0 0; }
  @media (max-width: 640px) { li { flex-wrap: wrap; } .row-actions { margin-left: 36px; } }
  @media (prefers-reduced-motion: reduce) { li, button { transition: none; } }
</style>
