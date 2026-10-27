import { getSessionGeneration, onSessionGenerationChange } from '../auth/session';

export type ArtworkRotationPolicy = {
  enabled: boolean; libraries: string[]; enableMovies: boolean; enableSeries: boolean;
  rotationMode: 'random' | 'sequential'; minHoursBetweenSwitches: number; poolSize: number;
  preferredLanguage: string; fallbackLanguage: string;
  includeUnknownLanguage: boolean; allowAnyLanguageFallback: boolean;
};

const disabled: ArtworkRotationPolicy = {
  enabled: false, libraries: [], enableMovies: true, enableSeries: true,
  rotationMode: 'random', minHoursBetweenSwitches: 0, poolSize: 4,
  preferredLanguage: 'fr', fallbackLanguage: 'en',
  includeUnknownLanguage: true, allowAnyLanguageFallback: true,
};
let current = $state<ArtworkRotationPolicy>(disabled);
export function artworkRotationPolicy(): ArtworkRotationPolicy { return current; }
export function setArtworkRotationPolicy(value: ArtworkRotationPolicy): void { current = value; }
onSessionGenerationChange(() => { current = disabled; });

export async function loadArtworkRotationPolicy(signal?: AbortSignal): Promise<void> {
  current = disabled;
  const generation = getSessionGeneration();
  try {
    const response = await fetch('/api/v1/artwork/rotation-policy', { credentials: 'same-origin', signal: signal ? AbortSignal.any([signal, AbortSignal.timeout(10_000)]) : AbortSignal.timeout(10_000) });
    if (!response.ok || signal?.aborted || generation !== getSessionGeneration()) return;
    const policy: unknown = await response.json();
    if (signal?.aborted || generation !== getSessionGeneration() || !policy || typeof policy !== 'object') return;
    const input = policy as Partial<ArtworkRotationPolicy>;
    if (!input.enabled || !Array.isArray(input.libraries)) return;
    current = { ...disabled, ...input, libraries: input.libraries.filter((id): id is string => typeof id === 'string') };
  } catch { /* Keep artwork fixed if the policy is unavailable. */ }
}
