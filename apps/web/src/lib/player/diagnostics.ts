import { tr, type TranslationKey } from '../i18n';

/**
 * Why a playback attempt failed, as far as the browser and the Media Node let
 * us tell. The kinds are deliberately few and map to what the viewer or an
 * administrator can act on:
 *
 * - `codec`: the container opened but a stream cannot be decoded here.
 * - `container`: the browser (or the Node's remuxer) does not accept the file layout.
 * - `network`: the bytes did not arrive (connection, timeout, truncated Range).
 * - `permission`: the Core refused this account or the token expired.
 * - `preparation`: the Node could not prepare the requested track (busy, full, tool missing, source changed).
 * - `storage`: the Node could not read the source file (share permissions, NAS I/O error); another Node's copy may work.
 * - `node`: the Media Node holding the file is unreachable or revoked.
 * - `unknown`: nothing above is established; the technical detail is kept.
 *
 * A diagnosis is a hypothesis built from HTTP status, the Node's error code
 * and `MediaError.code`; it is never a proof of the root cause.
 */
export type PlaybackFailureKind = 'codec' | 'container' | 'network' | 'permission' | 'preparation' | 'storage' | 'node' | 'unknown';

export class PlaybackFailure extends Error {
  constructor(
    public readonly kind: PlaybackFailureKind,
    message: string,
    public readonly detail: { status?: number; code?: string; mediaError?: number } = {}
  ) {
    super(message);
    this.name = 'PlaybackFailure';
  }
}

export function failureKindOf(error: unknown): PlaybackFailureKind | undefined {
  return error instanceof PlaybackFailure ? error.kind : undefined;
}

/**
 * Whether trying an equivalent copy on another Node can plausibly help.
 * Copies are the same file or the same runtime, so a decoding or container
 * refusal and a permission refusal would simply repeat on the next Node.
 */
export function failoverHelps(kind: PlaybackFailureKind | undefined): boolean {
  return kind !== 'codec' && kind !== 'container' && kind !== 'permission';
}

const NODE_CODES = new Set(['node_unavailable', 'node_revoked', 'stream_timeout']);
const PREPARATION_CODES = new Set([
  'preparation_busy', 'preparation_cache_limit_reached', 'preparation_timeout',
  'preparation_tool_or_source_unavailable', 'preparation_configuration_invalid',
  'preparation_failed', 'video_stream_capacity_reached', 'prepared_stream_failed',
  'asset_changed_rescan_required', 'source_changed_rescan_required'
]);

const KEY: Record<PlaybackFailureKind, TranslationKey> = {
  codec: 'failure_codec',
  container: 'failure_container',
  network: 'failure_network',
  permission: 'failure_permission',
  preparation: 'failure_preparation',
  storage: 'failure_storage',
  node: 'failure_node',
  unknown: 'failure_unknown'
};

export function failureMessage(kind: PlaybackFailureKind, technical = ''): string {
  const base = tr(KEY[kind]);
  return technical ? `${base} (${technical})` : base;
}

/** Classifies a refused HTTP response from Core or from a Node relayed by Core. */
export function classifyHttp(status: number | undefined, code?: string | null): PlaybackFailure {
  const detail = { status, code: code ?? undefined };
  const technical = [status ? `HTTP ${status}` : '', code ?? ''].filter(Boolean).join(' ');
  let kind: PlaybackFailureKind = 'unknown';
  if (code && NODE_CODES.has(code)) kind = 'node';
  else if (code === 'unsupported_preparation') kind = 'container';
  else if (code === 'source_permission_denied' || code === 'source_io_error') kind = 'storage';
  else if (code && PREPARATION_CODES.has(code)) kind = 'preparation';
  else if (status === 401 || status === 403) kind = 'permission';
  else if (status === 502 || status === 503 || status === 504 || status === 408 || status === 429) kind = 'network';
  else if (status === 404 || status === 410) kind = 'preparation';
  return new PlaybackFailure(kind, failureMessage(kind, technical), detail);
}

export interface MediaErrorContext {
  /** Container-level MIME without codecs, e.g. `video/x-matroska`. */
  mime?: string;
  canPlayType?: (mime: string) => string;
}

// MediaError.code values: 1 aborted, 2 network, 3 decode, 4 source not supported.
export function classifyMediaError(code: number | undefined, context: MediaErrorContext = {}): PlaybackFailure {
  const technical = `MediaError ${code ?? '?'}`;
  let kind: PlaybackFailureKind = 'unknown';
  if (code === 2) kind = 'network';
  else if (code === 3) kind = 'codec';
  else if (code === 4) {
    const container = context.mime?.split(';')[0].trim();
    // The container refused outright when even a codec-less MIME is rejected;
    // otherwise the container is known and a stream inside it is the likely cause.
    kind = container && context.canPlayType && context.canPlayType(container) === '' ? 'container' : 'codec';
  }
  return new PlaybackFailure(kind, failureMessage(kind, technical), { mediaError: code });
}

export interface ProbeOptions {
  fetcher?: typeof fetch;
  signal?: AbortSignal;
  timeoutMs?: number;
}

/**
 * One-byte Range request against the URL that failed. It separates "the bytes
 * cannot be fetched" (permission, Node, network) from "the bytes arrive but the
 * browser rejects them" (codec/container), which `MediaError` alone cannot do.
 * Resolves `undefined` when the URL answers, so the caller keeps its own
 * decoding diagnosis. Never throws; bounded by `timeoutMs` (default 3 s).
 */
export async function probeFailure(url: string, options: ProbeOptions = {}): Promise<PlaybackFailure | undefined> {
  const fetcher = options.fetcher ?? (typeof fetch === 'function' ? fetch : undefined);
  if (!fetcher) return undefined;
  const controller = new AbortController();
  const relay = () => controller.abort();
  options.signal?.addEventListener('abort', relay, { once: true });
  const timer = setTimeout(() => controller.abort(), options.timeoutMs ?? 3000);
  try {
    const response = await fetcher(url, { method: 'GET', headers: { Range: 'bytes=0-0' }, signal: controller.signal });
    if (response?.ok) {
      // Do not download the body: one byte is enough, release the connection.
      try { await response.body?.cancel(); } catch { /* ignore */ }
      return undefined;
    }
    let code = response?.headers?.get?.('x-media-preparation-error') ?? null;
    if (!code) {
      try {
        const body = await response.clone().json();
        if (body && typeof body.error === 'string') code = body.error;
      } catch { /* body is not JSON */ }
    }
    return classifyHttp(response?.status, code);
  } catch {
    if (options.signal?.aborted) return undefined;
    // fetch rejects only when no response was obtained (offline, reset, DNS, timeout).
    return new PlaybackFailure('network', failureMessage('network', controller.signal.aborted ? 'timeout' : 'fetch'));
  } finally {
    clearTimeout(timer);
    options.signal?.removeEventListener('abort', relay);
  }
}

/**
 * Keeps the established kind when an error is re-labelled with context
 * (track, language) so the player can still decide on failover and display.
 */
export function withContext(error: unknown, label: (message: string) => string): Error {
  const text = label(error instanceof Error ? error.message : String(error));
  if (error instanceof PlaybackFailure) return new PlaybackFailure(error.kind, text, error.detail);
  return new Error(text);
}

/**
 * Network, node and permission verdicts from the probe are more specific than
 * a decoding verdict inferred from `MediaError`; the reverse is never true.
 */
export async function refineFailure(error: unknown, url: string | undefined, options: ProbeOptions = {}): Promise<unknown> {
  if (!url) return error;
  const kind = failureKindOf(error);
  if (kind === 'permission' || kind === 'node' || kind === 'preparation') return error;
  const probed = await probeFailure(url, options);
  if (!probed || probed.kind === 'unknown') return error;
  return probed;
}
