<script lang="ts">
  import type { Snippet } from 'svelte';
  let { label, for: id, children }: { label: string; for: string; children: Snippet } = $props();
</script>
<div class="field"><label for={id}>{label}</label>{@render children()}</div>
<style>
  .field { display: grid; gap: 8px; min-width: 0; }
  label { color: var(--tx); font-size: 14px; font-weight: 700; }
</style>
