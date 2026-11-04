<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { session, onSessionGenerationChange } from '../auth/session';
  let enabled = $state(false), dismissed = $state(false);
  onMount(() => {
    let revision = 0, disposed = false;
    const refresh = async () => {
      const request = ++revision;
      if (!$session) { enabled = false; return; }
      try { const response = await fetch('/api/v1/playback/statistics', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(2500) }); if (response.ok) { const data = await response.json(); if (!disposed && request === revision) enabled = data.enabled; } } catch { /* Optional module. */ }
    };
    void refresh();
    const off = onSessionGenerationChange(() => { enabled = false; dismissed = false; void refresh(); });
    window.addEventListener('statistics-settings-changed', refresh);
    const timer = setInterval(() => void refresh(), 60000);
    return () => { disposed = true; revision++; off(); clearInterval(timer); window.removeEventListener('statistics-settings-changed', refresh); };
  });
</script>
{#if enabled && !dismissed}<aside class="statistics-notice" aria-label="Collecte statistique"><p>Ce serveur collecte les lectures et les interactions avec le lecteur. <a href={resolve('statistics/')}>Consulter mes statistiques et la conservation</a></p><button aria-label="Masquer l’information statistique" onclick={() => dismissed = true}>Fermer</button></aside>{/if}
<style>.statistics-notice{position:relative;z-index:10;display:flex;align-items:center;justify-content:space-between;gap:16px;padding:10px clamp(16px,4vw,48px);background:var(--color-surface);color:var(--color-muted);border-bottom:1px solid var(--color-line);font-size:.8rem}.statistics-notice p{margin:0;line-height:1.6}.statistics-notice a{color:var(--color-accent)}button{background:none;border:0;color:var(--color-muted);cursor:pointer;padding:8px}</style>
