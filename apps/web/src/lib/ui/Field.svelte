<script lang="ts">
  import type { Snippet } from 'svelte';
  let { label, for: id, children }: { label: string; for: string; children: Snippet } = $props();
</script>
<div class="field"><label for={id}>{label}</label>{@render children()}</div>
<style>.field { display: grid; gap: 9px; min-width: 0; } label { color: var(--color-muted); font-size: 11px; font-weight: 500; letter-spacing: .02em; }</style>
