export interface CodecTrack {
  index?: number; codec_type?: string; codec_name?: string; channels?: number; channel_layout?: string;
  width?: number; height?: number; pix_fmt?: string; tags?: Record<string, string>;
}
export interface Source {
  id: string;
  audioTrackId: string;
  language: string;
  url: string;
  directUrl?: string;
  relayUrl?: string;
  tokenExpiresAt?: number;
  nodeId?: string;
  nodeStatus?: 'connected' | 'disconnected' | 'revoked';
  mime: string;
  tracks?: CodecTrack[];
  externalAudioUrl?: string;
  prepare?: boolean;
  label?: string;
  audioOffsetSeconds?: number;
}
export interface Subtitle { id: string; language: string; label: string; url: string; forced?: boolean; sdh?: boolean; sourceFormat?: string }
export interface Media {
  chapters?: { start: number; title: string }[];
  id: string;
  workId?: string;
  title: string;
  synthetic?: boolean;
  nodeId?: string;
  nodeStatus?: 'connected' | 'disconnected' | 'revoked';
  available?: boolean;
  sources: Source[];
  subtitles: Subtitle[];
}
export interface PlaybackState { position: number; duration: number; paused: boolean; volume: number; speed: number }
export interface Player {
  setStatusListener?(listener:(message:string)=>void):void;
  load(media: Media, initialAudioTrackId?: string): Promise<void>;
  play(): Promise<void>;
  pause(): void;
  seek(position: number): void;
  selectAudio(id: string): Promise<void>;
  selectSubtitle(id: string | null): Promise<void>;
  setVolume(volume: number): void;
  setSpeed(speed: number): void;
  setAudioOffset?(seconds: number): void;
  setSubtitleOffset?(seconds: number): void;
  state(): PlaybackState;
  destroy(): void;
}

export type PlayerFactory = (video: HTMLVideoElement, onError: (message: string) => void) => Player;
