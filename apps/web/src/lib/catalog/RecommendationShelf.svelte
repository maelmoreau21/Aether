<script lang="ts">
  import { onMount } from 'svelte';
  import { session } from '../auth/session';
  import { resolve } from '$app/paths';
  import type { Work } from './api';
  import Poster from './Poster.svelte';
  import MediaShelf from '../ui/MediaShelf.svelte';
  let items = $state<(Work & { recommendationReason: string })[]>([]);
  let enabled = $state(false);
  let mode = $state('');
  let error = $state('');
  let mounted = $state(false);
  let controller: AbortController | undefined;
  let owner = '';
  async function load() {
    controller?.abort(); const current = new AbortController(); controller = current;
    items = []; enabled = false; error = '';
    try {
      const response = await fetch('/api/v1/account/recommendations', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.any([current.signal, AbortSignal.timeout(15000)]) });
      if (!response.ok) throw new Error();
      const result = await response.json();
      if (current.signal.aborted) return;
      enabled = result.enabled; mode = result.mode; items = result.items;
    } catch { if (!current.signal.aborted) error = 'Les recommandations sont temporairement indisponibles.'; }
  }
  $effect(() => {
    const next = $session?.user.id ?? '';
    if (mounted && next !== owner) { owner = next; if (next) void load(); else { controller?.abort(); items = []; enabled = false; } }
  });
  onMount(() => {
    mounted = true;
    const refresh = () => { if ($session) void load(); };
    window.addEventListener('focus', refresh); window.addEventListener('media-recommendations-changed', refresh);
    return () => { controller?.abort(); window.removeEventListener('focus', refresh); window.removeEventListener('media-recommendations-changed', refresh); };
  });
</script>
{#if enabled && items.length}
  <MediaShelf title={mode === 'personalized' ? 'Recommandé pour vous' : 'À découvrir dans votre catalogue'}>
    {#each items as item (item.id)}
      <div class="recommendation"><Poster work={item} href={item.kind === 'series' ? resolve(`series/?work=${encodeURIComponent(item.id)}`) : resolve(`films/?work=${encodeURIComponent(item.id)}`)} /><small>{item.recommendationReason}</small></div>
    {/each}
  </MediaShelf>
{:else if enabled}<p role="status">Aucun autre titre accessible à proposer pour le moment.</p>{/if}
{#if error}<p role="status">{error}</p>{/if}
<style>.recommendation { min-width:0; } small { display:block; margin-top:8px; color:var(--color-muted); line-height:1.4; }</style>
