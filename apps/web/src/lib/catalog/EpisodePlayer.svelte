<script lang="ts">
  import { tick } from 'svelte';
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
  import { t } from '../i18n';

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
    return versionIds.length > 1 ? `Version ${index + 1}` : $t('catalog_local_version');
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
    }catch(cause){launchError=cause instanceof Error?cause.message:$t('playback_unavailable');}
  }
</script>
<div class="episode-actions">
  <button class="episode-play" onpointerenter={() => { if (!episode.synthetic) preloadProgress(episode.id); prefetchVersions(); }} onfocus={() => { if (!episode.synthetic) preloadProgress(episode.id); prefetchVersions(); }} disabled={!versionIds.length || isLaunching || episode.available === false} onclick={handlePlay} aria-label={$t('play_title', { title: episode.title })}>
    {episode.available === false ? $t('temporarily_unavailable') : isLaunching ? 'Chargement…' : $t('play_the_episode')}
  </button>
  {#if episode.available === false}<span>{$t('the_server_hosting_this_episode_is')}</span>{:else if !versionIds.length}<span>{$t('no_version_available')}</span>{/if}
</div>
{#if launchError}<p role="alert">{launchError}</p>{/if}
{#if versionIds.length > 1 || (!episode.synthetic && versionIds.length)}
  <details class="episode-versions"><summary>{versionIds.length > 1 ? $t('versions_and_download') : 'Téléchargement'}</summary>
    {#if versionIds.length > 1}
      <label>{$t('version')}<select aria-label={$t('version_of', { title: episode.title })} bind:value={selected} onfocus={prefetchVersions} onpointerenter={prefetchVersions}><option value="auto">{$t('best_available_version')}</option>{#each versionIds as id, index (id)}<option value={id}>{getLabel(id, index)}</option>{/each}</select></label>
    {/if}
  </details>
{/if}

<style>
  .episode-actions { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; max-width: 100%; }
  .episode-play { display: inline-flex; align-items: center; justify-content: center; min-height: 44px; max-width: 100%; padding: 0 20px; border: 0; border-radius: var(--r-md); background: var(--btn); color: var(--btn-tx); font-size: 14px; font-weight: 700; cursor: pointer; transition: opacity var(--t-fast) var(--ease), transform var(--t-fast) var(--ease); }
  .episode-play:hover:not(:disabled) { opacity: 0.9; }
  .episode-play:active:not(:disabled) { transform: scale(0.98); }
  .episode-play:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; }
  .episode-actions span { font-size: 13px; color: var(--mu); }
  .episode-versions { max-width: 100%; width: 100%; font-size: 14px; }
  summary { display: flex; align-items: center; min-height: 44px; color: var(--mu); font-weight: 600; cursor: pointer; }
  summary:hover { color: var(--tx); }
  summary:focus-visible { outline: 2px solid var(--ac); outline-offset: 3px; border-radius: var(--r-xs); }
  .episode-versions[open] summary { color: var(--tx); }
  .episode-versions[open] { display: grid; gap: 12px; justify-items: start; }
  label { display: grid; gap: 8px; width: min(420px, 100%); color: var(--tx); font-size: 14px; font-weight: 700; }
  select { box-sizing: border-box; width: 100%; max-width: 100%; min-height: 46px; padding: 0 14px; border: 1px solid transparent; border-radius: var(--r-sm); background: var(--pn); color: var(--tx); font: inherit; font-size: 16px; font-weight: 500; transition: border-color var(--t-fast) var(--ease), background-color var(--t-fast) var(--ease); }
  select:focus { outline: none; border-color: var(--ac); background: var(--bg-3); }
  select option { background: var(--bg-3); color: var(--tx); }
  [role='alert'] { color: var(--err); font-size: 13px; line-height: 1.6; }
</style>
