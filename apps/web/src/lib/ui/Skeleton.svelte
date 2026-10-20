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
  }
</style>
