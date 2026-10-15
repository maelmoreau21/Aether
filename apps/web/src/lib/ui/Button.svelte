<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  let { children, variant = 'solid', class: className = '', ...rest }: HTMLButtonAttributes & { children: Snippet; variant?: 'solid' | 'quiet' } = $props();
</script>
<button class="ui-button {variant} {className}" {...rest}>{@render children()}</button>
<style>
  .ui-button { display: inline-flex; align-items: center; justify-content: center; gap: 10px; min-height: 44px; padding: 12px 20px; border: 0; border-radius: var(--radius-sm); font-size: 13px; font-weight: 650; line-height: 1.2; white-space: nowrap; }
  .solid { background: var(--color-text); color: #15131b; }
  .solid:hover:not(:disabled) { background: white; transform: translateY(-1px); }
  .quiet { background: #ffffff09; color: var(--color-text); }
  .quiet:hover:not(:disabled) { background: #ffffff15; }
  .ui-button:active:not(:disabled) { transform: translateY(0); }
</style>
