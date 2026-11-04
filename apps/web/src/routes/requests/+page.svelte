<script lang="ts">
  import Navbar from '../../lib/ui/Navbar.svelte';
  import Requests from '../../lib/applications/Requests.svelte';
  import { serverPresentation } from '../../lib/config/presentation';
</script>
<svelte:head><title>Demandes</title></svelte:head>
<Navbar section="requests" name={$serverPresentation.serverName} />
<main><Requests /></main>
