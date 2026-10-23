<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  let { children, variant = 'solid', class: className = '', ...rest }: HTMLButtonAttributes & { children: Snippet; variant?: 'solid' | 'quiet' } = $props();
</script>
<button class="ui-button {variant} {className}" {...rest}>{@render children()}</button>
<style>
  .ui-button { display: inline-flex; align-items: center; justify-content: center; gap: 10px; min-height: 44px; padding: 12px 20px; border: 0; border-radius: var(--radius-md); font-size: 13px; font-weight: 650; line-height: 1.2; white-space: nowrap; }
  .solid { background: var(--color-play); color: var(--color-on-play); }
  .solid:hover:not(:disabled) { background: var(--color-play-hover); transform: translateY(-1px); }
  .quiet { background: var(--color-surface); color: var(--color-text); }
  .quiet:hover:not(:disabled) { background: var(--color-raised); }
  .ui-button:active:not(:disabled) { transform: translateY(0); }
</style>
