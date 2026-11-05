<script lang="ts">
  import Navbar from '../../lib/ui/Navbar.svelte';
  import DownloadsPanel from '../../lib/catalog/DownloadsPanel.svelte';
</script>
<Navbar section="downloads" />
<main><DownloadsPanel /></main>
<style>
  main { width: 100%; max-width: var(--maxw); margin: 0 auto; padding: 20px var(--gutter) 80px; min-width: 0; color: var(--tx); }
  @media (max-width: 640px) { main { padding-top: 12px; } }
</style>
