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

/**
 * Audio and subtitle languages are their own preferences. When an account never chose one, audio
 * follows the interface language and subtitles follow the metadata language (what the person reads),
 * so an English account does not start with French audio because French was once the only default.
 */
export function languageDefaults(value: Pick<Preferences, 'locale' | 'audioLanguage' | 'subtitleLanguage' | 'metadataLocale'>) {
  const interfaceLanguage = audioLanguage(value.locale ?? 'fr');
  const audio = value.audioLanguage ? audioLanguage(value.audioLanguage) : interfaceLanguage;
  const subtitle = value.subtitleLanguage ? audioLanguage(value.subtitleLanguage)
    : value.metadataLocale ? audioLanguage(value.metadataLocale) : interfaceLanguage;
  return { audio: audio && audio !== 'und' ? audio : interfaceLanguage, subtitle: subtitle && subtitle !== 'und' ? subtitle : interfaceLanguage };
}

/** Configured defaults remain separate from remembered manual selections. */
export function trackPreferences(value: Preferences) {
  const rememberAudioSelections = value.rememberAudioSelections !== false;
  const rememberSubtitleSelections = value.rememberSubtitleSelections !== false;
  const audioTrack = rememberAudioSelections ? value.audioTrack : undefined;
  const defaults = languageDefaults(value);
  const language = audioLanguage(audioTrack?.language ?? defaults.audio);
  const subtitleDefault: SubtitlePreference | undefined = value.subtitles === 'off' ? { enabled: false }
    : value.subtitles === 'always' ? { enabled: true, language: defaults.subtitle, forced: false } : undefined;
  // Original-language audio (VO) has its own default and its own remembered choice: turning
  // subtitles on for one film in VO turns them on for every film in VO, never in the
  // person's own language. Accounts that never chose in VO keep their single old choice.
  const foreignDefault: SubtitlePreference | undefined = value.subtitles === 'foreign'
    ? { enabled: true, language: defaults.subtitle, forced: false } : subtitleDefault;
  const rememberedForeign = rememberSubtitleSelections
    ? value.subtitlePreferenceForeign ?? (value.subtitles === 'foreign' ? undefined : value.subtitlePreference)
    : undefined;
  return {
    rememberAudioSelections, rememberSubtitleSelections,
    playDefaultAudioTrack: value.playDefaultAudioTrack === true,
    audioTrack,
    audioLanguage: language && language !== 'und' ? language : defaults.audio,
    subtitleLanguage: defaults.subtitle,
    subtitlePreference: rememberSubtitleSelections ? value.subtitlePreference ?? subtitleDefault : subtitleDefault,
    foreignSubtitlePreference: rememberedForeign ?? foreignDefault
  };
}

/** The audio is in another language than the one the person reads subtitles in: original version. */
export function isForeignAudio(audio: string | undefined, readingLanguage: string): boolean {
  const spoken = audioLanguage(audio ?? '');
  const read = audioLanguage(readingLanguage);
  return Boolean(spoken && spoken !== 'und' && read && read !== 'und' && spoken !== read);
}
