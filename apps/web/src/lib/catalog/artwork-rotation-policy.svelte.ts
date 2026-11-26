import { getSessionGeneration, onSessionGenerationChange } from '../auth/session';
import { ensurePreferences } from '../preferences';

export type ArtworkRotationPolicy = {
  enabled: boolean; languageMode?: 'interface' | 'original'; enableMovies?: boolean; enableSeries?: boolean; enabledLibraries?: string[];
  rotationMode: 'random' | 'sequential'; minHoursBetweenSwitches: number; poolSize: number;
  preferredLanguage: string; fallbackLanguage: string;
  includeUnknownLanguage: boolean; allowAnyLanguageFallback: boolean;
};

export const defaultArtworkRotationPolicy: ArtworkRotationPolicy = {
  enabled: true, languageMode: 'interface', enableMovies: true, enableSeries: true,
  rotationMode: 'random', minHoursBetweenSwitches: 1, poolSize: 12,
  preferredLanguage: 'fr', fallbackLanguage: 'en',
  includeUnknownLanguage: true, allowAnyLanguageFallback: true,
};
let current = $state<ArtworkRotationPolicy>(defaultArtworkRotationPolicy);
let loadedGeneration = -1;
export function artworkRotationPolicy(): ArtworkRotationPolicy { return current; }

function samePolicy(a: ArtworkRotationPolicy, b: ArtworkRotationPolicy): boolean {
  if (a === b) return true;
  if (
    a.enabled !== b.enabled ||
    a.languageMode !== b.languageMode ||
    a.enableMovies !== b.enableMovies ||
    a.enableSeries !== b.enableSeries ||
    a.rotationMode !== b.rotationMode ||
    a.minHoursBetweenSwitches !== b.minHoursBetweenSwitches ||
    a.poolSize !== b.poolSize ||
    a.preferredLanguage !== b.preferredLanguage ||
    a.fallbackLanguage !== b.fallbackLanguage ||
    a.includeUnknownLanguage !== b.includeUnknownLanguage ||
    a.allowAnyLanguageFallback !== b.allowAnyLanguageFallback
  ) {
    return false;
  }
  const libsA = a.enabledLibraries;
  const libsB = b.enabledLibraries;
  if (libsA === libsB) return true;
  if (!libsA || !libsB || libsA.length !== libsB.length) return false;
  return libsA.every((id, idx) => id === libsB[idx]);
}

export function setArtworkRotationPolicy(value: ArtworkRotationPolicy): void {
  if (!samePolicy(current, value)) {
    current = value;
  }
}
onSessionGenerationChange(() => {
  loadedGeneration = -1;
  setArtworkRotationPolicy(defaultArtworkRotationPolicy);
});

export async function loadArtworkRotationPolicy(signal?: AbortSignal): Promise<void> {
  const generation = getSessionGeneration();
  if (loadedGeneration !== generation) {
    setArtworkRotationPolicy(defaultArtworkRotationPolicy);
  }
  try {
    const saved = await ensurePreferences();
    if (!signal?.aborted && generation === getSessionGeneration()) {
      loadedGeneration = generation;
      setArtworkRotationPolicy({ ...defaultArtworkRotationPolicy, ...saved?.artworkRotation });
    }
  } catch { /* Keep the default policy if preferences are unavailable. */ }
}
