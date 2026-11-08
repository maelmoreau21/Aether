import type { Work } from './api';
import type { Progress } from '../player/progress';

/** Texte réellement renseigné : ni vide, ni une des mentions génériques (« Média local »…). */
export function meaningfulText(value: string | null | undefined, placeholders: readonly string[] = []): value is string {
  const text = value?.trim();
  return Boolean(text) && !placeholders.includes(text as string);
}

export type ResumeTarget = { episode: Work; season: number; progress: Progress };

/**
 * Épisode à reprendre depuis la fiche d'une série : la première entrée d'historique
 * (la plus récente) de cette série encore en cours, si l'épisode est déjà chargé
 * (saisons de la fiche ou page d'épisodes affichée). Sinon `null` : la fiche
 * propose alors simplement d'ouvrir l'onglet Épisodes.
 */
export function resumeTarget(
  series: Work,
  history: readonly Progress[],
  resumable: (progress: Progress) => boolean,
  loaded: readonly Work[] = [],
  loadedSeason?: number
): ResumeTarget | null {
  const progress = history.find(item => item.seriesId === series.id && resumable(item));
  if (!progress) return null;
  for (const season of series.seasons ?? []) {
    const episode = season.episodes.find(item => item.id === progress.workId);
    if (episode) return { episode, season: season.number, progress };
  }
  const episode = loaded.find(item => item.id === progress.workId);
  if (!episode) return null;
  return { episode, season: episode.seasonNumber ?? loadedSeason ?? 1, progress };
}
