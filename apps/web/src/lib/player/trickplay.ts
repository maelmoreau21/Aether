/** Logique pure des vignettes de survol (trickplay) de la barre de lecture. */
import type { Media } from './types';

export type TrickplayManifest = NonNullable<Media['trickplay']>;

export interface PreviewFrame {
  sheet: number;
  url: string;
  width: number;
  height: number;
  x: number;
  y: number;
  fullWidth: number;
  fullHeight: number;
}

function frameCount(trickplay: TrickplayManifest): number {
  return Math.min(trickplay.thumbnailCount, trickplay.sheetCount * trickplay.columns * trickplay.rows);
}

function frameIndex(trickplay: TrickplayManifest, hoverTime: number): number {
  return Math.max(0, Math.min(frameCount(trickplay) - 1, Math.floor((hoverTime * 1000) / trickplay.intervalMs)));
}

/** Index de la planche (« sheet ») contenant la vignette du temps survolé. */
export function trickplaySheetFor(trickplay: TrickplayManifest, hoverTime: number): number {
  return Math.floor(frameIndex(trickplay, hoverTime) / (trickplay.columns * trickplay.rows));
}

/** Position de la vignette dans sa planche, ou `null` si aucune vignette n'est disponible. */
export function computePreviewFrame(
  hoverTime: number | null,
  duration: number,
  trickplay: TrickplayManifest | null | undefined,
  failedSheets: ReadonlySet<number> = new Set()
): PreviewFrame | null {
  if (hoverTime === null || !trickplay || !duration) return null;
  const frame = frameIndex(trickplay, hoverTime);
  const perSheet = trickplay.columns * trickplay.rows;
  const sheet = Math.floor(frame / perSheet);
  if (failedSheets.has(sheet)) return null;
  const local = frame % perSheet;
  return {
    sheet,
    url: trickplay.urlPrefix + sheet,
    width: trickplay.width,
    height: trickplay.height,
    x: (local % trickplay.columns) * trickplay.width,
    y: Math.floor(local / trickplay.columns) * trickplay.height,
    fullWidth: trickplay.columns * trickplay.width,
    fullHeight: trickplay.rows * trickplay.height
  };
}

/** Planches à précharger autour de la planche courante. */
export function getAdjacentSheets(currentSheet: number, sheetCount: number): number[] {
  const result: number[] = [currentSheet];
  if (currentSheet > 0) result.push(currentSheet - 1);
  if (currentSheet + 1 < sheetCount) result.push(currentSheet + 1);
  return result;
}

/** Temps (s) correspondant à la position horizontale du pointeur sur la barre. */
export function clampHoverPosition(clientX: number, rect: { left: number; width: number }, duration: number): number {
  return Math.max(0, Math.min(duration, ((clientX - rect.left) / rect.width) * duration));
}

/** Valide la réponse `/trickplay` ; renvoie le manifeste s'il est prêt et cohérent pour cet asset. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function readyTrickplay(ok: boolean, value: any, assetId: string): TrickplayManifest | null {
  if (ok && value && (value.mime === undefined || ['image/jpeg', 'image/webp'].includes(value.mime)) && value.status === 'ready' &&
      value.intervalMs >= 1000 && value.width > 0 && value.height > 0 &&
      value.columns > 0 && value.rows > 0 && value.thumbnailCount > 0 &&
      value.sheetCount > 0 && typeof value.urlPrefix === 'string' &&
      value.urlPrefix === '/api/v1/assets/' + encodeURIComponent(assetId) + '/trickplay/') {
    return value as TrickplayManifest;
  }
  return null;
}
