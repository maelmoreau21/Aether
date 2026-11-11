<script lang="ts">
  import type { TranscodeInfo } from './controls-props';
  import type { Media, PlaybackState } from './types';
  import { t } from '../i18n';

  let { statistics = $bindable(), busy, error, wake, active, subtitle, audio, transcodeInfo, paused, playback, metrics, selectedSource, loadStage }: { statistics: boolean; busy: boolean; error: string; wake: () => void; active: Media | undefined; subtitle: string | null; audio: string; transcodeInfo: TranscodeInfo; paused: boolean; playback: PlaybackState; metrics: { width: number; height: number; frames: number; dropped: number; buffered: number; audioBuffered: number | null; ready: number; network: number }; selectedSource: Media['sources'][number] | undefined; loadStage: string } = $props();
</script>

{#if statistics}<section id="stream-statistics" class="stream-statistics" aria-label={$t('playback_data')}>
  <header><strong>{$t('playback_data')}</strong><button onclick={() => { statistics = false; wake(); }}>{$t('close')}</button></header>
  <p>{$t('browser_video_metrics_refreshed_every_second')}</p>
  <dl>
    <dt>{$t('state')}</dt><dd>{busy ? 'Chargement' : error ? 'Erreur' : paused ? $t('paused') : 'Lecture'}</dd>
    <dt>{$t('decoded_video')}</dt><dd>{metrics.width} × {metrics.height}</dd>
    <dt>{$t('frames_dropped')}</dt><dd>{metrics.frames} / {metrics.dropped}</dd>
    <dt>{$t('buffered_video')}</dt><dd>{metrics.buffered.toFixed(1)} s</dd>
    <dt>{$t('buffered_separate_audio')}</dt><dd>{metrics.audioBuffered === null ? $t('not_used') : `${metrics.audioBuffered.toFixed(1)} s`}</dd>
    <dt>{$t('position_duration')}</dt><dd>{playback.position.toFixed(1)} / {playback.duration.toFixed(1)} s</dd>
    <dt>{$t('speed')}</dt><dd>{playback.speed}×</dd>
    <dt>{$t('readystate_networkstate')}</dt><dd>{metrics.ready} / {metrics.network}</dd>
    <dt>{$t('server_node')}</dt><dd>{selectedSource?.nodeId ?? active?.nodeId ?? 'Local'}</dd>
    <dt>{$t('transport_mode')}</dt><dd>{selectedSource?.directUrl ? $t('direct_node') : selectedSource?.relayUrl ? $t('core_relay') : 'Local'}</dd>
    <dt>{$t('source_audio_track')}</dt><dd>{selectedSource?.id ?? $t('waiting')} / {audio || $t('waiting')}</dd>
    <dt>{$t('container_language')}</dt><dd>{selectedSource?.mime ?? $t('stats_not_set')} / {selectedSource?.language ?? $t('not_specified')}</dd>
    <dt>{$t('declared_audio_mode')}</dt><dd>{selectedSource?.audioSegmentUrl ? 'Segments' : selectedSource?.externalAudioUrl ? 'Séparé' : 'Interne'}</dd>
    <dt>{$t('report_category_subtitles')}</dt><dd>{subtitle ?? 'Désactivés'}</dd>
    <dt>{$t('transcoding')}</dt><dd>{transcodeInfo ? `${transcodeInfo.height ? `${transcodeInfo.height}p · ${(transcodeInfo.kbps / 1000).toFixed(1)} Mb/s` : $t('transcoding_audio_only')} · ${transcodeInfo.reason}${transcodeInfo.encoder ? ` · ${transcodeInfo.encoder}` : ''}` : $t('not_used')}</dd>
  </dl>
  {#each selectedSource?.tracks ?? [] as track, i (i)}<p>{$t('track_diagnostic', { index: track.index ?? i, type: track.codec_type, codec: track.codec_name, profile: track.profile ?? '', channels: track.channels ? `· ${$t('channels_count', { count: track.channels })}` : '' })}</p>{/each}
  <p>{$t('technical_diagnostic', { detail: error || loadStage })}</p>
  <p>{$t('declared_tracks_do_not_prove_physical')}</p>
</section>{/if}

<style>
  .stream-statistics button { color:#eee; background:#ffffff15; border:1px solid #ffffff30; border-radius:8px; padding:10px 14px; cursor:pointer; }

  .stream-statistics { position:absolute; z-index:7; top:90px; right:24px; width:min(460px,calc(100% - 48px)); max-height:calc(100dvh - 230px); overflow:auto; padding:20px; box-sizing:border-box; background:rgba(16,17,25,.96); color:#eee; border:1px solid #ffffff25; border-radius:12px; font-size:12px; overflow-wrap:anywhere; }

  .stream-statistics header { display:flex; align-items:center; justify-content:space-between; gap:12px; }

  .stream-statistics dl { display:grid; grid-template-columns:1fr 1fr; gap:8px 16px; }

  .stream-statistics dd { margin:0; }

  .stream-statistics dt,
  .stream-statistics p { color:#bbb8ca; }

  .stream-statistics {
    padding: 20px;
    border: 0;
    border-radius: 24px;
    background: rgb(17 21 31 / 0.92);
    backdrop-filter: blur(24px);
    -webkit-backdrop-filter: blur(24px);
    color: #eef1f8;
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.5);
    font-family: var(--font);
  }

  .stream-statistics { font-size: 13px; }

  .stream-statistics dt,
  .stream-statistics p { color: #a2a9ba; }

  .stream-statistics dd { font-family: var(--font-mono); font-size: 12px; }

  .stream-statistics button { min-height: 40px; border: 0; border-radius: 12px; background: rgb(255 255 255 / 0.14); color: #f4f6fb; font-weight: 700; }

  .stream-statistics button:hover { background: rgb(255 255 255 / 0.24); }
</style>
