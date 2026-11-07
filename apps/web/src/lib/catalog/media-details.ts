/**
 * Fiche technique d'un asset (`GET /api/v1/assets/{id}/details`) et mise en forme
 * pure pour le panneau « Informations média ». Les chemins ne sont présents que
 * pour un administrateur : le Core les retire pour un compte `user`.
 */
import { colorProfileLabel, containerLabel, languageLabel, videoResolutionBadge } from '../player/track-labels';

import { tr, currentFormat } from '../i18n';
export interface AssetTrack {
  index: number;
  codecType?: string | null;
  codecName?: string | null;
  codecLongName?: string;
  profile?: string;
  level?: number;
  width?: number;
  height?: number;
  pixFmt?: string;
  colorSpace?: string;
  colorTransfer?: string;
  colorPrimaries?: string;
  colorRange?: string;
  fieldOrder?: string;
  sampleRate?: string | number;
  channels?: number;
  channelLayout?: string;
  bitsPerRawSample?: string | number;
  frameRate?: number;
  bitRate?: number;
  durationSeconds?: number;
  language?: string | null;
  title?: string | null;
  disposition?: Record<string, boolean>;
}

export interface AssetDetails {
  assetId: string;
  workId: string;
  available: boolean;
  pathsVisible: boolean;
  node: { id: string; name: string; kind: string; status: string; connectionMode?: string | null; version?: string | null };
  library: { id: string; name: string; kind: string };
  file: {
    size: number;
    mime: string;
    modifiedAt?: number | null;
    name?: string;
    relativePath?: string;
    libraryRoot?: string;
    absolutePath?: string;
  };
  container: { formatName?: string | null; formatLongName?: string | null; durationSeconds?: number | null; bitRate?: number | null; startTime?: number | null };
  tracks: AssetTrack[];
  externalAudio: { language: string; size: number; offsetSeconds: number; track?: AssetTrack | null; relativePath?: string; absolutePath?: string }[];
  externalSubtitles: { origin: 'sidecar' | 'downloaded'; language: string; label?: string | null; forced: boolean; sdh: boolean; sourceFormat?: string | null }[];
  chapterCount: number;
}

export async function fetchAssetDetails(id: string, signal: AbortSignal): Promise<AssetDetails> {
  const response = await fetch(`/api/v1/assets/${encodeURIComponent(id)}/details`, { signal, headers: { accept: 'application/json' } });
  if (response.status === 401) throw new Error(tr('catalog_your_session_has_expired_sign_in'));
  if (response.status === 403) throw new Error(tr('your_account_does_not_have_access'));
  if (response.status === 404) throw new Error(tr('this_file_is_no_longer_in'));
  if (!response.ok) throw new Error(tr('information_unavailable_http', { status: response.status }));
  return (await response.json()) as AssetDetails;
}

const number = (value: number, digits = 1) => currentFormat().number(value, { maximumFractionDigits: digits });

export function formatBytes(size?: number | null): string {
  if (!size || size < 0) return '—';
  const units = ['o', 'Ko', 'Mo', 'Go', 'To'];
  let value = size;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
  return `${number(value, unit === 0 ? 0 : value < 10 ? 2 : 1)} ${units[unit]}`;
}

export function formatBitrate(bps?: number | null): string | null {
  if (!bps || bps <= 0) return null;
  if (bps >= 1_000_000) return `${number(bps / 1_000_000, bps >= 10_000_000 ? 1 : 2)} Mb/s`;
  return `${number(Math.round(bps / 1000), 0)} kb/s`;
}

export function formatDuration(seconds?: number | null): string | null {
  if (!seconds || seconds <= 0) return null;
  const total = Math.round(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h) return `${h} h ${String(m).padStart(2, '0')} min`;
  if (m) return `${m} min ${String(s).padStart(2, '0')} s`;
  return `${s} s`;
}

export function formatSampleRate(rate?: string | number): string | null {
  const hz = Number(rate);
  return Number.isFinite(hz) && hz > 0 ? `${number(hz / 1000, 1)} kHz` : null;
}

export function formatChannels(track: Pick<AssetTrack, 'channels' | 'channelLayout'>): string | null {
  const count = track.channels ? `${track.channels} ${track.channels > 1 ? 'canaux' : 'canal'}` : '';
  const layout = track.channelLayout === 'stereo' ? 'stéréo' : track.channelLayout;
  return [layout, count].filter(Boolean).join(' · ') || null;
}

export function dynamicRangeLabel(track: Pick<AssetTrack, 'colorTransfer'>): string | null {
  if (track.colorTransfer === 'smpte2084') return tr('hdr10_pq');
  if (track.colorTransfer === 'arib-std-b67') return 'HLG';
  return null;
}

/** Rôles de piste lisibles (par défaut, forcés, SDH…), dans un ordre stable. */
export function trackRoles(disposition?: Record<string, boolean>): string[] {
  const labels: [string, string][] = [
    ['default', tr('default')], ['forced', tr('forced')], ['hearingImpaired', tr('hard_of_hearing_sdh')],
    ['visualImpaired', tr('audio_description')], ['comment', tr('commentary')], ['original', tr('catalog_original_version')], ['dub', tr('dubbed')]
  ];
  return labels.filter(([key]) => disposition?.[key]).map(([, label]) => label);
}

export function videoSummary(track: AssetTrack): string[] {
  const badge = videoResolutionBadge(track.width, track.height);
  return [
    track.width && track.height ? `${track.width} × ${track.height}${badge ? ` (${badge})` : ''}` : '',
    track.frameRate ? `${number(track.frameRate, 3)} i/s` : '',
    colorProfileLabel(track.pixFmt) ?? '',
    dynamicRangeLabel(track) ?? '',
    track.profile ? `Profil ${track.profile}` : '',
    formatBitrate(track.bitRate) ?? ''
  ].filter(Boolean);
}

export function audioSummary(track: AssetTrack): string[] {
  return [formatChannels(track) ?? '', formatSampleRate(track.sampleRate) ?? '', track.profile ?? '', formatBitrate(track.bitRate) ?? ''].filter(Boolean);
}

export function codecLabel(track: Pick<AssetTrack, 'codecName' | 'codecLongName'>): string {
  const name = track.codecName?.toUpperCase();
  if (!name) return tr('unknown_codec');
  return track.codecLongName && track.codecLongName.toUpperCase() !== name ? `${name} — ${track.codecLongName}` : name;
}

export function trackLanguage(track: Pick<AssetTrack, 'language'>): string {
  return track.language ? languageLabel(track.language) : tr('language_not_specified');
}

export function groupTracks(tracks: AssetTrack[]) {
  return {
    video: tracks.filter(track => track.codecType === 'video'),
    audio: tracks.filter(track => track.codecType === 'audio'),
    subtitle: tracks.filter(track => track.codecType === 'subtitle'),
    other: tracks.filter(track => !['video', 'audio', 'subtitle'].includes(track.codecType ?? ''))
  };
}

export function versionLabel(details: AssetDetails | undefined, position: number): string {
  const video = details?.tracks.find(track => track.codecType === 'video');
  const parts = [
    videoResolutionBadge(video?.width, video?.height),
    video?.codecName?.toUpperCase(),
    details ? shortContainer(details.file.mime) : null
  ].filter(Boolean);
  return parts.length ? `Version ${position + 1} · ${parts.join(' · ')}` : `Version ${position + 1}`;
}

export function shortContainer(mime?: string): string {
  return mime === 'video/mp4' ? 'MP4' : mime === 'video/x-matroska' ? 'MKV' : mime === 'video/webm' ? 'WebM' : containerLabel(mime);
}

export function nodeStatusLabel(status?: string): { label: string; tone: 'online' | 'offline' | 'revoked' } {
  if (status === 'connected') return { label: tr('connected'), tone: 'online' };
  if (status === 'revoked') return { label: tr('revoked'), tone: 'revoked' };
  return { label: tr('common_disconnected'), tone: 'offline' };
}

export function nodeKindLabel(kind?: string): string {
  return kind === 'local' ? tr('local_media_node') : kind === 'remote' ? tr('remote_media_node') : tr('media_node');
}
