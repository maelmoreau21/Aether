<script lang="ts">
  import { tick } from 'svelte';
  import OfflineDownload from './OfflineDownload.svelte';
  import { bestVersion } from '../player/version-choice';
  import { preloadProgress } from '../player/progress';
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
  const downloadOptions = $derived.by(() => {
    void cacheVersion;
    return versionIds.map((id, index) => {
      const item = getCachedMedia(id) ?? media.find(entry => entry.id === id);
      return {url:`/api/v1/assets/${encodeURIComponent(id)}/content`,label:item ? sourceLabel(item) : `Fichier ${index + 1}`,mime:item?.sources.find(source => !source.prepare)?.mime ?? 'application/octet-stream'};
    });
  });
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
    if(id==='auto' && versionIds.length === 1) id = versionIds[0];
    if (onlaunch) { await onlaunch(episode, id); return; }
    if(id==='auto'){
      const candidates=await fetchMediaDescriptors(versionIds,new AbortController().signal);
      const probe=document.createElement('video');
      id=bestVersion(candidates,mime=>probe.canPlayType(mime))?.id??versionIds[0]??'';
    }
    {
      await tick();
      const item = await revalidateMediaDescriptor(id);
      immersive?.open(item);
    }
    }catch(cause){launchError=cause instanceof Error?cause.message:'Lecture indisponible.';}
  }
</script>
<div class="episode-actions">
  <button class="episode-play" onpointerenter={() => { if (!episode.synthetic) preloadProgress(episode.id); prefetchVersions(); }} onfocus={() => { if (!episode.synthetic) preloadProgress(episode.id); prefetchVersions(); }} disabled={!versionIds.length || isLaunching || episode.available === false} onclick={handlePlay} aria-label={`Lire ${episode.title}`}>
    {episode.available === false ? 'Temporairement indisponible' : isLaunching ? 'Chargement…' : 'Lire l’épisode'}
  </button>
  {#if episode.available === false}<span>Le serveur qui héberge cet épisode est hors ligne.</span>{:else if !versionIds.length}<span>Aucune version disponible.</span>{/if}
</div>
{#if launchError}<p role="alert">{launchError}</p>{/if}
{#if versionIds.length > 1 || (!episode.synthetic && versionIds.length)}
  <details class="episode-versions"><summary>{versionIds.length > 1 ? 'Versions et téléchargement' : 'Téléchargement'}</summary>
    {#if versionIds.length > 1}
      <label>Version<select aria-label={`Version de ${episode.title}`} bind:value={selected} onfocus={prefetchVersions} onpointerenter={prefetchVersions}><option value="auto">Meilleure version disponible</option>{#each versionIds as id, index (id)}<option value={id}>{getLabel(id, index)}</option>{/each}</select></label>
    {/if}
    {#if !episode.synthetic && versionIds.length}<OfflineDownload options={downloadOptions} url={`/api/v1/assets/${selected === 'auto' ? versionIds[0] : selected}/content`} title={episode.title} mime={media.find(item => item.id === (selected === 'auto' ? versionIds[0] : selected))?.sources.find(source => !source.prepare)?.mime ?? 'application/octet-stream'} />{/if}
  </details>
{/if}

<style>
  .episode-actions { display:flex; align-items:center; gap:12px; flex-wrap:wrap; max-width:100%; }
  .episode-play { min-height:44px; max-width:100%; padding:11px 20px; border:0; border-radius:var(--radius-sm); background:var(--color-play); color:var(--color-on-play); font-size:14px; font-weight:600; }
  .episode-actions span { font-size:12px; color:var(--color-muted); }
  .episode-versions { max-width:100%; width:100%; font-size:13px; }
  summary { min-height:44px; padding-block:10px; color:var(--color-muted); cursor:pointer; }
  label { display:grid; gap:8px; color:var(--color-muted); font-size:12px; }
  select { max-width:100%; min-height:44px; font-size:16px; }
  [role='alert'] { color:var(--color-danger); font-size:13px; line-height:1.6; }
</style>
