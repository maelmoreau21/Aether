import type { Preferences } from '../preferences';
import type { SubtitlePreference } from './subtitle-preference';
import { audioLanguage } from './source-selection';

export const SKIP_SECONDS = [5, 10, 15, 30, 60] as const;

/** Default enables automatic episode chaining and ten-second control buttons. */
export function playbackPreferences(value: {
  autoplayNextEpisode?: unknown; skipBackSeconds?: unknown; skipForwardSeconds?: unknown;
}) {
  const skip = (seconds: unknown): number =>
    typeof seconds === 'number' && SKIP_SECONDS.some(option => option === seconds) ? seconds : 10;
  return {
    autoplayNextEpisode: value.autoplayNextEpisode === undefined ? true : value.autoplayNextEpisode === true,
    skipBackSeconds: skip(value.skipBackSeconds),
    skipForwardSeconds: skip(value.skipForwardSeconds)
  };
}

/** What the player does when it reaches a detected segment, chosen per kind of segment. */
export type SegmentBehavior = 'auto_skip' | 'ask' | 'off';
export const segmentBehaviors: readonly SegmentBehavior[] = ['auto_skip', 'ask', 'off'];
export type SkippableKind = 'intro' | 'recap' | 'credits';
export const skippableKinds: readonly SkippableKind[] = ['intro', 'recap', 'credits'];

const behaviorKeys = {
  intro: { show: 'skipIntroEnabled', auto: 'autoSkipIntro' },
  recap: { show: 'skipRecapEnabled', auto: 'autoSkipRecap' },
  credits: { show: 'skipCreditsEnabled', auto: 'autoSkipCredits' }
} as const;

/**
 * The three-way choice is stored in the two booleans accounts already have (the segment is
 * offered, and it is skipped without asking), so nothing is migrated: not offered is `off`,
 * offered is `ask`, offered and automatic is `auto_skip`.
 */
export function segmentBehavior(value: Preferences, kind: SkippableKind): SegmentBehavior {
  const keys = behaviorKeys[kind];
  if (value[keys.show] === false) return 'off';
  return value[keys.auto] === true ? 'auto_skip' : 'ask';
}

/** The preference changes that make `kind` behave as `behavior`. */
export function segmentBehaviorPatch(kind: SkippableKind, behavior: SegmentBehavior): Partial<Preferences> {
  const keys = behaviorKeys[kind];
  return { [keys.show]: behavior !== 'off', [keys.auto]: behavior === 'auto_skip' };
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
  const subtitleSetting = value.subtitles ?? 'foreign';
  const subtitleDefault: SubtitlePreference | undefined = subtitleSetting === 'off' ? { enabled: false }
    : subtitleSetting === 'always' ? { enabled: true, language: defaults.subtitle, forced: false } : undefined;
  // Original-language audio (VO) has its own default and its own remembered choice: turning
  // subtitles on for one film in VO turns them on for every film in VO, never in the
  // person's own language. Accounts that never chose in VO keep their single old choice.
  const foreignDefault: SubtitlePreference | undefined = subtitleSetting === 'foreign'
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
