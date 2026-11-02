import type { Preferences } from '../preferences';
import type { SubtitlePreference } from './subtitle-preference';
import { audioLanguage } from './source-selection';

export const SKIP_SECONDS = [5, 10, 15, 30, 60] as const;

/** Old accounts keep manual episode changes and ten-second control buttons. */
export function playbackPreferences(value: {
  autoplayNextEpisode?: unknown; skipBackSeconds?: unknown; skipForwardSeconds?: unknown;
}) {
  const skip = (seconds: unknown): number =>
    typeof seconds === 'number' && SKIP_SECONDS.some(option => option === seconds) ? seconds : 10;
  return {
    autoplayNextEpisode: value.autoplayNextEpisode === true,
    skipBackSeconds: skip(value.skipBackSeconds),
    skipForwardSeconds: skip(value.skipForwardSeconds)
  };
}

/** Configured defaults remain separate from remembered manual selections. */
export function trackPreferences(value: Preferences) {
  const rememberAudioSelections = value.rememberAudioSelections !== false;
  const rememberSubtitleSelections = value.rememberSubtitleSelections !== false;
  const audioTrack = rememberAudioSelections ? value.audioTrack : undefined;
  const language = audioLanguage(audioTrack?.language ?? value.audioLanguage ?? 'fr');
  const subtitleDefault: SubtitlePreference | undefined = value.subtitles === 'off' ? { enabled: false }
    : value.subtitles === 'always' ? { enabled: true, language: 'fr', forced: false } : undefined;
  return {
    rememberAudioSelections, rememberSubtitleSelections,
    playDefaultAudioTrack: value.playDefaultAudioTrack === true,
    audioTrack,
    audioLanguage: language && language !== 'und' ? language : 'fr',
    subtitlePreference: rememberSubtitleSelections ? value.subtitlePreference ?? subtitleDefault : subtitleDefault
  };
}
