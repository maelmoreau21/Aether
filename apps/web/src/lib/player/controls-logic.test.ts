import { describe, expect, it } from 'vitest';
import {
  aspectRatioLabel, clock, menuTitle, nextAspectRatio, percentOf, projectedEndTime,
  speedOptionLabel, speedSettingLabel
} from './controls-format';
import {
  chapterAt, chapterSkipTarget, inRange, markerSegmentStyle, rangeInSegment, segmentFill, segmentIndexAt,
  timelineChapters, timelineSegments
} from './chapters';
import { nextMenuIndex } from './menu-navigation';

describe('controls-format', () => {
  it('formats clock values', () => {
    expect(clock(0)).toBe('00:00');
    expect(clock(65)).toBe('01:05');
    expect(clock(3725)).toBe('1:02:05');
    expect(clock(NaN)).toBe('00:00');
  });

  it('projects the end time with speed, or returns an empty string', () => {
    const now = new Date(2026, 0, 1, 20, 0, 0).getTime();
    expect(projectedEndTime(now, 3600, 7200, 1)).toBe('21h00');
    expect(projectedEndTime(now, 3600, 7200, 2)).toBe('20h30');
    expect(projectedEndTime(now, 3600, 7200, 0)).toBe('21h00');
    expect(projectedEndTime(now, 0, 7200, 1)).toBe('');
    expect(projectedEndTime(now, 10, 0, 1)).toBe('');
  });

  it('labels menus, speeds and aspect ratios', () => {
    expect(menuTitle('audio')).toBe('Langue audio');
    expect(menuTitle('speed')).toBe('Vitesse de lecture');
    expect(menuTitle(null)).toBe('Vitesse de lecture');
    expect(speedOptionLabel(1)).toBe('Normale · 1×');
    expect(speedSettingLabel(1)).toBe('1x');
    expect(speedSettingLabel(2)).toBe('2x');
    expect(nextAspectRatio('contain')).toBe('cover');
    expect(nextAspectRatio('cover')).toBe('fill');
    expect(nextAspectRatio('fill')).toBe('contain');
    expect(aspectRatioLabel('fill')).toBe('Étirer');
    expect(aspectRatioLabel('contain')).toBe('Normal');
  });

  it('computes bounded percentages', () => {
    expect(percentOf(30, 120)).toBe(25);
    expect(percentOf(500, 120)).toBe(100);
    expect(percentOf(-5, 120)).toBe(0);
    expect(percentOf(5, 0)).toBe(0);
  });
});

describe('chapters', () => {
  const chapters = [{ start: 600, title: 'B' }, { start: 0, title: 'A' }, { start: 1200, title: 'C' }];

  it('keeps valid chapters sorted', () => {
    const list = timelineChapters([...chapters, { start: 5000, title: 'late' }, { start: NaN, title: 'nan' }], 3000);
    expect(list.map(c => c.title)).toEqual(['A', 'B', 'C']);
    expect(timelineChapters(undefined, 100)).toEqual([]);
    expect(chapterAt(list, 700)?.title).toBe('B');
    expect(chapterAt(list, -1)).toBeUndefined();
  });

  it('computes next/previous chapter targets', () => {
    expect(chapterSkipTarget(undefined, 10, 3000, 1)).toBeNull();
    expect(chapterSkipTarget(chapters, 10, 3000, 1)).toBe(600);
    expect(chapterSkipTarget(chapters, 1300, 3000, 1)).toBe(3000);
    expect(chapterSkipTarget(chapters, 1300, 0, 1)).toBeNull();
    expect(chapterSkipTarget(chapters, 650, 3000, -1)).toBe(600);
    expect(chapterSkipTarget(chapters, 601, 3000, -1)).toBe(0);
    expect(chapterSkipTarget(chapters, 1202, 3000, -1)).toBe(600);
    expect(chapterSkipTarget([{ start: 10, title: 'x' }], 2, 100, -1)).toBe(0);
  });

  it('handles marker ranges', () => {
    expect(inRange({ start: 10, end: 20 }, 10)).toBe(true);
    expect(inRange({ start: 10, end: 20 }, 20)).toBe(false);
    expect(inRange(undefined, 5)).toBe(false);
    expect(markerSegmentStyle({ start: 10, end: 20 }, 100)).toEqual({ left: '10%', width: '10%' });
    expect(markerSegmentStyle({ start: 10, end: 10.1 }, 100).width).toBe('0.5%');
  });

  it('splits the bar into contiguous chapter segments', () => {
    expect(timelineSegments([], 0)).toEqual([]);
    expect(timelineSegments([], 100)).toEqual([{ start: 0, end: 100, title: null }]);
    const list = timelineChapters(chapters, 3000);
    expect(timelineSegments(list, 3000)).toEqual([
      { start: 0, end: 600, title: 'A' },
      { start: 600, end: 1200, title: 'B' },
      { start: 1200, end: 3000, title: 'C' }
    ]);
    // Premier chapitre tardif : tronçon d'ouverture sans titre ; débuts identiques fusionnés.
    expect(timelineSegments([{ start: 30, title: 'x' }, { start: 30, title: 'y' }, { start: 60, title: '' }], 90)).toEqual([
      { start: 0, end: 30, title: null },
      { start: 30, end: 60, title: 'y' },
      { start: 60, end: 90, title: null }
    ]);
    // Premier chapitre à 0,4 s : rattaché au début, pas de tronçon minuscule.
    expect(timelineSegments([{ start: 0.4, title: 'a' }], 10)).toEqual([{ start: 0, end: 10, title: 'a' }]);
  });

  it('locates, fills and overlays segments', () => {
    const segments = timelineSegments(timelineChapters(chapters, 3000), 3000);
    expect(segmentIndexAt(segments, 0)).toBe(0);
    expect(segmentIndexAt(segments, 600)).toBe(1);
    expect(segmentIndexAt(segments, 2999)).toBe(2);
    expect(segmentIndexAt([], 5)).toBe(-1);
    expect(segmentFill(segments[1], 300)).toBe(0);
    expect(segmentFill(segments[1], 900)).toBe(50);
    expect(segmentFill(segments[1], 2000)).toBe(100);
    expect(rangeInSegment({ start: 500, end: 900 }, segments[1])).toEqual({ left: 0, width: 50 });
    expect(rangeInSegment({ start: 500, end: 900 }, segments[0])).toEqual({ left: (500 / 600) * 100, width: (100 / 600) * 100 });
    expect(rangeInSegment({ start: 500, end: 900 }, segments[2])).toBeNull();
    expect(rangeInSegment(undefined, segments[0])).toBeNull();
  });
});

describe('menu-navigation', () => {
  it('wraps arrows and jumps with Home/End', () => {
    expect(nextMenuIndex('ArrowDown', 2, 3)).toBe(0);
    expect(nextMenuIndex('ArrowUp', 0, 3)).toBe(2);
    expect(nextMenuIndex('ArrowDown', -1, 3)).toBe(0);
    expect(nextMenuIndex('ArrowUp', -1, 3)).toBe(2);
    expect(nextMenuIndex('Home', 2, 3)).toBe(0);
    expect(nextMenuIndex('End', 0, 3)).toBe(2);
    expect(nextMenuIndex('a', 0, 3)).toBeNull();
  });
});
