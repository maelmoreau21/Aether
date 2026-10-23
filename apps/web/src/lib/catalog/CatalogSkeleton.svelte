<script lang="ts">
  let { label = 'Chargement du catalogue…' }: { label?: string } = $props();
</script>

<div class="catalog-skeleton" role="status" aria-label={label}>
  <span class="sr-only">{label}</span>
  {#each Array.from({ length: 12 }, (_, index) => index) as index (index)}<div class="placeholder" aria-hidden="true"><div></div><span></span></div>{/each}
</div>

<style>
  .catalog-skeleton { display:grid;grid-template-columns:repeat(auto-fill,minmax(160px,1fr));gap:24px 18px;margin-block:24px; }
  .placeholder div { aspect-ratio:2/3;border-radius:var(--radius-md);background:var(--color-surface); }
  .placeholder span { display:block;height:14px;width:75%;margin-top:12px;background:var(--color-line);border-radius:4px; }
  .sr-only { position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%); }
  @media(max-width:640px) { .catalog-skeleton { grid-template-columns:repeat(2,1fr); } }
</style>
