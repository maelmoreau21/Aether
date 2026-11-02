import { describe, expect, it } from 'vitest';
import { playbackPreferences, trackPreferences, SKIP_SECONDS } from './playback-preferences';

describe('playback account defaults', () => {
  it('keeps old accounts on manual episode changes and ten-second skips', () => {
    expect(playbackPreferences({})).toEqual({ autoplayNextEpisode: false, skipBackSeconds: 10, skipForwardSeconds: 10 });
  });
  it('applies independent backward and forward durations', () => {
    for (const skipBackSeconds of SKIP_SECONDS) {
      expect(playbackPreferences({ autoplayNextEpisode: true, skipBackSeconds, skipForwardSeconds: 60 }))
        .toEqual({ autoplayNextEpisode: true, skipBackSeconds, skipForwardSeconds: 60 });
    }
  });
  it('ignores malformed values from an obsolete or unavailable server', () => {
    for (const invalid of [0, -10, 600, 1.5, NaN, Infinity, '30', null]) {
      expect(playbackPreferences({ autoplayNextEpisode: 'true', skipBackSeconds: invalid, skipForwardSeconds: invalid }))
        .toEqual({ autoplayNextEpisode: false, skipBackSeconds: 10, skipForwardSeconds: 10 });
    }
  });
});

describe('track selection policy', () => {
  const audioTrack = { mediaId: 'movie', trackId: 'internal-2', language: 'eng' };
  const subtitlePreference = { enabled: true, language: 'en', forced: false, sdh: true };
  it('keeps remembering choices by default, independently of configured languages', () => {
    expect(trackPreferences({ audioLanguage: 'fr', subtitles: 'forced', audioTrack, subtitlePreference }))
      .toMatchObject({ rememberAudioSelections: true, rememberSubtitleSelections: true, playDefaultAudioTrack: false,
        audioTrack, audioLanguage: 'en', subtitlePreference });
  });
  it('ignores saved choices when memory is disabled and uses configured defaults', () => {
    expect(trackPreferences({ audioLanguage: 'fr', subtitles: 'always', audioTrack, subtitlePreference,
      rememberAudioSelections: false, rememberSubtitleSelections: false, playDefaultAudioTrack: true }))
      .toMatchObject({ audioTrack: undefined, audioLanguage: 'fr', subtitlePreference: { enabled: true, language: 'fr', forced: false }, playDefaultAudioTrack: true });
  });
  it('remembers an explicit subtitle off choice and preserves old audioTrack payloads', () => {
    expect(trackPreferences({ subtitles: 'always', subtitlePreference: { enabled: false } }).subtitlePreference).toEqual({ enabled: false });
    expect(trackPreferences({ audioLanguage: 'fr-CA', audioTrack: { mediaId: 'movie', trackId: 'old' } }).audioLanguage).toBe('fr');
    expect(trackPreferences({ subtitles: 'off', rememberSubtitleSelections: false, subtitlePreference }).subtitlePreference).toEqual({ enabled: false });
  });
});
