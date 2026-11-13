<script lang="ts">
  import Icon from '../ui/Icon.svelte';
  import { audioLabel, videoLabel, videoResolutionBadge, containerLabel } from './track-labels';
  import { audioChoices } from './available-audio';
  import { SPEEDS, aspectRatioLabel, nextAspectRatio, speedOptionLabel, speedSettingLabel } from './controls-format';
  import { focusMenuItem, menuItems, nextMenuIndex } from './menu-navigation';
  import { LADDER, type QualityChoice } from './transcode';
  import { detectClientTranscodeCapabilities } from './client-transcode';
  import { AUDIO_ENHANCEMENTS, type AudioEnhancement } from './audio-enhancement';
  import { SLEEP_MINUTES, sleepLabel } from './sleep-timer';
  import { t } from '../i18n';
  import type { ControlsProps, Menu } from './controls-props';
  import { DEFAULT_SUBTITLE_STYLE, sanitizeSubtitleStyle, type SubtitleStyle } from './subtitle-style';

  let { playback, media, catalogue, audio, subtitle, busy, menu = $bindable(), onAudio, onSubtitle, onSource, onSpeed, audioOffset, subtitleOffset, onAudioOffset, onSubtitleOffset, subtitleStyle, onSubtitleStyle, aspectRatio, onAspectRatio, repeat, onRepeat, onStats, quality, maxKbps, onQuality, onMaxKbps, transcode, enhancement, onEnhancement, sleep, onSleep, selectableAudio, title, closeMenu, toggleMenu }: { playback: ControlsProps['state']; media: ControlsProps['media']; catalogue: ControlsProps['catalogue']; audio: ControlsProps['audio']; subtitle: ControlsProps['subtitle']; busy: ControlsProps['busy']; menu: Menu; onAudio: ControlsProps['onAudio']; onSubtitle: ControlsProps['onSubtitle']; onSource: ControlsProps['onSource']; onSpeed: ControlsProps['onSpeed']; audioOffset: number; subtitleOffset: number; onAudioOffset: ControlsProps['onAudioOffset']; onSubtitleOffset: ControlsProps['onSubtitleOffset']; subtitleStyle?: SubtitleStyle; onSubtitleStyle?: ControlsProps['onSubtitleStyle']; aspectRatio: NonNullable<ControlsProps['aspectRatio']>; onAspectRatio: ControlsProps['onAspectRatio']; repeat: boolean; onRepeat: ControlsProps['onRepeat']; onStats: ControlsProps['onStats']; quality: QualityChoice; maxKbps: number; onQuality: ControlsProps['onQuality']; onMaxKbps: ControlsProps['onMaxKbps']; transcode: NonNullable<ControlsProps['transcode']> | null; enhancement: AudioEnhancement; onEnhancement: ControlsProps['onEnhancement']; sleep: number | 'end' | null; onSleep: ControlsProps['onSleep']; selectableAudio: ReturnType<typeof audioChoices>; title: string; closeMenu: () => void; toggleMenu: (value: Menu) => void } = $props();

  const resolvedSubStyle = $derived<Required<SubtitleStyle>>({ ...DEFAULT_SUBTITLE_STYLE, ...sanitizeSubtitleStyle(subtitleStyle) });
  function patchSubStyle<K extends keyof SubtitleStyle>(key: K, value: SubtitleStyle[K]) {
    onSubtitleStyle?.({ ...resolvedSubStyle, [key]: value });
  }

  /** Bit rate ceilings offered for remote viewing, kbit/s (0: unlimited). */
  const BITRATES = [0, 40000, 20000, 10000, 8000, 4000, 2000, 1000];

  let panel = $state<HTMLDivElement>();

  const source = $derived(media.sources.find(source => source.audioTrackId === audio) ?? media.sources[0]);

  const selectableVersions = $derived(catalogue.filter(item => item.available !== false && item.nodeStatus !== 'revoked' && item.nodeStatus !== 'disconnected'));

  const videoTracks = $derived((source?.tracks?.some(track => track.codec_type === 'video') ? source.tracks : media.sources[0]?.tracks)?.filter(track => track.codec_type === 'video') ?? []);

  const sourceHeight = $derived(media.transcode?.height ?? videoTracks[0]?.height ?? 0);
  const canVideoTranscode = $derived(Boolean(media.transcode && (media.transcode.video !== false || detectClientTranscodeCapabilities().video)));
  // Heights above the original are never offered: transcoding only ever scales down.
  const qualityHeights = $derived(canVideoTranscode ? LADDER.map(item => item.height).filter(height => height < Math.max(sourceHeight, 361)) : []);
  const qualityOptions = $derived<QualityChoice[]>(['auto', 'original', ...qualityHeights as QualityChoice[]]);
  const enhancementLabel = (value: AudioEnhancement) => value === 'night' ? $t('night_mode') : value === 'normalize' ? $t('volume_levelling') : $t('none');
  const bitrateLabel = (kbps: number) => kbps === 0 ? $t('bitrate_unlimited') : $t('bitrate_mbps', { value: kbps / 1000 });
  const sleepSetting = $derived(sleep === null ? $t('none') : sleep === 'end' ? $t('sleep_end_of_media') : sleepLabel(sleep));
  const nudge = (value: number, step: number) => Math.round(Math.max(-30, Math.min(30, value + step)) * 100) / 100;

  const menuTabs = $derived(([
    ...(selectableAudio.length > 1 ? [{ id: 'audio', label: $t('audio') }] : []),
    { id: 'subtitles', label: $t('subtitles') },
    { id: 'video', label: $t('version') },
    ...(canVideoTranscode ? [{ id: 'quality', label: $t('quality') }] : []),
    { id: 'speed', label: $t('speed') },
    { id: 'settings', label: $t('player_settings') }
  ]) as { id: Exclude<Menu, null>; label: string }[]);

  /** Focus the checked entry of the open panel (first entry when none is checked). */
  export function focusSelected() {
    const items = menuItems(panel);
    const target = panel?.querySelector<HTMLButtonElement>('[role="menuitemradio"][aria-checked="true"]:not(:disabled)') ?? items[0];
    if (target) focusMenuItem(items, target);
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
    const items = menuItems(panel);
    if (items.length === 0) return;
    const next = nextMenuIndex(event.key, items.indexOf(document.activeElement as HTMLButtonElement), items.length);
    if (next !== null) {
      event.preventDefault();
      event.stopPropagation();
      focusMenuItem(items, items[next]);
    }
  }

  function choose(action: () => void) {
    closeMenu();
    action();
  }
</script>

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
      {#if menu === 'speed' || menu === 'sleep'}
        <button type="button" class="menu-back" aria-label={$t('return_to_settings')} tabindex="-1" onclick={() => menu = 'settings'}>
          <Icon name="back" size={18} />
        </button>
      {/if}
      <span>{title}</span>
      <button type="button" class="menu-close" aria-label={$t('close_menu')} tabindex="-1" onclick={closeMenu}>×</button>
    </div>
    <!-- Halo : le panneau garde toutes les sections à portée, une par onglet. -->
    <div class="menu-tabs" role="group" aria-label={$t('player_panel_sections')}>
      {#each menuTabs as tab (tab.id)}
        <button type="button" role="menuitem" tabindex="-1" aria-current={menu === tab.id ? 'true' : undefined} onclick={() => { if (menu !== tab.id) void toggleMenu(tab.id); }}>{tab.label}</button>
      {/each}
    </div>
    {#if menu === 'audio'}
      <div role="group" aria-label={$t('audio_languages')}>
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
      <label class="offset-control">{$t('audio_offset')}
        <span class="offset-stepper">
          <button type="button" tabindex="-1" aria-label={$t('offset_earlier')} disabled={busy || !source?.externalAudioUrl} onclick={() => onAudioOffset?.(nudge(audioOffset, -0.1))}>−</button>
          <input aria-label={$t('audio_offset_seconds')} type="number" min="-30" max="30" step="0.05" value={audioOffset} disabled={busy || !source?.externalAudioUrl} oninput={(e) => { const v = e.currentTarget.valueAsNumber; if (Number.isFinite(v)) onAudioOffset?.(v); }} onchange={(e) => { const v = e.currentTarget.valueAsNumber; if (Number.isFinite(v)) onAudioOffset?.(v); }} />
          <button type="button" tabindex="-1" aria-label={$t('offset_later')} disabled={busy || !source?.externalAudioUrl} onclick={() => onAudioOffset?.(nudge(audioOffset, 0.1))}>+</button>
          {#if audioOffset !== 0}
            <button type="button" class="offset-reset" tabindex="-1" disabled={busy || !source?.externalAudioUrl} onclick={() => onAudioOffset?.(0)}>{$t('reset')}</button>
          {/if}
        </span>
      </label>
      {#if !source?.externalAudioUrl}<p class="menu-note">{$t('audio_offset_separate_tracks_only')}</p>{/if}
      <p class="menu-note">{$t('positive_delays')}</p>
    {:else if menu === 'subtitles'}
      <div role="group" aria-label={$t('subtitle_tracks')}>
        <button
          data-subtitle-id="off"
          role="menuitemradio"
          tabindex={subtitle === null ? 0 : -1}
          aria-checked={subtitle === null}
          disabled={busy}
          onclick={() => choose(() => onSubtitle(null))}
        >
          <span>{$t('subtitles_off')}</span>
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
            <span>{item.label}<small>{item.image ? $t('subtitle_image_format', { value: item.language.toUpperCase() }) : $t('webvtt', { value: item.language.toUpperCase() })}</small></span>
            {#if item.id === subtitle}
              <Icon name="check" size={18} />
            {/if}
          </button>
        {/each}
      </div>
      <label class="offset-control">{$t('subtitle_offset')}
        <span class="offset-stepper">
          <button type="button" tabindex="-1" aria-label={$t('offset_earlier')} disabled={busy} onclick={() => onSubtitleOffset?.(nudge(subtitleOffset, -0.1))}>−</button>
          <input aria-label={$t('subtitle_offset_seconds')} type="number" min="-30" max="30" step="0.05" value={subtitleOffset} disabled={busy} oninput={(e) => { const v = e.currentTarget.valueAsNumber; if (Number.isFinite(v)) onSubtitleOffset?.(v); }} onchange={(e) => { const v = e.currentTarget.valueAsNumber; if (Number.isFinite(v)) onSubtitleOffset?.(v); }} />
          <button type="button" tabindex="-1" aria-label={$t('offset_later')} disabled={busy} onclick={() => onSubtitleOffset?.(nudge(subtitleOffset, 0.1))}>+</button>
          {#if subtitleOffset !== 0}
            <button type="button" class="offset-reset" tabindex="-1" disabled={busy} onclick={() => onSubtitleOffset?.(0)}>{$t('reset')}</button>
          {/if}
        </span>
      </label>
      {#if onSubtitleStyle}
        <div class="subtitle-style-panel">
          <label class="style-row">
            <span>{$t('font_size')}</span>
            <select value={resolvedSubStyle.fontSize} onchange={(e) => patchSubStyle('fontSize', e.currentTarget.value as SubtitleStyle['fontSize'])}>
              <option value="small">{$t('small')}</option>
              <option value="medium">{$t('medium_default')}</option>
              <option value="large">{$t('large')}</option>
              <option value="x-large">{$t('extra_large')}</option>
            </select>
          </label>
          <label class="style-row">
            <span>{$t('text_outline_shadow')}</span>
            <select value={resolvedSubStyle.textShadow} onchange={(e) => patchSubStyle('textShadow', e.currentTarget.value as SubtitleStyle['textShadow'])}>
              <option value="drop-shadow">{$t('drop_shadow_default')}</option>
              <option value="outline">{$t('strong_black_outline')}</option>
              <option value="none">{$t('no_outline_shadow')}</option>
            </select>
          </label>
          <label class="style-range">
            <span>{$t('background_opacity', { value: resolvedSubStyle.backgroundOpacity })}</span>
            <input type="range" min="0" max="100" step="5" value={resolvedSubStyle.backgroundOpacity} oninput={(e) => patchSubStyle('backgroundOpacity', Number(e.currentTarget.value))} />
          </label>
          <label class="style-range">
            <span>{$t('subtitle_position', { value: resolvedSubStyle.position })}</span>
            <input type="range" min="0" max="40" step="2" value={resolvedSubStyle.position} oninput={(e) => patchSubStyle('position', Number(e.currentTarget.value))} />
          </label>
        </div>
      {/if}
    {:else if menu === 'video'}
      <div role="group" aria-label={$t('active_video_source')}>
        <p class="menu-section" role="presentation">{$t('active_video_track')}</p>
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
          {$t('container_decoding_handled_by_the_browser', { value: containerLabel(source?.mime) })}
        </p>
      </div>
      {#if selectableVersions.length > 1}
      <div role="separator" class="menu-separator"></div>
      <div role="group" aria-label={$t('demo_sources')}>
        <p class="menu-section" role="presentation">{$t('available_versions')}</p>
        {#each selectableVersions as item (item.id)}
          <button
            data-source-id={item.id}
            role="menuitemradio"
            tabindex={item.id === media.id ? 0 : -1}
            aria-checked={item.id === media.id}
            disabled={busy}
            onclick={() => choose(() => onSource(item.id))}
          >
            <span>{item.title}<small>{item.synthetic ? $t('prepared_source') : $t('indexed_local_file')}</small></span>
            {#if item.id === media.id}
              <Icon name="check" size={18} />
            {/if}
          </button>
        {/each}
      </div>
      {/if}
    {:else if menu === 'quality'}
      <div role="group" aria-label={$t('quality')}>
        {#each qualityOptions as choice (choice)}
          <button
            data-quality={choice}
            role="menuitemradio"
            tabindex={quality === choice ? 0 : -1}
            aria-checked={quality === choice}
            disabled={busy}
            onclick={() => choose(() => onQuality?.(choice))}
          >
            <span>{choice === 'auto' ? $t('quality_auto') : choice === 'original' ? $t('quality_original') : `${choice}p`}{#if choice === 'auto'}<small>{$t('quality_auto_note')}</small>{/if}</span>
            {#if quality === choice}<Icon name="check" size={18} />{/if}
          </button>
        {/each}
      </div>
      <div role="separator" class="menu-separator"></div>
      <div role="group" aria-label={$t('max_bitrate')}>
        <p class="menu-section" role="presentation">{$t('max_bitrate')}</p>
        {#each BITRATES as kbps (kbps)}
          <button data-bitrate={kbps} role="menuitemradio" tabindex={maxKbps === kbps ? 0 : -1} aria-checked={maxKbps === kbps} disabled={busy} onclick={() => choose(() => onMaxKbps?.(kbps))}>
            <span>{bitrateLabel(kbps)}</span>
            {#if maxKbps === kbps}<Icon name="check" size={18} />{/if}
          </button>
        {/each}
      </div>
      <p class="menu-note" role="note">
        {#if transcode}
          {transcode.height ? $t('transcoding_active', { height: transcode.height, mbps: (transcode.kbps / 1000).toFixed(1) }) : $t('transcoding_audio_only')}
          {#if transcode.encoder} · {transcode.encoder}{/if}{#if transcode.tonemap === 'on'} · {$t('hdr_to_sdr')}{:else if transcode.tonemap === 'unavailable'} · {$t('hdr_to_sdr_unavailable')}{/if}
        {:else}{$t('direct_play_active')}{/if}
      </p>
    {:else if menu === 'sleep'}
      <div role="group" aria-label={$t('sleep_timer')}>
        <button role="menuitemradio" tabindex={sleep === null ? 0 : -1} aria-checked={sleep === null} onclick={() => choose(() => onSleep?.(null))}>
          <span>{$t('none')}</span>{#if sleep === null}<Icon name="check" size={18} />{/if}
        </button>
        {#each SLEEP_MINUTES as minutes (minutes)}
          <button role="menuitemradio" tabindex="-1" aria-checked="false" onclick={() => choose(() => onSleep?.(minutes))}><span>{$t('sleep_in_minutes', { count: minutes })}</span></button>
        {/each}
        <button role="menuitemradio" tabindex={sleep === 'end' ? 0 : -1} aria-checked={sleep === 'end'} onclick={() => choose(() => onSleep?.('end'))}>
          <span>{$t('sleep_end_of_media')}</span>{#if sleep === 'end'}<Icon name="check" size={18} />{/if}
        </button>
      </div>
      {#if typeof sleep === 'number'}<p class="menu-note" role="status">{$t('sleep_remaining', { value: sleepLabel(sleep) })}</p>{/if}
    {:else if menu === 'speed'}
      <div role="group" aria-label={$t('playback_speeds')}>
        {#each SPEEDS as speed (speed)}
          <button
            data-speed={speed}
            role="menuitemradio"
            tabindex={playback.speed === speed ? 0 : -1}
            aria-checked={playback.speed === speed}
            disabled={busy}
            onclick={() => choose(() => onSpeed(speed))}
          >
            <span>{speedOptionLabel(speed)}</span>
            {#if playback.speed === speed}
              <Icon name="check" size={18} />
            {/if}
          </button>
        {/each}
      </div>
    {:else if menu === 'settings'}
      <div role="group" aria-label={$t('player_settings_group')}>
        <button
          type="button"
          role="menuitem"
          onclick={() => {
            onAspectRatio?.(nextAspectRatio(aspectRatio));
          }}
        >
          <span>{$t('aspect_ratio')}</span>
          <span class="setting-value">{aspectRatioLabel(aspectRatio)}</span>
        </button>
        <button
          type="button"
          role="menuitem"
          onclick={() => menu = 'speed'}
        >
          <span>{$t('playback_speed')}</span>
          <span class="setting-value">{speedSettingLabel(playback.speed)}</span>
        </button>
        <button
          type="button"
          role="menuitem"
          onclick={() => onEnhancement?.(AUDIO_ENHANCEMENTS[(AUDIO_ENHANCEMENTS.indexOf(enhancement) + 1) % AUDIO_ENHANCEMENTS.length])}
        >
          <span>{$t('audio_enhancement')}</span>
          <span class="setting-value">{enhancementLabel(enhancement)}</span>
        </button>
        <button type="button" role="menuitem" onclick={() => menu = 'sleep'}>
          <span>{$t('sleep_timer')}</span>
          <span class="setting-value">{sleepSetting}</span>
        </button>
        <button
          type="button"
          role="menuitem"
          onclick={() => onRepeat?.(!repeat)}
        >
          <span>{$t('repeat_mode')}</span>
          <span class="setting-value">{repeat ? $t('repeat') : $t('none')}</span>
        </button>
        <button
          type="button"
          role="menuitem"
          onclick={() => {
            closeMenu();
            onStats?.();
          }}
        >
          <span>{$t('playback_data')}</span>
          <span class="setting-arrow">›</span>
        </button>
      </div>
    {/if}
  </div>
{/if}

<style>

  /* ---------- Panneau de lecture (audio, sous-titres, version, vitesse, réglages) ---------- */
  .floating-menu {
    position: absolute;
    bottom: calc(100% - 90px);
    right: 36px;
    width: 360px;
    max-height: min(480px, calc(100dvh - 200px));
    overflow-y: auto;
    padding: 12px;
    border: 1px solid rgb(255 255 255 / 0.09);
    border-radius: 24px;
    background: rgb(17 21 31 / 0.94);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.55);
    color: var(--tx);
    font-family: var(--font);
  }

  .floating-menu button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    border: 0;
    background: transparent;
    color: var(--tx);
    font: inherit;
    cursor: pointer;
    transition: background-color var(--t-fast) ease, color var(--t-fast) ease, transform var(--t-fast) var(--ease);
  }

  .floating-menu button:disabled { opacity: 0.45; cursor: not-allowed; }
  .floating-menu button:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }

  .menu-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px 10px 14px;
    margin-bottom: 8px;
    border-bottom: 1px solid var(--line);
    font-size: 17px;
    font-weight: 800;
    letter-spacing: -0.01em;
    color: #fff;
  }

  .menu-heading button {
    width: auto;
    min-width: 36px;
    min-height: 36px;
    padding: 4px;
    border-radius: 14px;
    font-size: 22px;
    font-weight: 300;
    line-height: 1;
    color: var(--tx);
  }

  .menu-heading button:hover:not(:disabled) { background: var(--pn-2); }

  .menu-back {
    width: 32px;
    height: 32px;
    min-width: 32px;
    min-height: 32px;
    padding: 0;
    margin-right: 8px;
  }

  .floating-menu button[role='menuitem'],
  .floating-menu button[role='menuitemradio'] {
    width: 100%;
    min-height: 48px;
    padding: 12px 14px;
    justify-content: space-between;
    gap: 16px;
    border-radius: 14px;
    text-align: left;
    font-size: 13.5px;
    font-weight: 700;
    color: var(--tx);
  }

  .floating-menu button[role='menuitem']:hover:not(:disabled),
  .floating-menu button[role='menuitemradio']:hover:not(:disabled) { background: rgb(255 255 255 / 0.07); }

  .floating-menu button[aria-checked='true'] {
    background: var(--pn-2);
    color: #fff;
    box-shadow: inset 3px 0 0 var(--ac);
  }

  .floating-menu small {
    display: block;
    margin-top: 3px;
    font-size: 11px;
    color: var(--mu);
  }

  .setting-value { color: var(--mu); font-size: 13px; font-weight: 500; }

  .setting-arrow { color: var(--mu-2); font-size: 18px; line-height: 1; }

  .menu-section {
    margin: 14px 14px 6px;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    color: var(--mu-2);
  }

  .menu-note {
    margin: 10px 14px;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--mu);
  }

  .track-info {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 8px 14px;
    font-size: 12.5px;
    color: var(--tx);
  }

  .track-badge {
    display: inline-block;
    margin-left: 6px;
    padding: 2px 7px;
    border-radius: var(--r-xs);
    background: rgb(255 255 255 / 0.12);
    color: var(--tx);
    font-size: 10.5px;
    font-weight: 700;
  }

  .menu-separator { margin: 8px 14px; border-top: 1px solid var(--line); }

  .menu-tabs { display: flex; flex-wrap: wrap; gap: 4px; margin: 2px 0 10px; padding: 4px; border-radius: 16px; background: var(--pn); }

  .floating-menu .menu-tabs button { flex: 1 1 auto; width: auto; min-width: 0; justify-content: center; min-height: 30px; height: 30px; padding: 0 10px; border-radius: var(--r-pill); color: var(--mu); font-size: 12px; font-weight: 700; white-space: nowrap; }

  .floating-menu .menu-tabs button:hover:not(:disabled) { background: var(--pn-2); color: #fff; }

  .floating-menu .menu-tabs button[aria-current='true'] { background: var(--btn); color: var(--btn-tx); }

  .offset-control {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 10px 12px;
    margin-top: 4px;
    padding: 10px 14px;
    border-radius: 14px;
    background: rgb(255 255 255 / 0.04);
    font-size: 13px;
    font-weight: 600;
    color: var(--tx);
  }

  .offset-control input {
    width: 5.5rem;
    padding: 6px 8px;
    border: 1px solid rgb(255 255 255 / 0.08);
    border-radius: 10px;
    background: var(--pn-2);
    color: #fff;
    font: inherit;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    text-align: center;
  }

  .offset-control input:focus { outline: none; border-color: var(--ac); }

  .offset-stepper { display: inline-flex; align-items: center; gap: 6px; }

  .offset-stepper button {
    width: 32px;
    height: 32px;
    min-width: 32px;
    min-height: 32px;
    padding: 0;
    border-radius: var(--r-pill, 999px);
    background: rgb(255 255 255 / 0.08);
    color: #fff;
    font-size: 16px;
    font-weight: 700;
    justify-content: center;
  }

  .offset-stepper button:hover:not(:disabled) { background: rgb(255 255 255 / 0.16); }

  .offset-stepper .offset-reset {
    width: auto;
    height: 32px;
    padding: 0 10px;
    font-size: 11.5px;
    font-weight: 700;
  }

  .subtitle-style-panel {
    display: grid;
    gap: 10px;
    margin-top: 8px;
    padding: 12px 14px;
    border-radius: 14px;
    background: rgb(255 255 255 / 0.04);
  }

  .style-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--tx);
  }

  .style-row select {
    max-width: 55%;
    padding: 6px 10px;
    border: 1px solid rgb(255 255 255 / 0.1);
    border-radius: 10px;
    background: var(--pn-2);
    color: #fff;
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }

  .style-range {
    display: grid;
    gap: 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--mu);
  }

  .style-range input[type='range'] {
    width: 100%;
    accent-color: var(--ac);
    cursor: pointer;
  }

  @media (min-width: 769px) {
  .floating-menu {
        position: fixed;
        top: auto;
        right: 24px;
        bottom: 96px;
        left: auto;
        width: 380px;
        max-height: min(560px, calc(100dvh - 130px));
      }
}

  @media (max-width: 768px) {
  .floating-menu {
        right: 16px;
        bottom: calc(100% - 70px);
        width: min(340px, calc(100vw - 32px));
        box-sizing: border-box;
        max-height: max(100px, calc(100dvh - 220px));
      }
}
</style>
