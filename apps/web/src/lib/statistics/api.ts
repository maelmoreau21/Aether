import { get } from 'svelte/store';
import { session } from '../auth/session';
export interface Distribution { label: string; plays: number; seconds: number }
export interface SessionSummary {
  id: string; userId: string; user: string; title: string; kind: string; artist: string; album: string;
  libraryId: string; nodeId: string; device: string; startedAt: number; endedAt: number | null;
  status: string; endReason: string; seconds: number; coveredSeconds: number; duration: number; completed: boolean;
  metrics: Record<string, unknown>;
}
export interface Statistics {
  enabled: boolean; firstEnabledAt: number | null; eventRetentionDays: number; summaryRetentionDays: number;
  periods: { startedAt: number; endedAt: number | null }[];
  summary: { plays: number; users: number; media: number; seconds: number; completed: number; active: number };
  popular: { id: string; kind: string; title: string; plays: number; users: number; seconds: number; completed: number; favorites: number; rating: number }[];
  users: { id: string; name: string; plays: number; seconds: number }[];
  daily: { day: number; seconds: number }[]; hours: { hour: number; plays: number; seconds: number }[];
  behavior: Record<string, number>; distributions: Record<string, Distribution[]>; music: Record<string, Distribution[]>;
  facets: { libraries: { id: string; name: string }[]; nodes: string[]; devices: string[] };
  sessions: SessionSummary[]; hasMore: boolean;
}
export interface Detail {
  session: SessionSummary; detailsExpired: boolean; eventsTruncated: boolean; nextSequence: number | null;
  events: { occurredAt: number; event: { sequence: number; kind: string; position: number; origin: string; track?: { id: string; language: string }; previousTrack?: { id: string; language: string }; fromPosition?: number; errorCode?: string } }[];
}
export async function statisticsRequest(path: string, options: RequestInit = {}): Promise<Response> {
  const current = get(session);
  if (!current) throw new Error('Session requise.');
  const response = await fetch(path, { ...options, credentials: 'same-origin', cache: 'no-store',
    headers: { 'content-type': 'application/json', 'x-csrf-token': current.csrfToken, ...options.headers } });
  if (!response.ok) throw new Error(response.status === 403 ? 'Accès aux statistiques refusé.' : response.status === 404 ? 'Session indisponible ou accès retiré.' : 'Statistiques indisponibles. Réessayez.');
  return response;
}
export function duration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)} s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ${Math.round(seconds % 60)} s`;
  return `${Math.floor(seconds / 3600)} h ${Math.floor(seconds % 3600 / 60)} min`;
}
export const eventLabels: Record<string, string> = {
  start: 'Ouverture', heartbeat: 'Lecture observée', pause: 'Pause', resume: 'Lecture', seek: 'Déplacement',
  buffering: 'Chargement', buffered: 'Chargement terminé', audio_change: 'Piste audio', subtitle_change: 'Sous-titres',
  speed_change: 'Vitesse', source_change: 'Source', skip_intro: 'Introduction passée', skip_credits: 'Générique passé',
  skip_recap: 'Résumé passé', skip_preview: 'Aperçu passé', stop: 'Arrêt', ended: 'Fin du média', error: 'Erreur'
};
