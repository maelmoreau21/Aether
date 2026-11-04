<script lang="ts">



  import { tick } from 'svelte';

  import { SvelteSet } from 'svelte/reactivity';



  import Icon from '../ui/Icon.svelte';



  import { audioLabel, videoLabel, videoResolutionBadge, containerLabel } from './track-labels';



  import { audioChoices } from './available-audio';



  import type { Media, PlaybackState } from './types';







  export type Menu = 'audio' | 'subtitles' | 'video' | 'speed' | 'settings' | null;







  let {



    state: playback,



    media,



    catalogue,



    audio,



    subtitle,



    visible,



    busy,



    menu = $bindable(null),



    fullscreen,



    onPlay,



    onVolume,



    onMute,



    onSeek,



    onAudio,



    onSubtitle,



    onSource,



    onSpeed,



    onFullscreen,



    audioOffset = 0, subtitleOffset = 0, onAudioOffset, onSubtitleOffset,



    aspectRatio = 'contain', onAspectRatio, repeat = false, onRepeat, onStats,



    skipBackSeconds = 10, skipForwardSeconds = 10,
    onWake,
    buffered = 0,
    hideCreditsSkip = false



  }: {



    audioOffset?: number; subtitleOffset?: number; onAudioOffset?: (value: number) => void; onSubtitleOffset?: (value: number) => void;



    skipBackSeconds?: number; skipForwardSeconds?: number;
    state: PlaybackState;



    media: Media;



    catalogue: Media[];



    audio: string;



    subtitle: string | null;



    visible: boolean;



    busy: boolean;



    menu?: Menu;



    fullscreen: boolean;



    onPlay: () => void;



    onVolume: (value: number) => void;



    onMute: () => void;



    onSeek: (value: number) => void;



    onAudio: (id: string) => void;



    onSubtitle: (id: string | null) => void;



    onSource: (id: string) => void;



    onSpeed: (speed: number) => void;



    onFullscreen: () => void;



    aspectRatio?: 'contain' | 'cover' | 'fill';



    onAspectRatio?: (ratio: 'contain' | 'cover' | 'fill') => void;



    repeat?: boolean;



    onRepeat?: (repeat: boolean) => void;



    onStats?: () => void;



    onWake: () => void;



    /** Position (s) jusqu’où le média est déjà chargé. */



    buffered?: number;



    /** La carte « épisode suivant » remplace le bouton « Passer le générique ». */



    hideCreditsSkip?: boolean;



  } = $props();







  let panel = $state<HTMLDivElement>();



  let triggers: HTMLDivElement;



  let hoverTime = $state<number | null>(null);



  let isSeeking = $state(false);



  let trickplay = $state<Media['trickplay']>(null);



  let previewFailedSheets = $state<Set<number>>(new Set());



  const preloadedSheets = new SvelteSet<number>();







  function preloadSheet(index: number) {



    if (!trickplay || index < 0 || index >= trickplay.sheetCount || preloadedSheets.has(index)) return;



    preloadedSheets.add(index);



    const img = new Image();



    img.src = trickplay.urlPrefix + index;



  }







  $effect(() => {



    if (!trickplay) {



      preloadedSheets.clear();



      return;



    }



    preloadSheet(0);



  });







  $effect(() => {



    if (hoverTime === null || !trickplay || !playback.duration) return;



    const count = Math.min(trickplay.thumbnailCount, trickplay.sheetCount * trickplay.columns * trickplay.rows);



    const frame = Math.max(0, Math.min(count - 1, Math.floor(hoverTime * 1000 / trickplay.intervalMs)));



    const perSheet = trickplay.columns * trickplay.rows;



    const sheet = Math.floor(frame / perSheet);



    preloadSheet(sheet);



    if (sheet > 0) preloadSheet(sheet - 1);



    if (sheet + 1 < trickplay.sheetCount) preloadSheet(sheet + 1);



  });







  function updateHoverPosition(clientX: number, target: HTMLElement) {



    if (!playback.duration) return;



    const rect = target.getBoundingClientRect();



    hoverTime = Math.max(0, Math.min(playback.duration, ((clientX - rect.left) / rect.width) * playback.duration));



  }



  $effect(() => {



    const id = media?.id ?? '';



    trickplay = null;



    previewFailedSheets = new Set();



    if (!id || media.synthetic) return;



    const controller = new AbortController();



    let retries = 0;



    let timer: ReturnType<typeof setTimeout> | undefined;



    const check = async () => {



      try {



        const res = await fetch('/api/v1/assets/' + encodeURIComponent(id) + '/trickplay', {



          credentials: 'same-origin', signal: controller.signal



        });



        const value = await res.json().catch(() => null);



        if (controller.signal.aborted || !value) return;



        if (res.ok && value.status === 'ready' &&



            value.intervalMs >= 1000 && value.width > 0 && value.height > 0 &&



            value.columns > 0 && value.rows > 0 && value.thumbnailCount > 0 &&



            value.sheetCount > 0 && typeof value.urlPrefix === 'string' &&



            value.urlPrefix === '/api/v1/assets/' + encodeURIComponent(id) + '/trickplay/') {



          trickplay = value;



          return;



        }



        if (res.status === 202 && value.status === 'generating' && retries++ < 30) {



          timer = setTimeout(() => void check(), 4000);



        }



      } catch { /* unavailable: keep the ordinary seek timer */ }



    };



    void check();



    return () => { controller.abort(); if (timer) clearTimeout(timer); };



  });



  const timelineChapters = $derived((media.chapters ?? [])
    .filter(chapter => Number.isFinite(chapter.start) && chapter.start >= 0 && chapter.start < playback.duration)
    .toSorted((a, b) => a.start - b.start));
  const activeChapter = $derived(timelineChapters.findLast(chapter => chapter.start <= playback.position));
  const hoverChapter = $derived(hoverTime === null ? undefined : timelineChapters.findLast(chapter => chapter.start <= hoverTime!));

  const previewFrame = $derived.by(() => {



    if (hoverTime === null || !trickplay || !playback.duration) return null;



    const count = Math.min(trickplay.thumbnailCount, trickplay.sheetCount * trickplay.columns * trickplay.rows);



    const frame = Math.max(0, Math.min(count - 1, Math.floor(hoverTime * 1000 / trickplay.intervalMs)));



    const perSheet = trickplay.columns * trickplay.rows;



    const sheet = Math.floor(frame / perSheet);



    if (previewFailedSheets.has(sheet)) return null;



    const local = frame % perSheet;



    return {



      url: trickplay.urlPrefix + sheet,



      width: trickplay.width, height: trickplay.height,



      x: (local % trickplay.columns) * trickplay.width,



      y: Math.floor(local / trickplay.columns) * trickplay.height,



      fullWidth: trickplay.columns * trickplay.width,



      fullHeight: trickplay.rows * trickplay.height



    };



  });



  let canSkipIntro = $derived(Boolean(media?.intro && playback.position >= media.intro.start && playback.position < media.intro.end));
  let canSkipCredits = $derived(Boolean(!hideCreditsSkip && media?.credits && playback.position >= media.credits.start && playback.position < media.credits.end));
  let canSkipRecap = $derived(Boolean(media?.recap && playback.position >= media.recap.start && playback.position < media.recap.end));
  let canSkipPreview = $derived(Boolean(media?.preview && playback.position >= media.preview.start && playback.position < media.preview.end));







  const source = $derived(media.sources.find(source => source.audioTrackId === audio) ?? media.sources[0]);



  const selectableAudio = $derived(audioChoices(media, audio));



  const selectableVersions = $derived(catalogue.filter(item => item.available !== false && item.nodeStatus !== 'revoked' && item.nodeStatus !== 'disconnected'));



  $effect(() => { if (menu === 'audio' && selectableAudio.length <= 1) menu = null; });



  const videoTracks = $derived((source?.tracks?.some(track => track.codec_type === 'video') ? source.tracks : media.sources[0]?.tracks)?.filter(track => track.codec_type === 'video') ?? []);



  const remainingTime = $derived(Math.max(0, (playback.duration || 0) - (playback.position || 0)));







  let now = $state(Date.now());



  $effect(() => {



    const timer = setInterval(() => { now = Date.now(); }, 5000);



    return () => clearInterval(timer);



  });







  const projectedEndTime = $derived.by(() => {



    if (!Number.isFinite(remainingTime) || remainingTime <= 0 || !playback.duration) return '';



    const speed = playback.speed > 0 ? playback.speed : 1;



    const end = new Date(now + (remainingTime / speed) * 1000);



    const hours = end.getHours().toString().padStart(2, '0');



    const minutes = end.getMinutes().toString().padStart(2, '0');



    return `${hours}h${minutes}`;



  });







  function skipChapter(direction: -1 | 1) {



    const chapters = media?.chapters;



    if (!chapters || chapters.length === 0) return;



    const sorted = [...chapters].sort((a, b) => a.start - b.start);



    const pos = playback.position;







    if (direction === 1) {



      const next = sorted.find(c => c.start > pos + 1);



      if (next) {



        onSeek(next.start);



      } else if (playback.duration) {



        onSeek(playback.duration);



      }



    } else {



      let currentIndex = -1;



      for (let i = 0; i < sorted.length; i++) {



        if (sorted[i].start <= pos + 0.5) {



          currentIndex = i;



        } else {



          break;



        }



      }







      if (currentIndex === -1) {



        onSeek(0);



      } else {



        const currentChapter = sorted[currentIndex];



        if (pos - currentChapter.start > 3) {



          onSeek(currentChapter.start);



        } else if (currentIndex > 0) {



          onSeek(sorted[currentIndex - 1].start);



        } else {



          onSeek(0);



        }



      }



    }



  }







  const title = $derived(



    menu === 'audio'



      ? 'Langue audio'



      : menu === 'subtitles'



        ? 'Sous-titres'



        : menu === 'video'



          ? 'Vidéo et sources'



          : menu === 'settings'



            ? 'Paramètres'



            : 'Vitesse de lecture'



  );







  function clock(seconds: number) {



    if (!Number.isFinite(seconds)) return '00:00';



    const hours = Math.floor(seconds / 3600);



    return `${hours ? hours + ':' : ''}${Math.floor((seconds / 60) % 60).toString().padStart(2, '0')}:${Math.floor(seconds % 60).toString().padStart(2, '0')}`;



  }







  const menuTabs = $derived(([
    ...(selectableAudio.length > 1 ? [{ id: 'audio', label: 'Audio' }] : []),
    { id: 'subtitles', label: 'Sous-titres' },
    { id: 'video', label: 'Version' },
    { id: 'speed', label: 'Vitesse' },
    { id: 'settings', label: 'Réglages' }
  ]) as { id: Exclude<Menu, null>; label: string }[]);
  export function closeMenu() {



    const active = menu;



    menu = null;



    triggers?.querySelector<HTMLButtonElement>(`[data-menu="${active}"]`)?.focus();



  }







  async function toggleMenu(value: Menu) {



    if (menu === value) {



      closeMenu();



      return;



    }



    menu = value;



    onWake();



    await tick();



    const items = panel ? Array.from(panel.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]:not(:disabled), [role="menuitem"]:not(:disabled)')) : [];



    const target = panel?.querySelector<HTMLButtonElement>('[role="menuitemradio"][aria-checked="true"]:not(:disabled)') ?? items[0];



    if (target) {



      for (const item of items) item.tabIndex = -1;



      target.tabIndex = 0;



      target.focus();



    }



  }







  function navigateMenu(event: KeyboardEvent) {



    if (event.key === 'Escape') {



      event.preventDefault();



      event.stopPropagation();



      closeMenu();



      return;



    }



    if (event.key === 'Tab') {



      closeMenu();



      return;



    }



    if (event.target instanceof HTMLInputElement) return;



    const items = Array.from(panel!.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]:not(:disabled), [role="menuitem"]:not(:disabled)'));



    if (items.length === 0) return;



    const index = items.indexOf(document.activeElement as HTMLButtonElement);



    if (['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) {



      event.preventDefault();



      event.stopPropagation();



      let next: number;



      if (event.key === 'Home') next = 0;



      else if (event.key === 'End') next = items.length - 1;



      else if (index === -1) next = event.key === 'ArrowUp' ? items.length - 1 : 0;



      else next = (index + (event.key === 'ArrowUp' ? -1 : 1) + items.length) % items.length;



      for (const item of items) item.tabIndex = -1;



      items[next].tabIndex = 0;



      items[next]?.focus();



    }



  }







  function choose(action: () => void) {



    closeMenu();



    action();



  }



</script>







<div class="player-controls" class:concealed={!visible} inert={!visible}>



  {#if menu}



    <!-- Menu navigation follows the ARIA menu keyboard pattern. -->



    <div



      class="floating-menu"



      bind:this={panel}



      id="player-menu"



      role="menu"



      tabindex="-1"



      aria-label={title}



      onkeydown={navigateMenu}



    >



      <div class="menu-heading" role="none">



        {#if menu === 'speed'}



          <button type="button" class="menu-back" aria-label="Retour aux paramètres" tabindex="-1" onclick={() => menu = 'settings'}>



            <Icon name="back" size={18} />



          </button>



        {/if}



        <span>{title}</span>



        <button type="button" class="menu-close" aria-label="Fermer le menu" tabindex="-1" onclick={closeMenu}>×</button>



      </div>
      <!-- Halo : le panneau garde toutes les sections à portée, une par onglet. -->
      <div class="menu-tabs" role="group" aria-label="Sections du panneau de lecture">
        {#each menuTabs as tab (tab.id)}
          <button type="button" role="menuitem" tabindex="-1" aria-current={menu === tab.id ? 'true' : undefined} onclick={() => { if (menu !== tab.id) void toggleMenu(tab.id); }}>{tab.label}</button>
        {/each}
      </div>







      {#if menu === 'audio'}



        <div role="group" aria-label="Langues audio">



          {#each selectableAudio as item (item.id)}



            <button



              data-audio-id={item.audioTrackId}



              role="menuitemradio"



              tabindex={item.audioTrackId === audio ? 0 : -1}



              aria-checked={item.audioTrackId === audio}



              disabled={busy}



              onclick={() => choose(() => onAudio(item.audioTrackId))}



            >



              <span>{audioLabel(item)}</span>



              {#if item.audioTrackId === audio}



                <Icon name="check" size={18} />



              {/if}



            </button>



          {/each}



        </div>



        <button role="menuitem" tabindex="-1" disabled={busy || !source?.externalAudioUrl} onclick={() => panel?.querySelector<HTMLInputElement>('input')?.focus()}>Régler le décalage audio</button>



        <label class="offset-control">Décalage audio (s)



          <input aria-label="Décalage audio en secondes" type="number" min="-30" max="30" step="0.05" value={audioOffset} disabled={busy || !source?.externalAudioUrl} onchange={(e) => onAudioOffset?.(e.currentTarget.valueAsNumber)} />



        </label>



        <p class="menu-note">Positif : retarder. Appliqué aux pistes préparées et externes.</p>



      {:else if menu === 'subtitles'}



        <div role="group" aria-label="Pistes de sous-titres">



          <button



            data-subtitle-id="off"



            role="menuitemradio"



            tabindex={subtitle === null ? 0 : -1}



            aria-checked={subtitle === null}



            disabled={busy}



            onclick={() => choose(() => onSubtitle(null))}



          >



            <span>Désactivés</span>



            {#if subtitle === null}



              <Icon name="check" size={18} />



            {/if}



          </button>



          {#each media.subtitles as item (item.id)}



            <button



              data-subtitle-id={item.id}



              role="menuitemradio"



              tabindex={item.id === subtitle ? 0 : -1}



              aria-checked={item.id === subtitle}



              disabled={busy}



              onclick={() => choose(() => onSubtitle(item.id))}



            >



              <span>{item.label}<small>WebVTT · {item.language.toUpperCase()}</small></span>



              {#if item.id === subtitle}



                <Icon name="check" size={18} />



              {/if}



            </button>



          {/each}



        </div>



        <button role="menuitem" tabindex="-1" disabled={busy} onclick={() => panel?.querySelector<HTMLInputElement>('input')?.focus()}>Régler le décalage des sous-titres</button>



        <label class="offset-control">Décalage sous-titres (s)



          <input aria-label="Décalage sous-titres en secondes" type="number" min="-30" max="30" step="0.05" value={subtitleOffset} disabled={busy} oninput={(e) => onSubtitleOffset?.(e.currentTarget.valueAsNumber)} onchange={(e) => onSubtitleOffset?.(e.currentTarget.valueAsNumber)} />



        </label>



      {:else if menu === 'video'}



        <div role="group" aria-label="Piste vidéo de la source active">



          <p class="menu-section" role="presentation">Piste vidéo active</p>



          {#each videoTracks as track, index (track.index ?? index)}



            <div class="track-info" role="note">



              <Icon name="film" size={17} />



              <div>



                <span>{videoLabel(track)}</span>



                {#if videoResolutionBadge(track.width, track.height)}



                  <small class="track-badge">{videoResolutionBadge(track.width, track.height)}</small>



                {/if}



              </div>



            </div>



          {/each}



          <p class="menu-note" role="note">



            Conteneur : {containerLabel(source?.mime)} · Décodage assuré par le navigateur.



          </p>



        </div>



        {#if selectableVersions.length > 1}



        <div role="separator" class="menu-separator"></div>



        <div role="group" aria-label="Sources de démonstration">



          <p class="menu-section" role="presentation">Versions / Sources disponibles</p>



          {#each selectableVersions as item (item.id)}



            <button



              data-source-id={item.id}



              role="menuitemradio"



              tabindex={item.id === media.id ? 0 : -1}



              aria-checked={item.id === media.id}



              disabled={busy}



              onclick={() => choose(() => onSource(item.id))}



            >



              <span>{item.title}<small>{item.synthetic ? 'Source préparée · même mire synthétique' : 'Fichier local indexé'}</small></span>



              {#if item.id === media.id}



                <Icon name="check" size={18} />



              {/if}



            </button>



          {/each}



        </div>



        {/if}



      {:else if menu === 'speed'}



        <div role="group" aria-label="Vitesses de lecture">



          {#each [0.5, 0.75, 1, 1.25, 1.5, 2] as speed (speed)}



            <button



              data-speed={speed}



              role="menuitemradio"



              tabindex={playback.speed === speed ? 0 : -1}



              aria-checked={playback.speed === speed}



              disabled={busy}



              onclick={() => choose(() => onSpeed(speed))}



            >



              <span>{speed === 1 ? 'Normale · 1×' : `${speed.toLocaleString('fr')}×`}</span>



              {#if playback.speed === speed}



                <Icon name="check" size={18} />



              {/if}



            </button>



          {/each}



        </div>



      {:else if menu === 'settings'}



        <div role="group" aria-label="Paramètres du lecteur">



          <button



            type="button"



            role="menuitem"



            onclick={() => {



              const next = aspectRatio === 'contain' ? 'cover' : aspectRatio === 'cover' ? 'fill' : 'contain';



              onAspectRatio?.(next);



            }}



          >



            <span>Format d'image</span>



            <span class="setting-value">{aspectRatio === 'cover' ? 'Remplir' : aspectRatio === 'fill' ? 'Étirer' : 'Normal'}</span>



          </button>







          <button



            type="button"



            role="menuitem"



            onclick={() => menu = 'speed'}



          >



            <span>Vitesse de lecture</span>



            <span class="setting-value">{playback.speed === 1 ? '1x' : `${playback.speed.toLocaleString('fr')}x`}</span>



          </button>







          <button



            type="button"



            role="menuitem"



            onclick={() => onRepeat?.(!repeat)}



          >



            <span>Mode de répétition</span>



            <span class="setting-value">{repeat ? 'Répéter' : 'Aucun'}</span>



          </button>







          <button



            type="button"



            role="menuitem"



            onclick={() => {



              closeMenu();



              onStats?.();



            }}



          >



            <span>Données de lecture</span>



            <span class="setting-arrow">›</span>



          </button>



        </div>



      {/if}



    </div>



  {/if}







    {#if canSkipRecap}
    <div class="skip-marker-overlay">
      <button type="button" class="skip-marker-btn" onclick={() => { onSeek(media.recap!.end); onWake(); }}>
        <span>Passer le résumé</span>
        <span class="skip-marker-arrow">&rsaquo;</span>
      </button>
    </div>
  {:else if canSkipIntro}
    <div class="skip-marker-overlay">
      <button type="button" class="skip-marker-btn" onclick={() => { onSeek(media.intro!.end); onWake(); }}>
        <span>Passer l'introduction</span>
        <span class="skip-marker-arrow">&rsaquo;</span>
      </button>
    </div>
  {:else if canSkipCredits}
    <div class="skip-marker-overlay">
      <button type="button" class="skip-marker-btn" onclick={() => { onSeek(media.credits!.end); onWake(); }}>
        <span>Passer le générique</span>
        <span class="skip-marker-arrow">&rsaquo;</span>
      </button>
    </div>
  {:else if canSkipPreview}
    <div class="skip-marker-overlay">
      <button type="button" class="skip-marker-btn" onclick={() => { onSeek(media.preview!.end); onWake(); }}>
        <span>Passer l'aperçu</span>
        <span class="skip-marker-arrow">&rsaquo;</span>
      </button>
    </div>
  {/if}

<div class="dock">
  <div class="timeline-row">
  <span class="bar-time" aria-hidden="true">{clock(playback.position)}</span>
  <div class="progress-line">
    {#if timelineChapters.length}
      <div class="chapter-ticks" aria-hidden="true">
        {#each timelineChapters as chapter, index (`${chapter.start}-${index}`)}
          <span class="chapter-tick" style:left={`${chapter.start / playback.duration * 100}%`}></span>
        {/each}
      </div>
    {/if}
    {#if playback.duration > 0}
      <div class="timeline-markers-track" aria-hidden="true">
        {#if media.recap}
          <div class="marker-segment recap" style:left={`${Math.max(0, (media.recap.start / playback.duration) * 100)}%`} style:width={`${Math.max(0.5, ((media.recap.end - media.recap.start) / playback.duration) * 100)}%`} title="Résumé"></div>
        {/if}
        {#if media.intro}
          <div class="marker-segment intro" style:left={`${Math.max(0, (media.intro.start / playback.duration) * 100)}%`} style:width={`${Math.max(0.5, ((media.intro.end - media.intro.start) / playback.duration) * 100)}%`} title="Introduction"></div>
        {/if}
        {#if media.credits}
          <div class="marker-segment credits" style:left={`${Math.max(0, (media.credits.start / playback.duration) * 100)}%`} style:width={`${Math.max(0.5, ((media.credits.end - media.credits.start) / playback.duration) * 100)}%`} title="Générique de fin"></div>
        {/if}
        {#if media.preview}
          <div class="marker-segment preview" style:left={`${Math.max(0, (media.preview.start / playback.duration) * 100)}%`} style:width={`${Math.max(0.5, ((media.preview.end - media.preview.start) / playback.duration) * 100)}%`} title="Aperçu"></div>
        {/if}
      </div>
    {/if}



    {#if hoverTime !== null}



      <output class="seek-preview" aria-hidden="true" style:left={playback.duration ? Math.max(10, Math.min(90, hoverTime! / playback.duration * 100)) + '%' : '50%'}>



        {#if previewFrame}



          <span class="trickplay-frame" style:width={previewFrame.width + 'px'} style:height={previewFrame.height + 'px'}>



            <img src={previewFrame.url} alt="" draggable="false"



              style:width={previewFrame.fullWidth + 'px'}



              style:height={previewFrame.fullHeight + 'px'}



              style:transform={`translate(-${previewFrame.x}px, -${previewFrame.y}px)`}



              onerror={() => { if (previewFrame) previewFailedSheets = new Set([...previewFailedSheets, Math.floor(hoverTime! * 1000 / trickplay!.intervalMs / (trickplay!.rows * trickplay!.columns))]); }}



            />



          </span>



        {/if}



        {#if hoverChapter}<span class="preview-chapter">{hoverChapter.title}</span>{/if}
        <span>{clock(hoverTime)}</span>



      </output>



    {/if}



    <input



      class="seek"
      style:--seek-progress={`${playback.duration > 0 ? Math.min(100, Math.max(0, (playback.position / playback.duration) * 100)) : 0}%`}
      style:--seek-buffered={`${playback.duration > 0 ? Math.min(100, Math.max(0, (Math.max(buffered, playback.position) / playback.duration) * 100)) : 0}%`}



      aria-label="Position"
      aria-valuetext={`${clock(playback.position)}${activeChapter ? ' · ' + activeChapter.title : ''}`}



      type="range"



      min="0"



      max={playback.duration || 1}



      step="0.01"



      value={playback.position}



      disabled={busy || !playback.duration}



      onpointerdown={event => {



        isSeeking = true;



        updateHoverPosition(event.clientX, event.currentTarget);



      }}



      onpointermove={event => {



        updateHoverPosition(event.clientX, event.currentTarget);



      }}



      onpointerup={() => {



        isSeeking = false;



        hoverTime = null;



      }}



      onpointercancel={() => {



        isSeeking = false;



        hoverTime = null;



      }}



      onpointerleave={() => {



        if (!isSeeking) hoverTime = null;



      }}



      oninput={event => {



        hoverTime = Number(event.currentTarget.value);



      }}



      onchange={event => {



        isSeeking = false;



        onSeek(Number(event.currentTarget.value));



      }}



    />



  </div>







  <span class="bar-time end" aria-hidden="true">{playback.duration > 0 ? `−${clock(Math.max(0, playback.duration - playback.position))}` : clock(0)}</span>
  </div>
  <div class="control-row">
    <div class="info-controls">
      <span class="time" aria-label="{clock(playback.position)} sur {clock(playback.duration)}{projectedEndTime ? ', fin à ' + projectedEndTime : ''}">
        <span class="current-time">{clock(playback.position)}</span>
        <span class="duration-time"> / {clock(playback.duration)}</span>
        {#if playback.duration > 0 && projectedEndTime}
          <span class="remaining-time">Fin à {projectedEndTime}</span>
        {/if}
      </span>
    </div>



    <div class="left-controls">



      <button



        class="control-btn"



        aria-label={`Reculer de ${skipBackSeconds} secondes`}



        title={`Reculer de ${skipBackSeconds} secondes`}



        disabled={busy || !playback.duration}



        onclick={() => onSeek(Math.max(0, playback.position - skipBackSeconds))}



      >



        <Icon name="jump-backward" size={22} />



      </button>







      <button



        class="control-btn chapter-btn"



        aria-label="Chapitre précédent"



        title={media.chapters?.length ? 'Chapitre précédent' : 'Aucun chapitre'}



        disabled={busy || !media.chapters?.length}



        onclick={() => skipChapter(-1)}



      >



        <Icon name="chapter-prev" size={20} />



      </button>







      <button



        class="primary play-btn"



        aria-label={playback.paused ? 'Lire' : 'Pause'}



        disabled={busy}



        onclick={onPlay}



      >



        <Icon name={playback.paused ? 'play' : 'pause'} size={26} />



      </button>







      <button



        class="control-btn chapter-btn"



        aria-label="Chapitre suivant"



        title={media.chapters?.length ? 'Chapitre suivant' : 'Aucun chapitre'}



        disabled={busy || !media.chapters?.length}



        onclick={() => skipChapter(1)}



      >



        <Icon name="chapter-next" size={20} />



      </button>







      <button



        class="control-btn"



        aria-label={`Avancer de ${skipForwardSeconds} secondes`}



        title={`Avancer de ${skipForwardSeconds} secondes`}



        disabled={busy || !playback.duration}



        onclick={() => onSeek(Math.min(playback.duration, playback.position + skipForwardSeconds))}



      >



        <Icon name="jump-forward" size={22} />



      </button>











    </div>







    <div class="right-controls" bind:this={triggers}>



      <button



        data-menu="subtitles"



        aria-label="Sous-titres"



        aria-haspopup="menu"



        aria-expanded={menu === 'subtitles'}



        aria-controls={menu === 'subtitles' ? 'player-menu' : undefined}



        onclick={() => toggleMenu('subtitles')}



        disabled={busy}



      >



        <Icon name="captions" size={22} />



      </button>







      {#if selectableAudio.length > 1}



      <button



        data-menu="audio"



        aria-label="Audio"



        aria-haspopup="menu"



        aria-expanded={menu === 'audio'}



        aria-controls={menu === 'audio' ? 'player-menu' : undefined}



        onclick={() => toggleMenu('audio')}



        disabled={busy}



      >



        <Icon name="globe" size={20} />



      </button>



      {/if}







      <div class="volume-group">



        <button



          class="mute"



          aria-label={playback.volume === 0 ? 'Rétablir le son' : 'Couper le son'}



          aria-pressed={playback.volume === 0}



          onclick={onMute}



        >



          <Icon name={playback.volume === 0 ? 'muted' : 'volume'} size={22} />



        </button>



        <input



          class="volume"



          aria-label="Volume"



          type="range"



          min="0"



          max="1"



          step="0.01"



          value={playback.volume}
          style:--volume-level={`${Math.round(playback.volume * 100)}%`}



          oninput={event => onVolume(Number(event.currentTarget.value))}



        />



      </div>







      <button



        data-menu="settings"



        aria-label="Paramètres"



        aria-haspopup="menu"



        aria-expanded={menu === 'settings'}



        aria-controls={menu === 'settings' ? 'player-menu' : undefined}



        onclick={() => toggleMenu('settings')}



        disabled={busy}



      >



        <Icon name="settings" size={20} />



      </button>







      <button



        aria-label={fullscreen ? 'Quitter le plein écran' : 'Plein écran'}



        onclick={onFullscreen}



      >



        <Icon name={fullscreen ? 'minimize' : 'fullscreen'} size={22} />



      </button>



    </div>



  </div>



</div>
</div>







<style>
  .chapter-ticks { position:absolute;inset:0;pointer-events:none;z-index:3; }
  .chapter-tick { position:absolute;top:50%;width:2px;height:10px;transform:translate(-1px,-50%);background:rgba(255,255,255,.85);border-radius:1px; }
  .preview-chapter { display:block;max-width:220px;white-space:normal;text-align:center;overflow-wrap:anywhere; }




  .offset-control {



    display: flex;



    align-items: center;



    gap: 12px;



    padding: 10px 14px;



    background: rgba(255, 255, 255, 0.03);



    border-radius: var(--radius-xs);



    font-size: 12px;



    color: var(--color-muted);



  }



  .offset-control input {



    width: 6.5rem;



    color: var(--color-text-bright);



    background: var(--color-raised);



    border: 1px solid var(--color-line);



    border-radius: var(--radius-xs);



    padding: 6px 10px;



    font-size: 13px;



  }



  .offset-control input:focus {



    outline: none;



    border-color: var(--color-accent);



  }



  .player-controls {



    position: absolute;



    z-index: 4;



    bottom: 0;



    left: 0;



    right: 0;



    padding: 120px max(36px, env(safe-area-inset-right)) max(26px, env(safe-area-inset-bottom)) max(36px, env(safe-area-inset-left));



    background: linear-gradient(180deg, transparent 0%, rgba(9, 10, 16, 0.75) 50%, rgba(9, 10, 16, 0.98) 100%);



    transition: opacity 0.25s cubic-bezier(0.4, 0, 0.2, 1);



  }



  .concealed {



    opacity: 0;



    pointer-events: none;



  }







  .control-row, .left-controls, .right-controls {



    display: flex;



    align-items: center;



  }



  .control-row {



    justify-content: space-between;



    gap: 16px;



  }



  .left-controls {



    gap: 10px;



  }



  .right-controls {



    gap: 10px;



  }







  button {



    display: inline-flex;



    align-items: center;



    justify-content: center;



    gap: 8px;



    min-width: 44px;



    min-height: 44px;



    padding: 8px 12px;



    background: transparent;



    color: #e2e8f0;



    border: 1px solid transparent;



    border-radius: var(--radius-sm);



    transition: all var(--transition-fast);



  }



  button:hover:not(:disabled) {



    background: rgba(255, 255, 255, 0.12);



    color: #ffffff;



    border-color: rgba(255, 255, 255, 0.15);



  }



  button[aria-expanded='true'] {



    color: var(--color-accent);



    background: var(--color-accent-soft);



    border-color: var(--color-accent-soft);



  }







  .control-btn {



    display: inline-flex;



    align-items: center;



    justify-content: center;



    width: 40px;



    height: 40px;



    min-width: 40px;



    min-height: 40px;



    padding: 0;



    border-radius: 50%;



    color: #e2e8f0;



    background: transparent;



    border: 1px solid transparent;



    transition: all var(--transition-fast);



  }



  .control-btn:hover:not(:disabled) {



    background: rgba(255, 255, 255, 0.12);



    color: #ffffff;



    border-color: rgba(255, 255, 255, 0.15);



    transform: scale(1.08);



  }



  .control-btn:disabled {



    opacity: 0.3;



    cursor: not-allowed;



  }



  .control-btn :global(svg) {



    stroke-width: 2;



  }



  .chapter-btn :global(svg) {



    fill: currentColor;



    stroke: none;



  }







  .primary.play-btn {



    width: 52px;



    height: 52px;



    min-width: 52px;



    min-height: 52px;



    border-radius: 50%;



    background: #ffffff;



    color: #090a10;



    margin: 0 4px;



    border: 1px solid transparent;



    box-shadow: 0 4px 18px rgba(0, 0, 0, 0.5), 0 0 16px var(--color-accent-glow);



    transition: all var(--transition-fast);



  }



  .primary.play-btn:hover:not(:disabled) {



    background: #ffffff;



    color: #090a10;



    border-color: transparent;



    transform: scale(1.08);



    box-shadow: 0 6px 24px rgba(255, 255, 255, 0.35), 0 0 24px var(--color-accent-glow);



  }



  .primary.play-btn:active:not(:disabled) {



    transform: scale(0.96);



  }



  .primary.play-btn :global(svg) {



    fill: currentColor;



    stroke-width: 1.8;



  }







  .volume-group {



    display: flex;



    align-items: center;



    gap: 6px;



    background: rgba(255, 255, 255, 0.04);



    padding: 2px 10px 2px 2px;



    border-radius: var(--radius-full);



    border: 1px solid var(--color-line);



  }



  .volume {



    width: 85px;



    accent-color: var(--color-accent);



  }







  .time {



    margin-left: 8px;



    font-size: 13px;



    font-variant-numeric: tabular-nums;



    color: var(--color-text-bright);



    white-space: nowrap;



    font-weight: 500;



  }



  .duration-time {



    color: var(--color-muted);



  }



  .remaining-time {



    color: var(--color-subtle);



    font-size: 11.5px;



  }







  .progress-line {



    position: relative;



    margin-bottom: 16px;



  }



  .seek {



    width: 100%;



    height: 28px;



    margin: 0;



    accent-color: var(--color-accent);



    cursor: pointer;



  }



  .seek-preview {



    position: absolute;



    left: 50%;



    transform: translateX(-50%);



    bottom: 100%;



    display: grid;



    gap: 4px;



    justify-items: center;



    pointer-events: none;



    font-size: 11.5px;



    font-weight: 600;



    font-variant-numeric: tabular-nums;



    background: rgba(17, 19, 28, 0.95);



    backdrop-filter: blur(8px);



    -webkit-backdrop-filter: blur(8px);



    padding: 4px 10px;



    border-radius: var(--radius-xs);



    border: 1px solid var(--color-line-strong);



    color: var(--color-text-bright);



    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.5);



  }







  .trickplay-frame {



    display: block;



    overflow: hidden;



    position: relative;



    border-radius: var(--radius-xs);



    max-width: min(70vw, 320px);



    max-height: 180px;



  }



  .trickplay-frame img { position: absolute; top: 0; left: 0; max-width: none; }



  .floating-menu {



    position: absolute;



    bottom: calc(100% - 90px);



    right: 36px;



    width: 360px;



    max-height: min(480px, calc(100dvh - 200px));



    overflow-y: auto;



    padding: 10px;



    border-radius: var(--radius-lg);



    background: rgba(18, 20, 30, 0.96);



    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.85);



    border: 1px solid rgba(255, 255, 255, 0.12);



    backdrop-filter: blur(24px);



    -webkit-backdrop-filter: blur(24px);



  }



  .menu-heading {



    display: flex;



    align-items: center;



    justify-content: space-between;



    padding: 6px 10px 10px 14px;



    font-size: 14px;



    font-weight: 700;



    color: var(--color-text-bright);



    border-bottom: 1px solid var(--color-line);



    margin-bottom: 8px;



  }



  .menu-heading button {



    width: auto;



    min-width: 36px;



    min-height: 36px;



    padding: 4px;



    font-size: 22px;



    font-weight: 300;



    line-height: 1;



    border-radius: 50%;



  }







  .floating-menu button[role="menuitem"],



  .floating-menu button[role="menuitemradio"] {



    width: 100%;



    min-height: 48px;



    padding: 12px 14px;



    justify-content: space-between;



    text-align: left;



    font-size: 13.5px;



    font-weight: 500;



    gap: 16px;



    border-radius: var(--radius-sm);



  }



  .setting-value {



    color: var(--color-muted, #94a3b8);



    font-size: 13px;



    font-weight: 500;



  }



  .setting-arrow {



    color: var(--color-muted, #94a3b8);



    font-size: 18px;



    line-height: 1;



  }



  .menu-back {



    display: inline-flex;



    align-items: center;



    justify-content: center;



    width: 32px;



    height: 32px;



    min-width: 32px;



    min-height: 32px;



    padding: 0;



    margin-right: 8px;



    border-radius: var(--radius-xs);



    background: transparent;



    color: var(--color-text-bright);



  }



  .floating-menu button[aria-checked='true'] {



    color: var(--color-accent-hover);



    background: var(--color-accent-soft);



    border-color: var(--color-accent-soft);



    font-weight: 600;



  }



  .floating-menu small {



    display: block;



    font-size: 11px;



    color: var(--color-muted);



    margin-top: 3px;



  }







  .menu-section {



    font-size: 10.5px;



    text-transform: uppercase;



    letter-spacing: 0.12em;



    color: var(--color-accent);



    margin: 14px 14px 6px;



    font-weight: 700;



  }



  .menu-note {



    font-size: 11.5px;



    color: var(--color-muted);



    line-height: 1.5;



    margin: 10px 14px;



  }



  .track-info {



    display: flex;



    align-items: flex-start;



    gap: 12px;



    padding: 8px 14px;



    font-size: 12.5px;



    color: var(--color-text);



  }



  .track-badge {



    display: inline-block;



    background: var(--color-accent-soft);



    color: var(--color-accent);



    font-size: 10.5px;



    font-weight: 700;



    padding: 2px 7px;



    border-radius: var(--radius-xs);



    margin-left: 6px;



  }



  .menu-separator {



    border-top: 1px solid var(--color-line);



    margin: 8px 14px;



  }







  @media (max-width: 768px) {



    .player-controls {



      padding: 56px max(16px, env(safe-area-inset-right)) max(14px, env(safe-area-inset-bottom)) max(16px, env(safe-area-inset-left));



    }



    .control-row {



      flex-wrap: wrap;



      row-gap: 8px;



    }



    .left-controls, .right-controls {



      width: 100%;



      justify-content: center;



      gap: 4px;



      min-width: 0;



    }



    .left-controls { flex-wrap: wrap; }



    .control-btn { min-width: 44px; min-height: 44px; }



    .time { margin-left: 4px; }



    .remaining-time { display: none; }



    .progress-line { margin-bottom: 4px; }



    .seek { height: 44px; }



    .volume-group {



      display: none;



    }







    .floating-menu {



      right: 16px;



      bottom: calc(100% - 70px);



      width: min(340px, calc(100vw - 32px));



      box-sizing: border-box;



      max-height: max(100px, calc(100dvh - 220px));



    }



  }



  @media (max-width: 380px) {



    .chapter-btn { display: none; }



  }



  .skip-marker-overlay {



    position: absolute;



    right: 2rem;



    bottom: 5.5rem;



    z-index: 40;



    pointer-events: auto;



  }



  .skip-marker-btn {



    display: inline-flex;



    align-items: center;



    gap: 0.5rem;



    padding: 0.75rem 1.25rem;



    background: rgba(15, 23, 42, 0.85);



    backdrop-filter: blur(8px);



    border: 1px solid rgba(255, 255, 255, 0.25);



    border-radius: 9999px;



    color: #ffffff;



    font-size: 0.95rem;



    font-weight: 600;



    cursor: pointer;



    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.4);



    transition: all 0.2s cubic-bezier(0.16, 1, 0.3, 1);



  }



  .skip-marker-btn:hover {



    background: rgba(255, 255, 255, 0.2);



    border-color: rgba(255, 255, 255, 0.5);



    transform: scale(1.04);



  }



  .timeline-markers-track {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    transform: translateY(-50%);
    height: 4px;
    pointer-events: none;
    border-radius: 2px;
    overflow: hidden;
    z-index: 1;
  }

  .marker-segment {
    position: absolute;
    top: 0;
    bottom: 0;
    border-radius: 2px;
    opacity: 0.85;
    transition: opacity 0.2s ease;
  }

  .marker-segment.recap {
    background-color: #06b6d4;
  }

  .marker-segment.intro {
    background-color: #f59e0b;
  }

  .marker-segment.credits {
    background-color: #a855f7;
  }

  .marker-segment.preview {
    background-color: #10b981;
  }

  .skip-marker-arrow {



    font-size: 1.25rem;



    line-height: 1;



  }

  /* ==========================================================================
     Halo — habillage du lecteur (couche finale).
     Uniquement couleurs, rayons, fonds et typographie : la mise en page et les
     tailles responsives définies plus haut restent en vigueur.
     ========================================================================== */
  .player-controls {
    background: linear-gradient(0deg, rgb(0 0 0 / 0.78) 0%, rgb(0 0 0 / 0.42) 55%, rgb(0 0 0 / 0) 100%);
    color: #f4f6fb;
    font-family: var(--font);
    transition: opacity var(--t-slow) ease;
  }

  .control-btn {
    border: 0;
    border-radius: 16px;
    background: transparent;
    color: #f4f6fb;
    box-shadow: none;
    transition: background-color var(--t-fast) ease, color var(--t-fast) ease;
  }

  .control-btn:hover:not(:disabled),
  button[aria-expanded='true'] {
    background: rgb(255 255 255 / 0.14);
    color: #fff;
    border-color: transparent;
    transform: none;
    box-shadow: none;
  }

  .primary.play-btn,
  .primary.play-btn:hover:not(:disabled) {
    border: 0;
    border-radius: 20px;
    background: rgb(255 255 255 / 0.16);
    color: #f4f6fb;
    box-shadow: none;
  }

  .primary.play-btn:hover:not(:disabled) { background: rgb(255 255 255 / 0.26); transform: none; }
  .primary.play-btn:active:not(:disabled) { transform: scale(0.96); }

  @media (min-width: 769px) {
    .primary.play-btn { width: 60px; height: 60px; min-width: 60px; min-height: 60px; }
  }

  .time { font-weight: 700; font-variant-numeric: tabular-nums; color: #f4f6fb; }
  .duration-time, .remaining-time { color: rgb(244 246 251 / 0.65); font-weight: 600; }

  .volume { accent-color: #f4f6fb; }

  /* Barre de progression : piste fine, partie lue en accent, curseur blanc */
  .seek {
    -webkit-appearance: none;
    appearance: none;
    background: transparent;
  }
  .seek::-webkit-slider-runnable-track {
    height: 5px;
    border-radius: 5px;
    background: linear-gradient(90deg, var(--ac) var(--seek-progress, 0%), rgb(255 255 255 / 0.24) var(--seek-progress, 0%));
  }
  .seek::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -5.5px;
    border: 0;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 0 0 4px rgb(255 255 255 / 0.18);
    transition: transform var(--t-fast) var(--ease);
  }
  .seek:hover::-webkit-slider-thumb, .seek:focus-visible::-webkit-slider-thumb { transform: scale(1.15); }
  .seek::-moz-range-track { height: 5px; border-radius: 5px; background: rgb(255 255 255 / 0.24); }
  .seek::-moz-range-progress { height: 5px; border-radius: 5px; background: var(--ac); }
  .seek::-moz-range-thumb { width: 16px; height: 16px; border: 0; border-radius: 50%; background: #fff; box-shadow: 0 0 0 4px rgb(255 255 255 / 0.18); }
  .seek:focus-visible { outline: none; }
  .seek:focus-visible::-webkit-slider-runnable-track { outline: 2px solid var(--ac); outline-offset: 4px; }

  .timeline-markers-track { height: 5px; border-radius: 5px; }
  .marker-segment, .marker-segment.recap, .marker-segment.preview { background-color: rgb(255 255 255 / 0.22); opacity: 1; border-radius: 0; }
  .marker-segment.intro, .marker-segment.credits { background-color: rgb(255 255 255 / 0.34); }
  .chapter-tick { width: 3px; height: 7px; transform: translate(-1.5px, -50%); background: rgb(0 0 0 / 0.55); border-radius: 0; }

  /* Aperçu au survol (miniatures Trickplay) */
  .seek-preview {
    gap: 6px;
    padding: 0;
    border: 0;
    background: transparent;
    box-shadow: none;
    backdrop-filter: none;
    -webkit-backdrop-filter: none;
    color: #fff;
    font-size: 13px;
    font-weight: 700;
  }
  .seek-preview > span { padding: 3px 8px; border-radius: 8px; background: rgb(0 0 0 / 0.6); }
  .trickplay-frame { border-radius: 12px; border: 2px solid rgb(255 255 255 / 0.9); box-shadow: 0 10px 30px rgb(0 0 0 / 0.5); }

  /* Panneau de réglages (audio, sous-titres, version, vitesse, réglages) */
  .floating-menu {
    padding: 10px;
    border: 0;
    border-radius: 24px;
    background: rgb(17 21 31 / 0.92);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.5);
    color: #eef1f8;
    font-family: var(--font);
  }
  .menu-heading { font-size: 17px; font-weight: 800; letter-spacing: -0.01em; color: #fff; }
  .menu-heading button, .menu-close, .menu-back {
    border: 0;
    border-radius: 14px;
    background: transparent;
    color: #eef1f8;
  }
  .menu-heading button:hover:not(:disabled), .menu-close:hover, .menu-back:hover:not(:disabled) { background: rgb(255 255 255 / 0.1); }
  .floating-menu button[role='menuitemradio'],
  .floating-menu button[role='menuitem'] {
    border: 0;
    border-radius: 14px;
    background: transparent;
    color: #eef1f8;
    font-weight: 700;
  }
  .floating-menu button[role='menuitemradio']:hover:not(:disabled),
  .floating-menu button[role='menuitem']:hover:not(:disabled) { background: rgb(255 255 255 / 0.07); }
  .floating-menu button[aria-checked='true'] {
    background: rgb(255 255 255 / 0.09);
    border-color: transparent;
    color: #fff;
    box-shadow: inset 3px 0 0 var(--ac);
  }
  .floating-menu small, .setting-value, .menu-note { color: #a2a9ba; }
  .setting-arrow { color: #8189a0; }
  .menu-section {
    color: #8189a0;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }
  .menu-separator { background: rgb(255 255 255 / 0.09); }
  .track-badge {
    border: 0;
    border-radius: 8px;
    background: rgb(255 255 255 / 0.12);
    color: #eef1f8;
    font-weight: 700;
  }
  .offset-control input {
    border: 1px solid transparent;
    border-radius: 12px;
    background: rgb(255 255 255 / 0.07);
    color: #fff;
  }
  .offset-control input:focus { border-color: var(--ac); }

  /* Passer l'intro / le générique : bouton clair, lisible sur toutes les images */
  .skip-marker-btn {
    gap: 10px;
    min-height: 52px;
    padding: 0 22px;
    border: 0;
    border-radius: 16px;
    background: rgb(255 255 255 / 0.92);
    backdrop-filter: none;
    color: #0b0e18;
    font-size: 15px;
    font-weight: 800;
    box-shadow: 0 10px 30px rgb(0 0 0 / 0.4);
  }
  .skip-marker-btn:hover { background: #fff; border-color: transparent; transform: none; }
  .skip-marker-btn:active { transform: scale(0.98); }
  .skip-marker-arrow { color: #0b0e18; }

  /* Halo : panneau latéral à onglets (ordinateur), feuille basse sur téléphone */
  .menu-tabs { display: flex; gap: 4px; margin: 2px 0 10px; padding: 4px; border-radius: 999px; background: rgb(255 255 255 / 0.06); overflow-x: auto; scrollbar-width: none; }
  .menu-tabs::-webkit-scrollbar { display: none; }
  .floating-menu .menu-tabs button { flex: none; min-height: 34px; height: 34px; padding: 0 12px; border: 0; border-radius: 999px; background: transparent; color: #a2a9ba; font-size: 13px; font-weight: 700; white-space: nowrap; box-shadow: none; }
  .floating-menu .menu-tabs button:hover:not(:disabled) { background: rgb(255 255 255 / 0.08); color: #fff; }
  .floating-menu .menu-tabs button[aria-current='true'] { background: #eef1f8; color: #0b0e18; box-shadow: none; }
  @media (min-width: 769px) {
    .floating-menu {
      position: fixed;
      top: 16px;
      right: 16px;
      bottom: 16px;
      left: auto;
      width: 380px;
      max-height: none;
    }
  }

  /* ==========================================================================
     Dock « verre liquide » : timeline + commandes dans une même capsule vitrée,
     dans la lignée du panneau de réglages. Couche finale, après Halo.
     ========================================================================== */
  .player-controls {
    padding: 140px max(28px, env(safe-area-inset-right)) max(22px, env(safe-area-inset-bottom)) max(28px, env(safe-area-inset-left));
    background: linear-gradient(0deg, rgb(0 0 0 / 0.6) 0%, rgb(0 0 0 / 0.2) 55%, rgb(0 0 0 / 0) 100%);
  }
  .dock {
    max-width: 1180px;
    margin: 0 auto;
    padding: 12px 16px 10px;
    border-radius: 30px;
    background:
      linear-gradient(180deg, rgb(255 255 255 / 0.1) 0%, rgb(255 255 255 / 0.03) 100%),
      rgb(16 18 26 / 0.42);
    backdrop-filter: blur(28px) saturate(180%);
    -webkit-backdrop-filter: blur(28px) saturate(180%);
    border: 1px solid rgb(255 255 255 / 0.13);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.2),
      inset 0 -1px 0 rgb(255 255 255 / 0.04),
      0 24px 60px rgb(0 0 0 / 0.45);
    transition: transform var(--t-slow) var(--ease);
  }
  .concealed .dock { transform: translateY(14px); }
  @supports not ((backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px))) {
    .dock { background: rgb(16 18 26 / 0.88); }
  }

  .timeline-row { display: flex; align-items: center; gap: 14px; padding: 0 4px; }
  .timeline-row .progress-line { flex: 1; min-width: 0; margin-bottom: 0; }
  .bar-time {
    flex: none;
    min-width: 56px;
    font-size: 13px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    color: #f4f6fb;
  }
  .bar-time.end { text-align: right; color: rgb(244 246 251 / 0.68); }

  /* Piste fine qui s'épaissit au survol, partie lue en accent */
  .progress-line { --track-h: 4px; --thumb: 14px; }
  .progress-line:hover, .progress-line:focus-within { --track-h: 7px; }
  .seek { height: 26px; }
  .seek::-webkit-slider-runnable-track {
    height: var(--track-h);
    border-radius: 999px;
    background: linear-gradient(90deg, var(--ac) var(--seek-progress, 0%), rgb(255 255 255 / 0.2) var(--seek-progress, 0%));
    transition: height var(--t-fast) var(--ease);
  }
  .seek::-webkit-slider-thumb {
    width: var(--thumb);
    height: var(--thumb);
    margin-top: calc((var(--track-h) - var(--thumb)) / 2);
    box-shadow: 0 0 0 5px rgb(255 255 255 / 0.16), 0 2px 8px rgb(0 0 0 / 0.4);
    transform: scale(0.85);
  }
  .seek:hover::-webkit-slider-thumb, .seek:focus-visible::-webkit-slider-thumb, .seek:active::-webkit-slider-thumb { transform: scale(1.15); }
  .seek::-moz-range-track { height: var(--track-h); border-radius: 999px; background: rgb(255 255 255 / 0.2); transition: height var(--t-fast) var(--ease); }
  .seek::-moz-range-progress { height: var(--track-h); border-radius: 999px; background: var(--ac); }
  .seek::-moz-range-thumb { width: var(--thumb); height: var(--thumb); box-shadow: 0 0 0 5px rgb(255 255 255 / 0.16), 0 2px 8px rgb(0 0 0 / 0.4); }
  .timeline-markers-track { height: var(--track-h); border-radius: 999px; transition: height var(--t-fast) var(--ease); }
  .chapter-tick { height: calc(var(--track-h) + 2px); }
  .seek-preview { bottom: calc(100% + 10px); }
  .seek-preview > span {
    padding: 4px 10px;
    border-radius: 10px;
    background: rgb(16 18 26 / 0.6);
    backdrop-filter: blur(16px) saturate(160%);
    -webkit-backdrop-filter: blur(16px) saturate(160%);
    border: 1px solid rgb(255 255 255 / 0.12);
  }
  .trickplay-frame { border-radius: 14px; border: 1px solid rgb(255 255 255 / 0.5); }

  /* Commandes : infos à gauche, transport centré, options à droite */
  .control-row { display: grid; grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr); gap: 12px; margin-top: 2px; }
  .info-controls { display: flex; align-items: center; min-width: 0; padding-left: 4px; }
  .info-controls .time { margin: 0; }
  .info-controls .current-time, .info-controls .duration-time { display: none; }
  .info-controls .remaining-time { font-size: 12.5px; font-weight: 600; color: rgb(244 246 251 / 0.62); }
  .left-controls { justify-content: center; gap: 6px; }
  .right-controls { justify-content: flex-end; gap: 4px; min-width: 0; }
  .control-btn { width: 44px; height: 44px; min-width: 44px; min-height: 44px; border-radius: 50%; }
  .right-controls > button { border: 0; border-radius: 50%; width: 44px; padding: 0; }
  .control-btn:hover:not(:disabled), .right-controls > button:hover:not(:disabled), .volume-group .mute:hover {
    background: rgb(255 255 255 / 0.14);
    border-color: transparent;
  }
  .primary.play-btn,
  .primary.play-btn:hover:not(:disabled) {
    border-radius: 50%;
    background: rgb(248 249 252 / 0.94);
    color: #0b0e18;
    box-shadow: 0 6px 20px rgb(0 0 0 / 0.35), inset 0 1px 0 rgb(255 255 255 / 0.9);
  }
  .primary.play-btn:hover:not(:disabled) { background: #fff; transform: scale(1.05); }
  .primary.play-btn:active:not(:disabled) { transform: scale(0.95); }
  @media (min-width: 769px) {
    .primary.play-btn { width: 56px; height: 56px; min-width: 56px; min-height: 56px; }
  }
  .volume-group { gap: 2px; padding: 0 12px 0 0; border: 0; border-radius: 999px; background: rgb(255 255 255 / 0.06); }
  .volume-group .mute { border: 0; border-radius: 50%; width: 44px; padding: 0; }
  .volume {
    -webkit-appearance: none;
    appearance: none;
    width: 84px;
    height: 20px;
    background: transparent;
  }
  .volume::-webkit-slider-runnable-track { height: 4px; border-radius: 999px; background: linear-gradient(90deg, #fff var(--volume-level), rgb(255 255 255 / 0.28) var(--volume-level)); }
  .volume::-webkit-slider-thumb { -webkit-appearance: none; width: 12px; height: 12px; margin-top: -4px; border-radius: 50%; background: #fff; box-shadow: 0 1px 4px rgb(0 0 0 / 0.4); }
  .volume::-moz-range-track { height: 4px; border-radius: 999px; background: rgb(255 255 255 / 0.28); }
  .volume::-moz-range-progress { height: 4px; border-radius: 999px; background: #fff; }
  .volume::-moz-range-thumb { width: 12px; height: 12px; border: 0; border-radius: 50%; background: #fff; }

  @media (max-width: 768px) {
    .player-controls { padding: 64px max(10px, env(safe-area-inset-right)) max(10px, env(safe-area-inset-bottom)) max(10px, env(safe-area-inset-left)); }
    .dock { padding: 8px 10px; border-radius: 24px; }
    .timeline-row { gap: 10px; padding: 0 2px; }
    .bar-time { min-width: 44px; font-size: 12px; }
    .seek { height: 40px; }
    .control-row { grid-template-columns: minmax(0, 1fr); row-gap: 2px; }
    .info-controls { display: none; }
    .right-controls { justify-content: center; }
  }
  @media (prefers-reduced-motion: reduce) {
    .dock, .concealed .dock { transition: none; transform: none; }
  }

  /* Zone déjà chargée : entre la partie lue et le reste de la piste */
  .seek::-webkit-slider-runnable-track {
    background: linear-gradient(90deg,
      var(--ac) var(--seek-progress, 0%),
      rgb(255 255 255 / 0.42) var(--seek-progress, 0%),
      rgb(255 255 255 / 0.42) var(--seek-buffered, 0%),
      rgb(255 255 255 / 0.16) var(--seek-buffered, 0%));
  }
  .seek::-moz-range-track {
    background: linear-gradient(90deg, rgb(255 255 255 / 0.42) var(--seek-buffered, 0%), rgb(255 255 255 / 0.16) var(--seek-buffered, 0%));
  }
  /* Boutons « Passer… » : juste au-dessus du dock vitré */
  .skip-marker-overlay { bottom: calc(100% - 124px); right: max(28px, env(safe-area-inset-right)); }
  @media (max-width: 768px) {
    .skip-marker-overlay { bottom: calc(100% - 56px); right: max(12px, env(safe-area-inset-right)); }
  }
</style>



