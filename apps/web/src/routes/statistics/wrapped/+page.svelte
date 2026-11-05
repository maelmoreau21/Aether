<script lang="ts">
  // Full-screen yearly recap. `?user=<id>` opens an account's recap for administrators.
  import { onMount } from 'svelte';
  import Wrapped from '../../../lib/statistics/Wrapped.svelte';
  import { st } from '../../../lib/statistics/i18n';
  // Read on mount: the page is prerendered, so the query string is not known at build time.
  let userId = $state<string | null>(null);
  onMount(() => { userId = new URLSearchParams(window.location.search).get('user') ?? ''; });
</script>
<svelte:head><title>{$st('w_title', { year: new Date().getFullYear() })}</title></svelte:head>
{#if userId !== null}{#key userId}<Wrapped {userId} />{/key}{/if}
