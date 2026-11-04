<script lang="ts">
  import Navbar from '../../lib/ui/Navbar.svelte';
  import Reports from '../../lib/applications/Reports.svelte';
  import { serverPresentation } from '../../lib/config/presentation';
</script>
<svelte:head><title>Signalements</title></svelte:head>
<Navbar section="reports" name={$serverPresentation.serverName} />
<main><Reports /></main>
