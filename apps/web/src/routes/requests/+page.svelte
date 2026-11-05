<script lang="ts">
  // Former standalone page: kept so old links land in the matching settings panel.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { t } from '../../lib/i18n';
  onMount(() => { void goto(resolve('settings/#requests'), { replaceState: true }); });
</script>
<svelte:head><title>{$t('settings_requests')}</title></svelte:head>
<main><p><a href={resolve('settings/#requests')}>{$t('settings_requests')}</a></p></main>
<style>main{padding:clamp(20px,4vw,56px) var(--gutter);}</style>
