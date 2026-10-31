import { describe, expect, it } from 'vitest';

export interface TrickplayManifest {
  intervalMs: number;
  width: number;
  height: number;
  columns: number;
  rows: number;
  sheetCount: number;
  thumbnailCount: number;
  urlPrefix: string;
}

export function computePreviewFrame(
  hoverTime: number | null,
  duration: number,
  trickplay: TrickplayManifest | null,
  failedSheets: Set<number> = new Set()
) {
  if (hoverTime === null || !trickplay || !duration) return null;
  const count = Math.min(
    trickplay.thumbnailCount,
    trickplay.sheetCount * trickplay.columns * trickplay.rows
  );
  const frame = Math.max(
    0,
    Math.min(count - 1, Math.floor((hoverTime * 1000) / trickplay.intervalMs))
  );
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
    fullHeight: trickplay.rows * trickplay.height,
  };
}

export function getAdjacentSheets(currentSheet: number, sheetCount: number): number[] {
  const result: number[] = [currentSheet];
  if (currentSheet > 0) result.push(currentSheet - 1);
  if (currentSheet + 1 < sheetCount) result.push(currentSheet + 1);
  return result;
}

export function clampHoverPosition(clientX: number, rect: { left: number; width: number }, duration: number): number {
  if (duration <= 0 || rect.width <= 0) return 0;
  return Math.max(0, Math.min(duration, ((clientX - rect.left) / rect.width) * duration));
}

describe('Trickplay player previews', () => {
  const sampleTrickplay: TrickplayManifest = {
    intervalMs: 10_000,
    width: 320,
    height: 180,
    columns: 10,
    rows: 10,
    sheetCount: 5,
    thumbnailCount: 450,
    urlPrefix: '/api/v1/assets/test-asset/trickplay/',
  };

  it('calculates the initial frame at 0 seconds', () => {
    const frame = computePreviewFrame(0, 4500, sampleTrickplay);
    expect(frame).not.toBeNull();
    expect(frame?.sheet).toBe(0);
    expect(frame?.x).toBe(0);
    expect(frame?.y).toBe(0);
    expect(frame?.url).toBe('/api/v1/assets/test-asset/trickplay/0');
    expect(frame?.width).toBe(320);
    expect(frame?.height).toBe(180);
    expect(frame?.fullWidth).toBe(3200);
    expect(frame?.fullHeight).toBe(1800);
  });

  it('calculates frame coordinates within the first sheet', () => {
    // 25 seconds -> index 2 (at 10s intervals: 0: 0-9s, 1: 10-19s, 2: 20-29s)
    const frame = computePreviewFrame(25, 4500, sampleTrickplay);
    expect(frame?.sheet).toBe(0);
    expect(frame?.x).toBe(2 * 320);
    expect(frame?.y).toBe(0);
  });

  it('correctly transitions across rows and sheets', () => {
    // Frame 15 (150 seconds) -> row 1, col 5 of sheet 0
    const frameRow1 = computePreviewFrame(150, 4500, sampleTrickplay);
    expect(frameRow1?.sheet).toBe(0);
    expect(frameRow1?.x).toBe(5 * 320);
    expect(frameRow1?.y).toBe(1 * 180);

    // Frame 105 (1050 seconds) -> 100 per sheet -> sheet 1, frame 5 (row 0, col 5)
    const frameSheet1 = computePreviewFrame(1050, 4500, sampleTrickplay);
    expect(frameSheet1?.sheet).toBe(1);
    expect(frameSheet1?.url).toBe('/api/v1/assets/test-asset/trickplay/1');
    expect(frameSheet1?.x).toBe(5 * 320);
    expect(frameSheet1?.y).toBe(0);
  });

  it('clamps to the last thumbnail at duration boundary', () => {
    const frameEnd = computePreviewFrame(5000, 4500, sampleTrickplay);
    // 450 thumbnails total -> last frame is 449
    // 449 / 100 = sheet 4, local = 49 (col 9, row 4)
    expect(frameEnd?.sheet).toBe(4);
    expect(frameEnd?.x).toBe(9 * 320);
    expect(frameEnd?.y).toBe(4 * 180);
  });

  it('suppresses preview if the sheet has failed loading', () => {
    const failed = new Set<number>([1]);
    const frameValid = computePreviewFrame(50, 4500, sampleTrickplay, failed);
    expect(frameValid).not.toBeNull();

    // 1050s is in sheet 1 which failed
    const frameFailed = computePreviewFrame(1050, 4500, sampleTrickplay, failed);
    expect(frameFailed).toBeNull();
  });

  it('computes adjacent sheets for prefetching', () => {
    expect(getAdjacentSheets(0, 5)).toEqual([0, 1]);
    expect(getAdjacentSheets(2, 5)).toEqual([2, 1, 3]);
    expect(getAdjacentSheets(4, 5)).toEqual([4, 3]);
    expect(getAdjacentSheets(0, 1)).toEqual([0]);
  });

  it('clamps clientX accurately across touchscreen drag', () => {
    const rect = { left: 100, width: 800 };
    const duration = 120;

    // Normal point
    expect(clampHoverPosition(500, rect, duration)).toBe(60);

    // Dragged before the track starts
    expect(clampHoverPosition(50, rect, duration)).toBe(0);

    // Dragged past the track end
    expect(clampHoverPosition(950, rect, duration)).toBe(120);
  });
});
