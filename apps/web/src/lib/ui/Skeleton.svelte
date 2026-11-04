<script lang="ts">
  let {
    width = '100%',
    height = '20px',
    radius = 'var(--radius-sm)',
    class: className = ''
  }: {
    width?: string;
    height?: string;
    radius?: string;
    class?: string;
  } = $props();
</script>

<div
  class="skeleton {className}"
  style:width={width}
  style:height={height}
  style:border-radius={radius}
  aria-hidden="true"
></div>

<style>
  div {
    display: block;
    flex-shrink: 0;
    background: var(--pn-2);
    animation: halo-skeleton-pulse 1.6s var(--ease) infinite;
  }

  @keyframes halo-skeleton-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.55; }
  }

  @media (prefers-reduced-motion: reduce) {
    div { animation: none; }
  }
</style>
