<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import QuickConnectApprove from '../../lib/auth/QuickConnectApprove.svelte';
  import { normalizeDeviceCode, PENDING_DEVICE_CODE } from '../../lib/auth/quick-connect';
  let code = $state<string | null>(null);
  onMount(() => {
    code = normalizeDeviceCode(window.location.hash.slice(1)) ?? normalizeDeviceCode(sessionStorage.getItem(PENDING_DEVICE_CODE) ?? '');
    sessionStorage.removeItem(PENDING_DEVICE_CODE);
    history.replaceState(history.state, '', window.location.pathname);
  });
</script>
<svelte:head><title>Connecter un appareil</title><meta name="referrer" content="no-referrer" /></svelte:head>
<main><a href={resolve('settings/')} class="back">← Retour aux paramètres</a><h1>Connecter un appareil</h1>{#if code !== null}{#key code}<QuickConnectApprove initialCode={code} />{/key}{:else}<QuickConnectApprove />{/if}</main>
<style>main { max-width: 680px; padding: 32px var(--gutter); margin: 48px auto; } h1 { font-size: clamp(26px, 5vw, 34px); margin: 28px 0; } .back { color: var(--mu); text-decoration: none; font-size: 14px; } .back:focus-visible { outline: 2px solid var(--ac); outline-offset: 4px; }</style>
