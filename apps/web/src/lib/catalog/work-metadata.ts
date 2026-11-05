/** Conversion pure entre une œuvre et le formulaire d'édition des métadonnées (WorkTools). */
import type { Work } from './api';

export interface MetadataForm {
  title: string;
  plot: string;
  year: number | undefined;
  genres: string;
  tags: string;
  ids: string;
  age: number | undefined;
}

export interface MetadataPayload {
  title: string;
  plot: string;
  year: number | null;
  genres: string[];
  tags: string[];
  externalIds: unknown;
  minimumAge: number | null;
}

/** Liste séparée par des virgules -> éléments non vides. */
export function splitCsv(value: string): string[] {
  return value.split(',').map(s => s.trim()).filter(Boolean);
}

/** Valeurs initiales du formulaire (le texte « Média local indexé » n'est pas un vrai synopsis). */
export function metadataFormFromWork(work: Work): MetadataForm {
  return {
    title: work.title,
    plot: (work.plot ?? work.description ?? '').replace(/^Média local indexé$/i, ''),
    year: work.year ?? undefined,
    genres: (work.genres ?? []).join(', '),
    tags: (work.tags ?? []).join(', '),
    ids: JSON.stringify(work.externalIds ?? {}, null, 2),
    age: work.minimumAge ?? undefined
  };
}

/** Charge utile PUT ; `null` si le JSON des identifiants externes est invalide. */
export function metadataPayloadFromForm(form: MetadataForm): MetadataPayload | null {
  let externalIds;
  try {
    externalIds = JSON.parse(form.ids);
  } catch {
    return null;
  }
  return {
    title: form.title,
    plot: form.plot,
    year: form.year ?? null,
    genres: splitCsv(form.genres),
    tags: splitCsv(form.tags),
    externalIds,
    minimumAge: form.age ?? null
  };
}
