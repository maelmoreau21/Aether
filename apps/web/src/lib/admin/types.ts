export const scanLabels = {
  idle: 'Jamais scanné',
  queued: 'En attente',
  running: 'En cours',
  completed: 'Terminé',
  error: 'Erreur'
} as const;

export const nodeStatusLabels = {
  connected: 'Connecté',
  disconnected: 'Hors ligne',
  revoked: 'Révoqué'
} as const;

export const connectionModeLabels: Record<string, string> = {
  outbound: 'Relais sortant (NAT)',
  direct: 'Direct HTTPS',
  inbound: 'Entrant direct (Legacy)'
};

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
  saveNfoToMedia: boolean; saveImagesToMedia: boolean;
  trickplayEnabled: boolean; trickplayDuringScan: boolean; trickplaySaveToMedia: boolean;
  trickplayIntervalSeconds: number; trickplayWidth: number;
  chapterImagesEnabled: boolean; chapterImagesDuringScan: boolean; chapterImagesSaveToMedia: boolean;
  subtitleDownloadEnabled: boolean; subtitleLanguages: string[];
  subtitleOnlyMissing: boolean; subtitleMatchAudio: boolean; subtitleRequireHash: boolean;
  saveSubtitlesToMedia: boolean;
}
export function defaultLibraryOptions(): LibraryOptions {
  return { preferredMetadataLanguage: 'fr', metadataCountryCode: 'FR', readLocalNfo: true,
    fetchPoster: true, fetchBackdrop: true, fetchLogo: true, fetchThumb: true, fetchBanner: true, fetchDisc: true,
    automaticRefreshIntervalDays: 0, preferEmbeddedTitles: false, embeddedSubtitlePolicy: 'allow_all',
    metadataProviders: [], imageProviders: [], similarItemsProviders: [], segmentProviders: [], subtitleProviders: [],
    onlineRefreshIntervalDays: 0, realtimeMonitoring: false, automaticCollections: false, introDetectionEnabled: false,
    saveNfoToMedia: false, saveImagesToMedia: false,
    trickplayEnabled: false, trickplayDuringScan: false, trickplaySaveToMedia: false, trickplayIntervalSeconds: 10, trickplayWidth: 320,
    chapterImagesEnabled: false, chapterImagesDuringScan: false, chapterImagesSaveToMedia: false,
    subtitleDownloadEnabled: false, subtitleLanguages: ['fr'], subtitleOnlyMissing: true, subtitleMatchAudio: false,
    subtitleRequireHash: true, saveSubtitlesToMedia: false };
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
  status: 'idle' | 'queued' | 'running' | 'completed' | 'error';
  discovered: number;
  probed: number;
  reused: number;
  errors: string[];
  errorsCount?: number;
  warningsCount?: number;
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
