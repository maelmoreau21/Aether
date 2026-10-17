<script lang="ts">
  import { tick } from 'svelte';
  import {
    playbackIds,
    sourceLabel,
    fetchMediaDescriptor,
    revalidateMediaDescriptor,
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
  let selected = $state('');
  let cacheVersion = $state(0);
  let prefetchController: AbortController | null = null;
  let inFlightPrefetches: string[] = [];

  $effect(() => {
    if (!versionIds.includes(selected)) {
      selected = versionIds[0] ?? '';
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
    onselect?.();
    if (onlaunch) {
      await onlaunch(episode, selected);
    } else {
      await tick();
      const item = await revalidateMediaDescriptor(selected);
      immersive?.open(item);
    }
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
      {#each versionIds as id, index (id)}
        <option value={id}>{getLabel(id, index)}</option>
      {/each}
    </select>
  </label>
  <button
    disabled={!versionIds.length || isLaunching}
    onclick={handlePlay}
    aria-label={`Lire ${episode.title}`}
  >
    {isLaunching ? 'Chargement…' : '▶ Lecture'}
  </button>
  {#if !versionIds.length}
    <span>Aucune version disponible.</span>
  {/if}
</div>

<style>
  .episode-actions { display:flex; align-items:end; gap:14px; flex-wrap:wrap; }
  label { display:grid; gap:6px; font-size:11px; color:var(--color-muted); }
  button { background:var(--color-text); color:var(--color-canvas); border:0; padding:13px 20px; border-radius:6px; min-height:46px; font-size:12px; cursor:pointer; }
  button:disabled { opacity:0.6; cursor:not-allowed; }
  span { font-size:12px; color:var(--color-muted); }
</style>
