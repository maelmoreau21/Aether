<script lang="ts">
  import AdminExtensions from './AdminExtensions.svelte';
  let { section }: { section: string } = $props();
</script>

{#if section === 'extensions-section'}
  <AdminExtensions />
{/if}
