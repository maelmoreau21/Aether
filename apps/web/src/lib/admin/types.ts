import { translatedLabels } from '../i18n';
export const scanLabels = translatedLabels({ idle: 'never_scanned', queued: 'waiting', paused: 'paused', running: 'stats_kpi_active', completed: 'done', error: 'admin_error' });

export const nodeStatusLabels = translatedLabels({ connected: 'connected', disconnected: 'disconnected', revoked: 'revoked' });

export const connectionModeLabels: Readonly<Record<string, string>> = translatedLabels({ outbound: 'outbound_relay_nat', direct: 'direct_https', inbound: 'direct_inbound_legacy' });

export interface EnrollmentToken {
  tokenId: string;
  token: string;
  createdAt: number;
  expiresAt: number;
  coreUrl: string;
}

export interface ProviderPreference { id: string; enabled: boolean }
export interface LibraryOptions {
  preferredMetadataLanguage: string;
  metadataCountryCode: string;
  readLocalNfo: boolean;
  fetchPoster: boolean; fetchBackdrop: boolean; fetchLogo: boolean;
  fetchThumb: boolean; fetchBanner: boolean; fetchDisc: boolean;
  automaticRefreshIntervalDays: number;
  preferEmbeddedTitles: boolean;
  embeddedSubtitlePolicy: 'allow_all' | 'allow_text' | 'block_all';
  metadataProviders: ProviderPreference[]; imageProviders: ProviderPreference[];
  similarItemsProviders: ProviderPreference[]; segmentProviders: ProviderPreference[];
  subtitleProviders: ProviderPreference[];
  onlineRefreshIntervalDays: number;
  realtimeMonitoring: boolean; automaticCollections: boolean; introDetectionEnabled: boolean;
  /** Percent (50–99): automatic markers below it are stored but not used during playback. */
  markerMinConfidence?: number;
  saveNfoToMedia: boolean; saveImagesToMedia: boolean;
  trickplayEnabled: boolean; trickplayDuringScan: boolean; trickplaySaveToMedia: boolean;
  trickplayIntervalSeconds: number; trickplayWidth: number; trickplayQuality: number;
  chapterImagesEnabled: boolean; chapterImagesDuringScan: boolean; chapterImagesSaveToMedia: boolean;
  subtitleDownloadEnabled: boolean; subtitleLanguages: string[];
  subtitleOnlyMissing: boolean; subtitleMatchAudio: boolean; subtitleRequireHash: boolean;
  saveSubtitlesToMedia: boolean;
  posterRotationEnabled?: boolean;
  artworkDeduplicate: boolean;
  artworkSimilarity: number;
  artworkMinPosterWidth: number;
  artworkMinPosterHeight: number;
  artworkLanguages?: string[];
  artworkIncludeNeutral?: boolean;
  artworkMaxImages?: number;
  /** Multi-image mode: several images per kind, collected automatically. Off by default. */
  artworkPoolEnabled?: boolean;
  /** Administrators see the library in their catalogue; management is unaffected. */
  adminAccess?: boolean;
  /** A series library named "Animés": same scan and playback as series, another label. */
  anime?: boolean;
}
export function defaultLibraryOptions(): LibraryOptions {
  return { preferredMetadataLanguage: 'fr', metadataCountryCode: 'FR', readLocalNfo: true,
    fetchPoster: true, fetchBackdrop: true, fetchLogo: true, fetchThumb: true, fetchBanner: true, fetchDisc: true,
    automaticRefreshIntervalDays: 0, preferEmbeddedTitles: false, embeddedSubtitlePolicy: 'allow_all',
    metadataProviders: [], imageProviders: [], similarItemsProviders: [], segmentProviders: [], subtitleProviders: [],
    onlineRefreshIntervalDays: 30, realtimeMonitoring: true, automaticCollections: true, introDetectionEnabled: true, markerMinConfidence: 80,
    saveNfoToMedia: false, saveImagesToMedia: false,
    trickplayEnabled: true, trickplayDuringScan: true, trickplaySaveToMedia: false, trickplayIntervalSeconds: 10, trickplayWidth: 320, trickplayQuality: 7,
    chapterImagesEnabled: true, chapterImagesDuringScan: false, chapterImagesSaveToMedia: false,
    subtitleDownloadEnabled: false, subtitleLanguages: ['fr'], subtitleOnlyMissing: true, subtitleMatchAudio: false,
    subtitleRequireHash: true, saveSubtitlesToMedia: false,
    posterRotationEnabled: true, artworkDeduplicate: true, artworkSimilarity: 5,
    artworkMinPosterWidth: 0, artworkMinPosterHeight: 0,
    artworkLanguages: ['fr', 'en'], artworkIncludeNeutral: true, artworkMaxImages: 12, artworkPoolEnabled: false, adminAccess: true, anime: false };
}
/** Apply the processing preset without changing language, provider scopes or media paths. */
export function recommendedLibraryOptions(current: LibraryOptions): LibraryOptions {
  const defaults = defaultLibraryOptions();
  const processingKeys = ['realtimeMonitoring', 'automaticCollections', 'introDetectionEnabled',
    'onlineRefreshIntervalDays', 'trickplayEnabled', 'trickplayDuringScan',
    'trickplayIntervalSeconds', 'trickplayWidth', 'trickplayQuality', 'chapterImagesEnabled'] as const;
  const next = { ...current };
  for (const key of processingKeys) Object.assign(next, { [key]: defaults[key] });
  return next;
}
export interface LibraryFolder { nodeId: string; root: string }
export interface Library {
  folders?: LibraryFolder[];
  id: string;
  name: string;
  kind: 'films' | 'series' | 'music';
  nodeId: string;
  root: string;
  enabled: boolean;
  excludedPatterns?: string;
  options?: LibraryOptions;
  coverUrls?: string[];
  imageKinds?: string[];
  imageVersions?: Record<string,string>;
  userIds: string[];
}

export interface Scan {
  status: 'idle' | 'queued' | 'paused' | 'running' | 'completed' | 'error';
  discovered: number;
  probed: number;
  reused: number;
  errors: string[];
  errorsCount?: number;
  warningsCount?: number;
  /** Why a queued scan has not started; the counters then describe the previous scan. */
  waitingReason?: 'node_busy' | 'node_offline' | 'retry' | 'prerequisite' | 'dispatching';
  /** Category of the task occupying the node, or the last error before a retry. */
  waitingDetail?: string;
}

export interface NodeLibraryRef {
  id: string;
  name: string;
  kind: string;
  root: string;
  enabled: boolean;
}

export interface NodeScanSummary {
  libraryId: string;
  status: string;
  discovered: number;
  probed: number;
  reused: number;
  errorsCount: number;
  warningsCount: number;
}

export interface AdminNode {
  id: string;
  name: string;
  kind: 'local' | 'remote';
  baseUrl?: string | null;
  publicUrl?: string | null;
  status: 'connected' | 'disconnected' | 'revoked';
  version?: string | null;
  connectionMode?: 'outbound' | 'direct' | 'inbound' | string | null;
  keyId: string;
  roots: string[];
  pairedAt: number;
  lastSeenAt?: number | null;
  revokedAt?: number | null;
  libraries: NodeLibraryRef[];
  scans: NodeScanSummary[];
}

export interface OidcAdminStatus {
  enabled: boolean;
  issuer?: string | null;
  clientId?: string | null;
  redirectUri?: string | null;
  scopes?: string | null;
  groupsClaim?: string | null;
  requireGroup?: boolean;
  userGroups?: string[];
  endSessionEndpoint?: string | null;
  localLoginEnabled?: boolean;
}

export interface CoreHealth {
  status: string;
  mode: string;
  version?: string;
}

export type RefreshMode = 'incremental' | 'new_only' | 'missing_metadata' | 'full';
