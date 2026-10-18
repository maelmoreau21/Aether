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

export interface Library {
  id: string;
  name: string;
  kind: 'films' | 'series';
  nodeId: string;
  root: string;
  enabled: boolean;
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
}
