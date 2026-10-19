import { get, writable } from 'svelte/store';
import { session, getSessionGeneration, onSessionGenerationChange } from './auth/session';
import type { SubtitlePreference } from './player/subtitle-preference';

export type Preferences = {
  theme?: 'light' | 'dark' | 'system'; locale?: 'fr' | 'en'; audioLanguage?: string;
  subtitles?: 'forced' | 'always' | 'off'; audioTrack?: { mediaId: string; trackId: string };
  offsets?: { audio: number; subtitle: number }; subtitlePreference?: SubtitlePreference;
};
let current: Preferences = {};
let owner: string | null = null;
let queue: Promise<void> = Promise.resolve();
export const preferenceSyncError = writable('');

onSessionGenerationChange(() => {
  current = {}; owner = null; preferenceSyncError.set('');
  // Global browser keys are compatibility mirrors, never a source for a new account.
  if (typeof localStorage !== 'undefined') {
    try { for (const key of ['media_theme', 'media_locale', 'media_pref_audio', 'media_pref_subtitles']) localStorage.removeItem(key); } catch { /* Optional browser cache. */ }
  }
});

function mirror(value: Preferences, userId: string) {
  if (typeof localStorage === 'undefined') return;
  try {
    const values: Record<string, string | undefined> = {
      media_theme: value.theme, media_locale: value.locale, media_pref_audio: value.audioLanguage,
      media_pref_subtitles: value.subtitles,
      [`media_audio_${userId}`]: value.audioLanguage,
      [`media_audio_track_${userId}`]: value.audioTrack ? JSON.stringify(value.audioTrack) : undefined,
      [`media_offsets_${userId}`]: value.offsets ? JSON.stringify(value.offsets) : undefined,
      [`media_subtitle_${userId}`]: value.subtitlePreference ? JSON.stringify(value.subtitlePreference) : undefined
    };
    for (const [key, content] of Object.entries(values)) {
      if (content === undefined) localStorage.removeItem(key); else localStorage.setItem(key, content);
    }
  } catch { /* Server preferences work even when browser cache is unavailable. */ }
}

export function preferences(): Preferences { return current; }

function legacyPreferences(userId: string, loaded: Preferences): Preferences {
  if (typeof localStorage === 'undefined') return {};
  const patch: Preferences = {};
  const read = (key: string): string | null => {
    try { const value = localStorage.getItem(key); return value !== null && value.length <= 1024 ? value : null; } catch { return null; }
  };
  const object = (key: string): Record<string, unknown> | null => {
    try { const value: unknown = JSON.parse(read(key) ?? 'null'); return value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null; } catch { return null; }
  };
  const language = (value: unknown): value is string => typeof value === 'string' && /^[a-zA-Z0-9-]{2,35}$/.test(value);
  if (loaded.audioLanguage === undefined) {
    const audio = read(`media_audio_${userId}`);
    if (language(audio)) patch.audioLanguage = audio;
  }
  if (loaded.audioTrack === undefined) {
    const track = object(`media_audio_track_${userId}`);
    const identifier = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && new TextEncoder().encode(value).length <= 256 && !Array.from(value).some(character => character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127);
    if (track && identifier(track.mediaId) && identifier(track.trackId)) patch.audioTrack = { mediaId: track.mediaId, trackId: track.trackId };
  }
  if (loaded.offsets === undefined) {
    const offsets = object(`media_offsets_${userId}`);
    if (offsets && typeof offsets.audio === 'number' && typeof offsets.subtitle === 'number' && Number.isFinite(offsets.audio) && Number.isFinite(offsets.subtitle) && Math.abs(offsets.audio) <= 30 && Math.abs(offsets.subtitle) <= 30) patch.offsets = { audio: offsets.audio, subtitle: offsets.subtitle };
  }
  if (loaded.subtitlePreference === undefined) {
    const subtitle = object(`media_subtitle_${userId}`);
    if (subtitle && typeof subtitle.enabled === 'boolean' && (subtitle.language === undefined || language(subtitle.language)) && (subtitle.forced === undefined || typeof subtitle.forced === 'boolean') && (subtitle.sdh === undefined || typeof subtitle.sdh === 'boolean')) {
      patch.subtitlePreference = { enabled: subtitle.enabled };
      if (language(subtitle.language)) patch.subtitlePreference.language = subtitle.language;
      if (typeof subtitle.forced === 'boolean') patch.subtitlePreference.forced = subtitle.forced;
      if (typeof subtitle.sdh === 'boolean') patch.subtitlePreference.sdh = subtitle.sdh;
    }
  }
  return patch;
}

export async function hydratePreferences(): Promise<Preferences | null> {
  const authenticated = get(session);
  const generation = getSessionGeneration();
  if (!authenticated) return null;
  const response = await fetch('/api/v1/preferences', { credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000) });
  if (generation !== getSessionGeneration()) return null;
  if (!response.ok) throw new Error('Impossible de charger vos préférences.');
  const loaded: Preferences = await response.json();
  if (generation !== getSessionGeneration()) return null;
  const migrated = legacyPreferences(authenticated.user.id, loaded);
  if (generation !== getSessionGeneration()) return null;
  if (Object.keys(migrated).length > 0) {
    const saved = await fetch('/api/v1/preferences', {
      method: 'PUT', credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000),
      headers: { 'content-type': 'application/json', 'x-csrf-token': authenticated.csrfToken }, body: JSON.stringify(migrated)
    });
    if (generation !== getSessionGeneration()) return null;
    if (!saved.ok) throw new Error('La migration de vos préférences locales a échoué. Les valeurs du navigateur sont conservées. Réessayez pour les enregistrer sur le serveur.');
  }
  current = { ...loaded, ...migrated }; owner = authenticated.user.id;
  mirror(current, owner);
  return current;
}

export function savePreferences(patch: Preferences): void {
  const authenticated = get(session);
  const generation = getSessionGeneration();
  if (!authenticated || owner !== authenticated.user.id) return;
  current = { ...current, ...patch }; mirror(current, owner);
  const save = async () => {
    if (generation !== getSessionGeneration()) return;
    try {
      const response = await fetch('/api/v1/preferences', {
        method: 'PUT', credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(10000),
        headers: { 'content-type': 'application/json', 'x-csrf-token': authenticated.csrfToken }, body: JSON.stringify(patch)
      });
      if (!response.ok) throw new Error('Vos préférences ne sont pas encore enregistrées sur le serveur. Modifiez à nouveau le réglage pour réessayer.');
      if (generation === getSessionGeneration()) preferenceSyncError.set('');
    } catch (error) {
      if (generation === getSessionGeneration()) preferenceSyncError.set(error instanceof Error ? error.message : 'Enregistrement des préférences impossible.');
    }
  };
  queue = queue.then(save, save);
}
