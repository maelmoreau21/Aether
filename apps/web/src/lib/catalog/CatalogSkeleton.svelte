<script lang="ts">
  let { label = 'Chargement du catalogue…' }: { label?: string } = $props();
</script>

<div class="catalog-skeleton" role="status" aria-label={label}>
  <span class="sr-only">{label}</span>
  {#each Array.from({ length: 12 }, (_, index) => index) as index (index)}
    <div class="placeholder" aria-hidden="true">
      <div class="skeleton poster-skeleton"></div>
      <span class="skeleton title-skeleton"></span>
      <span class="skeleton meta-skeleton"></span>
    </div>
  {/each}
</div>

<style>
  .catalog-skeleton {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 24px 18px;
    margin-block: 24px;
  }

  .poster-skeleton {
    aspect-ratio: 2 / 3;
    border-radius: var(--radius-md);
    border: 1px solid var(--color-line);
  }

  .title-skeleton {
    display: block;
    height: 14px;
    width: 78%;
    margin-top: 12px;
    border-radius: 4px;
  }

  .meta-skeleton {
    display: block;
    height: 11px;
    width: 45%;
    margin-top: 6px;
    border-radius: 4px;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  @media (max-width: 640px) {
    .catalog-skeleton {
      grid-template-columns: repeat(2, 1fr);
    }
  }
</style>
