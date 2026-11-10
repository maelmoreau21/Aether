export interface CodecTrack {
  index?: number; codec_type?: string; codec_name?: string; profile?: string; channels?: number; channel_layout?: string;
  width?: number; height?: number; pix_fmt?: string; tags?: Record<string, string>;
  disposition?: { default?: number; comment?: number; hearing_impaired?: number; visual_impaired?: number; descriptions?: number };
}
export interface Source {
  id: string;
  reportTrackId?: string;
  audioTrackId: string;
  language: string;
  url: string;
  directUrl?: string;
  relayUrl?: string;
  tokenExpiresAt?: number;
  nodeId?: string;
  nodeStatus?: 'connected' | 'disconnected' | 'revoked';
  connectionMode?: string;
  score?: number;
  mime: string;
  tracks?: CodecTrack[];
  externalAudioUrl?: string;
  prepare?: boolean;
  prepareVideo?: boolean;
  preparedVideoUrl?: string;
  audioSegmentUrl?: string;
  audioSegmentMode?: 'absolute' | 'rebased';
  durationSeconds?: number;
  videoStreamUrl?: string;
  videoMime?: string;
  label?: string;
  audioOffsetSeconds?: number;
  /** Another embedded audio track, reachable only through fallback transcoding. */
  transcodeOnly?: boolean;
}
export interface Subtitle {
  id: string; reportTrackId?: string; language: string; label: string; url: string; assUrl?: string | null; forced?: boolean; sdh?: boolean; sourceFormat?: string;
  /** Bitmap subtitles: `pgs` renders in the browser, `burn` needs the picture transcoded. */
  image?: 'pgs' | 'burn' | null;
  /** Stream index to burn into the picture when fallback transcoding is offered. */
  burnIndex?: number | null;
}
/** Another copy of the same content on another Node, authorized for the current viewer. */
export interface MediaAlternative {
  id: string;
  nodeId: string;
  nodeName: string;
  nodeStatus: 'connected' | 'disconnected' | 'revoked';
  connectionMode: string;
  score: number;
  mime: string;
  /** `identical`: same file; `same-runtime`: another encode with the same timeline. */
  equivalence: 'identical' | 'same-runtime';
}
export interface Media {
  chapters?: { start: number; title: string }[];
  trickplay?: { intervalMs: number; width: number; height: number; columns: number; rows: number; sheetCount: number; thumbnailCount: number; urlPrefix: string; mime?: 'image/jpeg' | 'image/webp' } | null;
  intro?: { start: number; end: number };
  credits?: { start: number; end: number };
  recap?: { start: number; end: number };
  preview?: { start: number; end: number };
  id: string;
  workId?: string;
  title: string;
  synthetic?: boolean;
  episode?: boolean;
  nodeId?: string;
  nodeStatus?: 'connected' | 'disconnected' | 'revoked';
  connectionMode?: string;
  score?: number;
  available?: boolean;
  /** Equivalent copies on connected Nodes this viewer may read, best first. */
  fallbackMediaIds?: string[];
  /** Every equivalent, authorized copy whatever its Node's state, for an explicit choice. */
  alternatives?: MediaAlternative[];
  failoverFrom?: string;
  requestedId?: string;
  sources: Source[];
  subtitles: Subtitle[];
  /** Fallback transcoding offered by the server for this file. */
  transcode?: import('./transcode').TranscodeOffer | null;
}
export interface PlaybackState { position: number; duration: number; paused: boolean; volume: number; speed: number }
export interface Player {
  playbackInfo?(): import('../statistics/collector').PlaybackInfo;
  setStatusListener?(listener:(message:string)=>void):void;
  load(media: Media, initialAudioTrackId?: string, initialPosition?: number | Promise<number>): Promise<void>;
  play(): Promise<void>;
  pause(): void;
  seek(position: number): void;
  selectAudio(id: string): Promise<void>;
  selectSubtitle(id: string | null): Promise<void>;
  setVolume(volume: number): void;
  setSpeed(speed: number): void;
  setAudioOffset?(seconds: number): void;
  setSubtitleOffset?(seconds: number): void;
  /** WebVTT cue line in percent from the top (custom subtitle position). */
  setSubtitlePosition?(line: number): void;
  /** Browser-side night mode / loudness levelling; `false` when this playback cannot be processed. */
  setAudioEnhancement?(mode: import('./audio-enhancement').AudioEnhancement): boolean;
  /** Quality, bit rate ceiling, HDR and audio choices; reloads the stream when they change it. */
  setStreamOptions?(options: Partial<import('./transcode').TranscodeOptions>): Promise<void>;
  /** The active transcode, or `null` for direct play. */
  transcodeState?(): { plan: import('./transcode').TranscodePlan; encoder?: string; tonemap?: string } | null;
  state(): PlaybackState;
  destroy(): void;
}

export type PlayerFactory = (video: HTMLVideoElement, onError: (message: string, kind?: import("./diagnostics").PlaybackFailureKind) => void) => Player;
