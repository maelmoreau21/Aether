import type { Subtitle } from './types';

export type SubtitlePreference = { enabled: boolean; language?: string; forced?: boolean; sdh?: boolean };

/**
 * `preferSdh`: subtitles for the deaf and hard of hearing come first in the chosen language
 * (full subtitles only; forced ones stay forced).
 */
export function preferredSubtitle(tracks: Subtitle[], preference?: SubtitlePreference, defaultLanguage = 'fr', preferSdh = false): string | null {
  if (preference?.enabled === false) return null;
  if (preference && preferSdh && !preference.forced) {
    const sdh = tracks.find(track => track.language === preference.language && !track.forced && track.sdh);
    if (sdh) return sdh.id;
  }
  if (preference) {
    return tracks.find(track => track.language === preference.language && Boolean(track.forced) === Boolean(preference.forced) && Boolean(track.sdh) === Boolean(preference.sdh))?.id ?? null;
  }
  return tracks.find(track => track.language === defaultLanguage && track.forced)?.id ?? null;
}
