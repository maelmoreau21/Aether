// Time series helpers shared by the statistics page and the per-title panel. No DOM access.
import { addDays } from './insights';

export interface DayRow { date: string; plays: number; seconds: number }
export interface SeriesPoint { key: string; from: string; to: string; plays: number; seconds: number }

/** Every local day of `from`…`to` (inclusive), missing days as zero. */
export function dailySeries(rows: DayRow[], from: string, to: string, limit = 3700): SeriesPoint[] {
  const values = new Map(rows.map(row => [row.date, row]));
  const out: SeriesPoint[] = [];
  for (let cursor = from; cursor <= to && out.length < limit; cursor = addDays(cursor, 1)) {
    const row = values.get(cursor);
    out.push({ key: cursor, from: cursor, to: cursor, plays: row?.plays ?? 0, seconds: row?.seconds ?? 0 });
  }
  return out;
}

/** Merge consecutive points into at most `count` buckets (sums), oldest first. */
export function bucket(points: SeriesPoint[], count: number): SeriesPoint[] {
  if (count <= 0 || points.length <= count) return points;
  const size = Math.ceil(points.length / count), out: SeriesPoint[] = [];
  for (let start = 0; start < points.length; start += size) {
    const slice = points.slice(start, start + size);
    out.push({ key: slice[0].key, from: slice[0].from, to: slice[slice.length - 1].to, plays: slice.reduce((sum, point) => sum + point.plays, 0), seconds: slice.reduce((sum, point) => sum + point.seconds, 0) });
  }
  return out;
}

/** Daily up to ~four months, then weekly buckets, so the chart stays readable. */
export function chartSeries(rows: DayRow[], from: string, to: string): { points: SeriesPoint[]; grouped: boolean } {
  const days = dailySeries(rows, from, to);
  if (days.length <= 120) return { points: days, grouped: false };
  const weeks = Math.ceil(days.length / 7);
  return { points: bucket(days, weeks > 104 ? 104 : weeks), grouped: true };
}
