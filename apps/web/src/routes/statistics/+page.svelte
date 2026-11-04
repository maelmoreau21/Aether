<script lang="ts">
  import Navbar from '../../lib/ui/Navbar.svelte';
  import Statistics from '../../lib/statistics/Statistics.svelte';
  import { serverPresentation } from '../../lib/config/presentation';
</script>
<svelte:head><title>Mes statistiques</title></svelte:head>
<Navbar section="settings" name={$serverPresentation.serverName} />
<main><Statistics /></main>
<style>main{padding:clamp(20px,4vw,56px) var(--gutter);padding-bottom:calc(48px + var(--music-player-height,0px));}</style>
