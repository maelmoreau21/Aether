import type { Media, PlaybackState } from './types';
import type { ControlsMenu } from './controls-format';
import type { QualityChoice } from './transcode';
import type { AudioEnhancement } from './audio-enhancement';
import type { SleepChoice } from './sleep-timer';
import type { SubtitleStyle } from './subtitle-style';

export type TranscodeInfo = { height: number; kbps: number; reason: string; encoder?: string; tonemap?: string } | null;

export type Menu = ControlsMenu;

/** Props of `PlayerControls`; the settings menu reuses the subset it needs. */
export type ControlsProps = {
  quality?: QualityChoice; maxKbps?: number; onQuality?: (value: QualityChoice) => void; onMaxKbps?: (value: number) => void;
  /** Active fallback transcode, `null` for direct play. */
  transcode?: TranscodeInfo;
  enhancement?: AudioEnhancement; onEnhancement?: (value: AudioEnhancement) => void;
  /** Seconds before sleep, `'end'` at the end of this media, `null` when off. */
  sleep?: number | 'end' | null; onSleep?: (value: SleepChoice | null) => void;
  audioOffset?: number; subtitleOffset?: number; onAudioOffset?: (value: number) => void; onSubtitleOffset?: (value: number) => void;
  subtitleStyle?: SubtitleStyle; onSubtitleStyle?: (style: SubtitleStyle) => void;
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
  onSeek: (value: number, kind?: 'seek' | 'skip_intro' | 'skip_credits' | 'skip_recap' | 'skip_preview') => void;
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
};
