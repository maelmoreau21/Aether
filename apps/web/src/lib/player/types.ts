export interface CodecTrack {
  index?: number; codec_type?: string; codec_name?: string; channels?: number; channel_layout?: string;
  width?: number; height?: number; pix_fmt?: string; tags?: Record<string, string>;
}
export interface Source { id: string; audioTrackId: string; language: string; url: string; mime: string; tracks?: CodecTrack[] }
export interface Subtitle { id: string; language: string; label: string; url: string }
export interface Media { id: string; title: string; sources: Source[]; subtitles: Subtitle[] }
export interface PlaybackState { position: number; duration: number; paused: boolean; volume: number; speed: number }
export interface Player {
  load(media: Media): Promise<void>;
  play(): Promise<void>;
  pause(): void;
  seek(position: number): void;
  selectAudio(id: string): Promise<void>;
  selectSubtitle(id: string | null): Promise<void>;
  setVolume(volume: number): void;
  setSpeed(speed: number): void;
  state(): PlaybackState;
  destroy(): void;
}

export type PlayerFactory = (video: HTMLVideoElement, onError: (message: string) => void) => Player;
