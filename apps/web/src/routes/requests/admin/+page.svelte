<script lang="ts">
  import Navbar from '../../../lib/ui/Navbar.svelte';
  import AcquisitionAdmin from '../../../lib/applications/AcquisitionAdmin.svelte';
  import { serverPresentation } from '../../../lib/config/presentation';
</script>
<svelte:head><title>Administration des demandes</title></svelte:head>
<Navbar section="admin" name={$serverPresentation.serverName} />
<main><AcquisitionAdmin /></main>
