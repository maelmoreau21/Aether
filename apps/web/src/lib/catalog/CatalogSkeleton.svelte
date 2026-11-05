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
    grid-template-columns: repeat(auto-fill, minmax(var(--poster-width, clamp(150px, 12vw, 190px)), 1fr));
    gap: 28px 16px;
    margin-block: 24px;
  }

  .skeleton {
    background: var(--pn-2);
    animation: halo-skeleton-pulse 1.6s var(--ease) infinite;
  }

  .poster-skeleton {
    aspect-ratio: 2 / 3;
    border-radius: var(--r-md);
  }

  .title-skeleton {
    display: block;
    height: 14px;
    width: 78%;
    margin-top: 12px;
    border-radius: var(--r-xs);
  }

  .meta-skeleton {
    display: block;
    height: 11px;
    width: 45%;
    margin-top: 6px;
    border-radius: var(--r-xs);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  @keyframes halo-skeleton-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.55; }
  }

  @media (prefers-reduced-motion: reduce) {
    .skeleton { animation: none; }
  }

  @media (max-width: 640px) {
    .catalog-skeleton {
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 18px 10px;
    }
  }
</style>
