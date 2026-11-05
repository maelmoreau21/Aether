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
  wrappedAvailable?: boolean;
  periods: { startedAt: number; endedAt: number | null }[];
  summary: { plays: number; qualifiedPlays?: number; users: number; media: number; seconds: number; completed: number; active: number };
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
/** Ranked entry shared by insights and Wrapped (titles, shows, genres, artists…). */
export interface Rank { id: string; title: string; subtitle: string; image: string; kind: string; seconds: number; plays: number; users: number; items: number }
export interface Person { name: string; role: 'director' | 'actor'; seconds: number; titles: number }
export interface Bucket { label: string; plays: number; seconds: number }
export interface Insights {
  timezone: string; minimumPlaySeconds: number; truncated: boolean; period: { from: number; to: number };
  totals: { seconds: number; plays: number; opened: number; zapped: number; users: number; media: number; completed: number };
  growth: { previous: { seconds: number; plays: number; users: number; media: number; completed: number }; seconds: number | null; plays: number | null; users: number | null; media: number | null; completed: number | null };
  weekdays: { weekday: number; plays: number; seconds: number }[];
  hours: { cell: number; plays: number; seconds: number }[];
  monthly: { month: string; plays: number; seconds: number; users: number }[];
  calendar: { date: string; plays: number; seconds: number }[];
  concurrency: { peak: number; peakAt: number | null; daily: { date: string; peak: number }[] };
  genres: Rank[]; series: Rank[]; titles: Rank[]; people: Person[];
  completion: { buckets: Record<'completed' | 'partial' | 'dropped' | 'abandoned' | 'unknown', number>; dropoff: { id: string; title: string; kind: string; starters: number; below25: number; below50: number; averageCompletion: number }[] };
  resolution: Bucket[]; methods: Bucket[];
  clients: { label: string; plays: number; seconds: number; direct: number; transcoded: number; unknown: number; transcodeShare: number }[];
}
export interface Profile extends Insights {
  user: { id: string; name: string; role: string };
  recent: { id: string; title: string; kind: string; startedAt: number; seconds: number; device: string; completed: boolean }[];
  firstPlay: number | null; lastPlay: number | null;
}
export interface WrappedSettings { visible: boolean; periodEnabled: boolean; startMonth: number; startDay: number; endMonth: number; endDay: number }
export interface WrappedMoment { title: string; kind: string; at: number; image: string }
export interface Wrapped {
  visible: boolean; settings: WrappedSettings; year: number; kind: 'all' | 'film' | 'episode' | 'music'; timezone: string;
  availableYears: number[]; user: { id: string; name: string }; generatedAt: number;
  totals: { seconds: number; hours: number; plays: number; titles: number; days: number; activeDays: number };
  monthly: { month: number; plays: number; seconds: number }[]; hours: number[]; weekdays: number[];
  peaks: { hour: number | null; weekday: number | null; month: number | null };
  streak: { days: number; start: string | null; end: string | null };
  topDevice: string | null; devices: Rank[]; genres: Rank[]; films: Rank[]; series: Rank[];
  binge: { id: string; title: string; date: string; episodes: number; image: string } | null;
  artists: Rank[]; albums: Rank[]; tracks: Rank[]; people: Person[];
  kinds: Record<'film' | 'episode' | 'music', { plays: number; seconds: number }>;
  first: WrappedMoment | null; last: WrappedMoment | null;
  previous: { seconds: number; plays: number; titles: number; secondsGrowth: number | null; playsGrowth: number | null; titlesGrowth: number | null } | null;
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
