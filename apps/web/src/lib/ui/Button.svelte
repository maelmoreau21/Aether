<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  let { children, variant = 'solid', class: className = '', ...rest }: HTMLButtonAttributes & { children: Snippet; variant?: 'solid' | 'quiet' } = $props();
</script>
<button class="ui-button {variant} {className}" {...rest}>{@render children()}</button>
<style>
  .ui-button { display: inline-flex; align-items: center; justify-content: center; gap: 10px; min-height: 48px; padding: 0 22px; border: 0; border-radius: var(--r-md); font: inherit; font-size: 15px; font-weight: 700; line-height: 1.2; white-space: nowrap; cursor: pointer; transition: background-color var(--t-fast) ease, opacity var(--t-fast) ease, transform var(--t-fast) var(--ease); }
  .solid { background: var(--btn); color: var(--btn-tx); }
  .solid:hover:not(:disabled) { opacity: 0.9; }
  .quiet { background: var(--pn-2); color: var(--tx); }
  .quiet:hover:not(:disabled) { background: var(--pn-3); }
  .ui-button:active:not(:disabled) { transform: scale(0.98); }
  .ui-button:disabled { opacity: 0.45; cursor: default; }
  .ui-button:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
</style>
