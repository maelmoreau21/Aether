import type { Subtitle } from './types';

export type SubtitlePreference = { enabled: boolean; language?: string; forced?: boolean; sdh?: boolean };

export function preferredSubtitle(tracks: Subtitle[], preference?: SubtitlePreference): string | null {
  if (preference?.enabled === false) return null;
  if (preference) {
    return tracks.find(track => track.language === preference.language && Boolean(track.forced) === Boolean(preference.forced) && Boolean(track.sdh) === Boolean(preference.sdh))?.id ?? null;
  }
  return tracks.find(track => track.language === 'fr' && track.forced)?.id ?? null;
}
