<script lang="ts">
  import { tick } from 'svelte';
  import OfflineDownload from './OfflineDownload.svelte';
  import { bestVersion } from '../player/version-choice';
  import {
    playbackIds,
    sourceLabel,
    fetchMediaDescriptor,
    revalidateMediaDescriptor,
    fetchMediaDescriptors,
    getCachedMedia,
    type Work
  } from './api';
  import { session, getSessionGeneration } from '../auth/session';
  import type { Media } from '../player/types';
  import type ImmersivePlayer from '../player/ImmersivePlayer.svelte';

  let {
    episode,
    media,
    immersive,
    onselect,
    onlaunch,
    isLaunching = false
  }: {
    episode: Work;
    media: Media[];
    immersive?: ImmersivePlayer;
    onselect?: () => void;
    onlaunch?: (episode: Work, versionId: string) => Promise<void>;
    isLaunching?: boolean;
  } = $props();

  const versionIds = $derived(playbackIds(episode));
  let selected = $state('auto');
  let cacheVersion = $state(0);
  let launchError=$state('');
  let prefetchController: AbortController | null = null;
  let inFlightPrefetches: string[] = [];

  $effect(() => {
    if (selected!=='auto' && !versionIds.includes(selected)) {
      selected = 'auto';
    }
  });

  $effect(() => {
    const activeSessionKey = $session ? `${$session.user.id}:${$session.csrfToken}` : 'anonymous';
    void activeSessionKey;
    return () => {
      prefetchController?.abort();
      prefetchController = null;
      inFlightPrefetches = [];
    };
  });

  function getLabel(id: string, index: number): string {
    void $session;
    void cacheVersion;
    const item = getCachedMedia(id) ?? media.find(m => m.id === id);
    if (item) return sourceLabel(item);
    return versionIds.length > 1 ? `Version ${index + 1}` : 'Version locale';
  }

  function prefetchVersions() {
    onselect?.();
    if (!prefetchController || prefetchController.signal.aborted) {
      prefetchController = new AbortController();
    }
    const signal = prefetchController.signal;
    const startGen = getSessionGeneration();
    for (const id of versionIds) {
      if (!getCachedMedia(id) && !inFlightPrefetches.includes(id)) {
        inFlightPrefetches = [...inFlightPrefetches, id];
        void fetchMediaDescriptor(id, signal)
          .then(() => {
            if (!signal.aborted && startGen === getSessionGeneration()) {
              cacheVersion += 1;
            }
          })
          .catch(() => {})
          .finally(() => {
            inFlightPrefetches = inFlightPrefetches.filter(item => item !== id);
          });
      }
    }
  }

  async function handlePlay() {
    launchError='';
    try {
    onselect?.();
    let id=selected;
    if(id==='auto'){
      const candidates=await fetchMediaDescriptors(versionIds,new AbortController().signal);
      const probe=document.createElement('video');
      id=bestVersion(candidates,mime=>probe.canPlayType(mime))?.id??versionIds[0]??'';
    }
    if (onlaunch) {
      await onlaunch(episode, id);
    } else {
      await tick();
      const item = await revalidateMediaDescriptor(id);
      immersive?.open(item);
    }
    }catch(cause){launchError=cause instanceof Error?cause.message:'Lecture indisponible.';}
  }
</script>

<div class="episode-actions">
  <label>Version
    <select
      aria-label={`Version de ${episode.title}`}
      bind:value={selected}
      onfocus={prefetchVersions}
      onpointerenter={prefetchVersions}
    >
      <option value="auto">Meilleure version disponible</option>
      {#each versionIds as id, index (id)}
        <option value={id}>{getLabel(id, index)}</option>
      {/each}
    </select>
  </label>
  <button
    disabled={!versionIds.length || isLaunching || episode.available === false}
    onclick={handlePlay}
    aria-label={`Lire ${episode.title}`}
  >
    {episode.available === false ? 'Node indisponible' : isLaunching ? 'Chargement…' : '▶ Lecture'}
  </button>
  {#if episode.available === false}
    <span>Média temporairement indisponible (Media Node hors ligne).</span>
  {:else if !versionIds.length}
    <span>Aucune version disponible.</span>
  {/if}
</div>
{#if launchError}<p role="alert">{launchError}</p>{/if}
{#if !episode.synthetic && versionIds.length}<OfflineDownload url={`/api/v1/assets/${selected==='auto'?versionIds[0]:selected}/content`} title={episode.title} mime={(media.find(item=>item.id===(selected==='auto'?versionIds[0]:selected)))?.sources.find(source=>!source.prepare)?.mime??'video/mp4'} />{/if}

<style>
  .episode-actions { display:flex; align-items:end; gap:14px; flex-wrap:wrap; }
  label { display:grid; gap:6px; font-size:11px; color:var(--color-muted); }
  button { background:var(--color-text); color:var(--color-canvas); border:0; padding:13px 20px; border-radius:6px; min-height:46px; font-size:12px; cursor:pointer; }
  button:disabled { opacity:0.6; cursor:not-allowed; }
  span { font-size:12px; color:var(--color-muted); }
</style>
