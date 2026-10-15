<script lang="ts">
  import { tick } from 'svelte';
  import { playbackIds, sourceLabel, type Work } from './api';
  import type { Media } from '../player/types';
  import type ImmersivePlayer from '../player/ImmersivePlayer.svelte';
  let { episode, media, immersive, onselect }: { episode: Work; media: Media[]; immersive?: ImmersivePlayer; onselect: () => void } = $props();
  let selected = $state('');
  const sources = $derived(media.filter(item => playbackIds(episode).includes(item.id)));
  $effect(() => { if (!sources.some(source => source.id === selected)) selected = sources[0]?.id ?? ''; });
  async function play() { onselect(); await tick(); const item = sources.find(item => item.id === selected) ?? sources[0]; if (item) immersive?.open(item); }
</script>
<div class="episode-actions"><label>Version<select aria-label={`Version de ${episode.title}`} bind:value={selected}>{#each sources as source (source.id)}<option value={source.id}>{sourceLabel(source)}</option>{/each}</select></label><button disabled={!sources.length} onclick={play} aria-label={`Lire ${episode.title}`}>▶ Lecture</button>{#if !sources.length}<span>Aucune version disponible.</span>{/if}</div>
<style>.episode-actions { display:flex; align-items:end; gap:14px; flex-wrap:wrap; }label { display:grid; gap:6px; font-size:11px; color:var(--color-muted); }button { background:var(--color-text); color:var(--color-canvas); border:0; padding:13px 20px; border-radius:6px; min-height:46px; font-size:12px; }span { font-size:12px; color:var(--color-muted); }</style>
