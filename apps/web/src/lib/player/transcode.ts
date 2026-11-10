import type { Media, Source } from './types';

/**
 * Fallback transcoding, decided in the browser from the descriptor's `transcode` offer.
 * Direct play stays the default: a plan exists only when the viewer asked for a lower
 * quality, a bit rate ceiling is exceeded, the browser cannot decode the original, the
 * chosen audio or subtitle needs it, or HDR must be converted for this screen.
 * The resource grammar mirrors `crates/node/src/transcode.rs`.
 */
export type QualityChoice = 'auto' | 'original' | 2160 | 1440 | 1080 | 720 | 480 | 360;
export const QUALITY_CHOICES: readonly QualityChoice[] = ['auto', 'original', 2160, 1440, 1080, 720, 480, 360];
export type ToneMapping = 'auto' | 'always' | 'never';
export type AudioChannels = 'stereo' | 'surround' | 'passthrough';
export type TranscodeReason = 'quality' | 'bitrate' | 'audio' | 'subtitle' | 'hdr' | 'fallback';

export const FLAG_TONEMAP = 1;
export const FLAG_NORMALIZE = 2;
export const FLAG_NIGHT = 4;
export const FLAG_SURROUND = 8;
export const FLAG_COPY_AUDIO = 16;

/** Heights and the video bit rate used for each, close to common streaming ladders. */
export const LADDER: readonly { height: number; kbps: number }[] = [
  { height: 2160, kbps: 20000 }, { height: 1440, kbps: 12000 }, { height: 1080, kbps: 8000 },
  { height: 720, kbps: 4000 }, { height: 480, kbps: 1500 }, { height: 360, kbps: 800 }
];
/** Audio bit rate reserved inside a ceiling (AAC stereo 192k, a little margin). */
const AUDIO_KBPS = 256;

export interface TranscodeOffer {
  url: string;
  videoIndex: number;
  videoCodec?: string | null;
  width?: number | null;
  height?: number | null;
  bitrateKbps?: number | null;
  durationSeconds: number;
  /** `hdr10`, `hlg`, `dolby_vision_{profile}` or absent for SDR. */
  dynamicRange?: string | null;
  /** MSE type of the original video when it can be copied (AV1, H.264). */
  copyVideoMime?: string | null;
  /** Server switches: `false` when the administrator forbids encoding that stream kind. */
  audio?: boolean;
  video?: boolean;
  /** `false` when the server refuses HDR to SDR conversion. */
  toneMapping?: boolean;
  /** Server video bit rate ceiling of an encode in kbit/s; 0 or absent for none. */
  maxKbps?: number;
}

export interface TranscodeOptions {
  quality: QualityChoice;
  /** kbit/s, 0 for unlimited. */
  maxKbps: number;
  toneMapping: ToneMapping;
  audioChannels: AudioChannels;
  /** Server-side audio processing, used only when the browser cannot process audio. */
  serverAudio?: 'off' | 'normalize' | 'night';
  /** Image subtitle to burn into the picture. */
  burnSubtitle?: number;
  /** A lower ceiling learned from stalls by the automatic quality. */
  adaptiveKbps?: number;
}

export interface TranscodePlan {
  reason: TranscodeReason;
  video: number;
  audio?: number;
  subtitle?: number;
  /** 0 copies the original video. */
  height: number;
  kbps: number;
  flags: number;
  mime: string;
  /** Prefix the progressive player completes with the start position in milliseconds. */
  prefix: string;
}

export interface Environment {
  /** MediaSource.isTypeSupported. */
  mse: (mime: string) => boolean;
  /** HTMLMediaElement.canPlayType, for passthrough codecs. */
  canPlay: (mime: string) => string;
  hdrDisplay: boolean;
}

export function browserEnvironment(video: HTMLVideoElement): Environment {
  return {
    mse: mime => typeof MediaSource !== 'undefined' && MediaSource.isTypeSupported(mime),
    canPlay: mime => video.canPlayType?.(mime) ?? '',
    hdrDisplay: typeof matchMedia === 'function' && (matchMedia('(dynamic-range: high)').matches || matchMedia('(video-dynamic-range: high)').matches)
  };
}

export function qualityChoice(value: unknown): QualityChoice {
  const numeric = typeof value === 'string' && /^\d+$/.test(value) ? Number(value) : value;
  return QUALITY_CHOICES.find(choice => choice === numeric) ?? 'auto';
}

export function maxBitrate(value: unknown): number {
  return typeof value === 'number' && Number.isInteger(value) && (value === 0 || (value >= 500 && value <= 120000)) ? value : 0;
}

/** Index of the embedded audio a source plays; `undefined` for an external (sidecar) track. */
export function audioIndex(media: Media, source: Source): number | undefined {
  const explicit = /^internal-(\d+)$/.exec(source.audioTrackId)?.[1];
  if (explicit !== undefined) return Number(explicit);
  if (source.audioTrackId !== 'internal-default') return undefined;
  const audios = (source.tracks ?? media.sources[0]?.tracks ?? []).filter(track => track.codec_type === 'audio');
  return (audios.find(track => track.disposition?.default === 1) ?? audios[0])?.index;
}

function audioTrack(media: Media, index: number | undefined) {
  if (index === undefined) return undefined;
  for (const source of media.sources) {
    const track = source.tracks?.find(item => item.codec_type === 'audio' && item.index === index);
    if (track) return track;
  }
  return undefined;
}

/** HDR the browser would show wrongly here: needs the server's tone mapping. */
export function needsToneMapping(offer: TranscodeOffer, options: Pick<TranscodeOptions, 'toneMapping'>, environment: Pick<Environment, 'hdrDisplay'>): boolean {
  if (!offer.dynamicRange || options.toneMapping === 'never' || environment.hdrDisplay) return false;
  // Dolby Vision profile 5 has no HDR10/HLG base layer: the generic curve gives wrong colours.
  if (offer.dynamicRange === 'dolby_vision_5') return false;
  return true;
}

/** Highest rung within the source height and every ceiling; never above the original. */
export function rung(offer: TranscodeOffer, options: TranscodeOptions): { height: number; kbps: number } {
  const sourceHeight = offer.height ?? 1080;
  // The smallest rung at least as tall as the source: scaling is `min(ih, height)`, so a
  // 1920×800 film asked at "1080" keeps its 800 lines instead of dropping to 720.
  const top = LADDER.filter(item => item.height >= sourceHeight).at(-1)?.height ?? LADDER[0].height;
  const ceilings = [options.maxKbps, options.adaptiveKbps ?? 0].filter(value => value > 0);
  // The server ceiling bounds the video rate itself, the audio comes on top.
  const server = offer.maxKbps && offer.maxKbps > 0 ? offer.maxKbps : Infinity;
  const budget = ceilings.length ? Math.max(200, Math.min(...ceilings) - AUDIO_KBPS) : Infinity;
  const wanted = typeof options.quality === 'number' ? options.quality : Infinity;
  const fits = LADDER.filter(item => item.height <= top && item.height <= wanted);
  // A rung may be squeezed to three quarters of its nominal rate before dropping a step.
  const limit = Math.min(budget, server);
  const chosen = fits.find(item => item.kbps * 0.75 <= limit) ?? fits.at(-1) ?? LADDER.at(-1)!;
  return { height: chosen.height, kbps: Math.round(Math.min(chosen.kbps, limit)) };
}

/** Why this source cannot be played as it is, or `undefined` for direct play. */
export function transcodeReason(media: Media, source: Source, options: TranscodeOptions, environment: Environment, unsupportedAudio: boolean): TranscodeReason | undefined {
  const offer = media.transcode;
  if (!offer) return undefined;
  if (options.burnSubtitle !== undefined) return 'subtitle';
  if (source.transcodeOnly || unsupportedAudio) return 'audio';
  if (typeof options.quality === 'number' && offer.height && options.quality < offer.height) return 'quality';
  const ceilings = [options.maxKbps, options.adaptiveKbps ?? 0].filter(value => value > 0);
  if (ceilings.length && offer.bitrateKbps && offer.bitrateKbps > Math.min(...ceilings)) return 'bitrate';
  if (options.toneMapping === 'always' && needsToneMapping(offer, options, environment)) return 'hdr';
  return undefined;
}

export function resourcePrefix(offer: TranscodeOffer, plan: Omit<TranscodePlan, 'prefix' | 'mime' | 'reason'>): string {
  const optional = (value: number | undefined) => value === undefined ? 'x' : String(value);
  return `${offer.url}v${plan.video}a${optional(plan.audio)}s${optional(plan.subtitle)}h${plan.height}b${plan.kbps}f${plan.flags}-`;
}

function h264Codec(height: number) {
  return height <= 1080 ? 'avc1.640029' : height <= 1440 ? 'avc1.640032' : 'avc1.640033';
}

/** Audio the Node may copy into fragmented MP4 instead of encoding it. */
const COPY_AUDIO: Record<string, string> = { aac: 'mp4a.40.2', ac3: 'ac-3', eac3: 'ec-3', opus: 'opus', flac: 'flac' };

function videoCodecs(mime: string): string | undefined {
  return /codecs="([^"]+)"/.exec(mime)?.[1];
}

/**
 * The transcode to request for `reason`, or `undefined` when the browser cannot play
 * any transcoded output (no MSE) or the offer is missing.
 */
export function planTranscode(media: Media, source: Source, reason: TranscodeReason, options: TranscodeOptions, environment: Environment): TranscodePlan | undefined {
  const offer = media.transcode;
  if (!offer || !(offer.durationSeconds > 0)) return undefined;
  const audio = audioIndex(media, source);
  const track = audioTrack(media, audio);
  let flags = 0;
  let audioCodec: string | undefined;
  if (audio !== undefined) {
    const codec = track?.codec_name;
    const passthrough = options.audioChannels === 'passthrough' && (codec === 'eac3' || codec === 'ac3')
      && Boolean(environment.canPlay(`audio/mp4; codecs="${codec === 'eac3' ? 'ec-3' : 'ac-3'}"`));
    if (passthrough && !options.serverAudio) { flags |= FLAG_COPY_AUDIO; audioCodec = codec === 'eac3' ? 'ec-3' : 'ac-3'; }
    else if (offer.audio === false) {
      // Audio encoding is off on this server: only a copy of the original track remains.
      audioCodec = codec ? COPY_AUDIO[codec] : undefined;
      if (!audioCodec) return undefined;
      flags |= FLAG_COPY_AUDIO;
    } else {
      audioCodec = 'mp4a.40.2';
      if ((options.audioChannels === 'surround' || options.audioChannels === 'passthrough') && (track?.channels ?? 0) >= 6) flags |= FLAG_SURROUND;
      if (options.serverAudio === 'normalize') flags |= FLAG_NORMALIZE;
      if (options.serverAudio === 'night') flags |= FLAG_NIGHT;
    }
  }
  const tonemap = offer.video !== false && offer.toneMapping !== false && needsToneMapping(offer, options, environment);
  const audioPart = audioCodec ? `,${audioCodec}` : '';
  // Audio-only conversions keep the original picture when the browser decodes it.
  const copyCodecs = offer.copyVideoMime ? videoCodecs(offer.copyVideoMime) : undefined;
  const canCopy = reason === 'audio' && !tonemap && audio !== undefined && !(flags & FLAG_COPY_AUDIO)
    && copyCodecs !== undefined && environment.mse(`video/mp4; codecs="${copyCodecs}${audioPart}"`);
  const target = rung(offer, options);
  const height = canCopy ? 0 : target.height;
  // Video encoding is off on this server: nothing but a copied picture can be offered.
  if (height !== 0 && offer.video === false) return undefined;
  if (height !== 0 && tonemap) flags |= FLAG_TONEMAP;
  const mime = `video/mp4; codecs="${canCopy ? copyCodecs : h264Codec(height)}${audioPart}"`;
  if (!environment.mse(mime)) return undefined;
  const plan = { video: offer.videoIndex, audio, subtitle: height === 0 ? undefined : options.burnSubtitle, height, kbps: target.kbps, flags };
  return { ...plan, reason, mime, prefix: resourcePrefix(offer, plan) };
}

/** One rung down, or `undefined` at the bottom of the ladder. */
export function lowerCeiling(plan: Pick<TranscodePlan, 'kbps'>): number | undefined {
  const next = LADDER.find(item => item.kbps < plan.kbps);
  return next ? next.kbps + AUDIO_KBPS : undefined;
}

/**
 * Automatic quality: repeated waiting while playing means the link (or the encoder)
 * cannot keep up. Three stalls within a minute, or one longer than four seconds,
 * ask for a lower ceiling. Startup and seeks are not counted.
 */
export class StallMonitor {
  private stalls: number[] = [];
  private since?: number;
  constructor(private readonly window = 60000, private readonly count = 3, private readonly longStall = 4000) {}
  waiting(now: number) { this.since ??= now; this.stalls = [...this.stalls.filter(time => now - time < this.window), now]; return this.stalls.length >= this.count; }
  playing(now: number) { const long = this.since !== undefined && now - this.since >= this.longStall; this.since = undefined; return long; }
  /** Checked periodically while still waiting, so a long stall reacts before playback gives up. */
  stuck(now: number) { return this.since !== undefined && now - this.since >= this.longStall; }
  reset() { this.stalls = []; this.since = undefined; }
}

/** The stored spelling of a quality choice (heights are kept as strings). */
export type QualityPreference = 'auto' | 'original' | '2160' | '1440' | '1080' | '720' | '480' | '360';
export function qualityPreference(value: QualityChoice): QualityPreference {
  return value === 'auto' || value === 'original' ? value : String(value) as QualityPreference;
}
