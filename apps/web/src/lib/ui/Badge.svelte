<script lang="ts">
  import type { Snippet } from 'svelte';
  let { children, accent = false }: { children: Snippet; accent?: boolean } = $props();
</script>
<span class:accent>{@render children()}</span>
<style>
  span { display: inline-flex; align-items: center; gap: 6px; height: 24px; padding: 0 9px; border-radius: var(--r-xs); background: var(--pn-2); color: var(--tx); font-size: 12px; font-weight: 700; line-height: 1; white-space: nowrap; }
  .accent { background: var(--ac-soft); color: var(--ac); }
</style>
