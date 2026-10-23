<script lang="ts">
  import type { Snippet } from 'svelte';
  let { children, accent = false }: { children: Snippet; accent?: boolean } = $props();
</script>
<span class:accent>{@render children()}</span>
<style>span { display: inline-flex; align-items: center; gap: 6px; border-radius: var(--radius-full); padding: 4px 7px; background: var(--color-raised); color: var(--color-muted); font-size: 11px; font-weight: 650; letter-spacing: .02em; line-height: 1.2; white-space: nowrap; }.accent { background: var(--color-accent-soft); color: var(--color-accent); }</style>
