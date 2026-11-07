<script lang="ts">
  import Navbar from '../../../lib/ui/Navbar.svelte';
  import AcquisitionAdmin from '../../../lib/applications/AcquisitionAdmin.svelte';
  import { serverPresentation } from '../../../lib/config/presentation';
  import { t } from '../../../lib/i18n';
</script>
<svelte:head><title>{$t('requests_administration')}</title></svelte:head>
<Navbar section="admin" name={$serverPresentation.serverName} />
<main><AcquisitionAdmin /></main>
