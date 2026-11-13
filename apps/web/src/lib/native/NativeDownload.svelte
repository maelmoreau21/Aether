<script lang="ts">
  import Icon from '../ui/Icon.svelte';
  import { t } from '../i18n';
  import { nativeApp } from './app-bridge';

  // Shown inside the Android application only: it keeps the file for offline viewing.
  let { id, kind, title }: { id: string; kind: string; title: string } = $props();
  const app = nativeApp();
</script>

{#if app?.download}
  <button type="button" class="native-download" onclick={() => app.download?.(id, kind, title)}>
    <Icon name="download" size={15} /><span>{$t('native_download')}</span>
  </button>
{/if}

<style>
  .native-download { display: inline-flex; align-items: center; gap: 6px; min-height: 36px; padding: 0 12px; font: inherit; font-size: 13px; font-weight: 700; border-radius: var(--r-sm); background: var(--pn-2); color: var(--tx); border: 0; cursor: pointer; }
  .native-download:hover { background: var(--pn-3); }
</style>
