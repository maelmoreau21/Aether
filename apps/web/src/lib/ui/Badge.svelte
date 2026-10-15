<script lang="ts">
  import type { Snippet } from 'svelte';
  let { children, accent = false }: { children: Snippet; accent?: boolean } = $props();
</script>
<span class:accent>{@render children()}</span>
<style>span { display: inline-flex; align-items: center; gap: 6px; border-radius: 4px; padding: 4px 7px; background: #ffffff0b; color: #c7c3d1; font-size: 9px; font-weight: 650; letter-spacing: .1em; line-height: 1.2; white-space: nowrap; }.accent { background: var(--color-accent-soft); color: var(--color-accent); }</style>
