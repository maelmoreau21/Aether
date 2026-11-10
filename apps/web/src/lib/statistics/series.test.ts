import { describe, expect, it } from 'vitest';
import { bucket, chartSeries, dailySeries } from './series';

describe('statistics series', () => {
  it('fills every day of the range, missing days as zero', () => {
    const points = dailySeries([{ date: '2026-03-02', plays: 2, seconds: 600 }], '2026-03-01', '2026-03-03');
    expect(points.map(point => [point.key, point.plays, point.seconds])).toEqual([
      ['2026-03-01', 0, 0], ['2026-03-02', 2, 600], ['2026-03-03', 0, 0]
    ]);
  });
  it('crosses DST and month ends with calendar arithmetic', () => {
    const points = dailySeries([], '2026-03-28', '2026-04-02');
    expect(points.map(point => point.key)).toEqual(['2026-03-28', '2026-03-29', '2026-03-30', '2026-03-31', '2026-04-01', '2026-04-02']);
  });
  it('buckets keep totals and bounds', () => {
    const days = dailySeries([{ date: '2026-01-01', plays: 1, seconds: 60 }, { date: '2026-01-10', plays: 3, seconds: 120 }], '2026-01-01', '2026-01-10');
    const merged = bucket(days, 3);
    expect(merged.length).toBe(3);
    expect(merged[0].from).toBe('2026-01-01');
    expect(merged.at(-1)?.to).toBe('2026-01-10');
    expect(merged.reduce((sum, point) => sum + point.plays, 0)).toBe(4);
    expect(merged.reduce((sum, point) => sum + point.seconds, 0)).toBe(180);
    expect(bucket(days, 20)).toBe(days);
  });
  it('switches to weekly points beyond four months', () => {
    expect(chartSeries([], '2026-01-01', '2026-03-31').grouped).toBe(false);
    const year = chartSeries([], '2025-01-01', '2025-12-31');
    expect(year.grouped).toBe(true);
    expect(year.points.length).toBeLessThanOrEqual(53);
  });
});
