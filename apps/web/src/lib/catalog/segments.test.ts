import { describe, expect, it } from 'vitest';
import {
  analysisState, analysisSteps, copySegment, effectiveSegment, formatClock,
  nearestChapter, parseClock, seasonSummary, segmentProblem, snapBound,
  suggestedSegment, type SegmentEpisode
} from './segments';

const episode = (patch: Partial<SegmentEpisode> = {}): SegmentEpisode => ({
  assetId: 'a', workId: 'w', episodeNumber: 1, title: 'Pilot', duration: 1400, minConfidence: 0.8, detectionEnabled: true,
  chapters: [], fingerprint: { status: 'completed', error: null, updatedAt: 1 }, detection: { status: 'completed', error: null, updatedAt: 1 },
  automatic: [], manual: [], ...patch
});

describe('segment clock', () => {
  it('reads seconds, minutes and hours', () => {
    expect(parseClock('83')).toBe(83);
    expect(parseClock('83,5')).toBe(83.5);
    expect(parseClock('1:23')).toBe(83);
    expect(parseClock(' 1:23.5 ')).toBe(83.5);
    expect(parseClock('1:02:03')).toBe(3723);
  });
  it('refuses malformed times', () => {
    for (const text of ['', '-3', '1:60', '1:61:00', 'abc', '1::2', '1:2:3:4']) expect(parseClock(text), text).toBeNull();
  });
  it('formats like the player', () => {
    expect(formatClock(83)).toBe('1:23');
    expect(formatClock(3723)).toBe('1:02:03');
    expect(formatClock(83.46, true)).toBe('1:23.5');
    expect(formatClock(83, true)).toBe('1:23');
    expect(formatClock(null)).toBe('—');
  });
});

describe('effective segment', () => {
  it('prefers a manual correction, even one that disables the segment', () => {
    const detected = { type: 'intro' as const, start: 30, end: 120, confidence: 0.95, extensionId: 'intro-skipper', updatedAt: 1, usable: true };
    expect(effectiveSegment(episode({ automatic: [detected] }), 'intro')).toMatchObject({ source: 'automatic', start: 30 });
    expect(effectiveSegment(episode({ automatic: [detected], manual: [{ type: 'intro', start: 31, end: 118, enabled: true, updatedAt: 2 }] }), 'intro')).toEqual({ source: 'manual', start: 31, end: 118 });
    expect(effectiveSegment(episode({ automatic: [detected], manual: [{ type: 'intro', start: null, end: null, enabled: false, updatedAt: 2 }] }), 'intro')).toEqual({ source: 'disabled' });
  });
  it('ignores detections below the library confidence floor', () => {
    const weak = { type: 'credits' as const, start: 1300, end: 1390, confidence: 0.7, extensionId: 'intro-skipper', updatedAt: 1, usable: false };
    expect(effectiveSegment(episode({ automatic: [weak] }), 'credits')).toEqual({ source: 'none' });
    expect(seasonSummary([episode({ automatic: [weak] })])).toMatchObject({ weak: 1, covered: { credits: 0 } });
  });
});

describe('segment validation and copies', () => {
  it('mirrors the server limits', () => {
    expect(segmentProblem(10, 5, 100)).toBe('segment_end_before_start');
    expect(segmentProblem(10, 120, 100)).toBe('segment_beyond_duration');
    expect(segmentProblem(0, 700, 1000)).toBe('segment_too_long');
    expect(segmentProblem(0, 700, 2000, 'credits')).toBeNull();
    expect(segmentProblem(0, 1300, 2000, 'credits')).toBe('segment_too_long_credits');
    expect(segmentProblem(null, 10, 100)).toBe('segment_invalid_time');
    expect(segmentProblem(30, 120, 1400)).toBeNull();
  });
  it('snaps bounds to nearby chapters and surfaces weak suggestions', () => {
    const chapters = [{ start: 30, title: 'Opening' }, { start: 120, title: 'Part A' }];
    expect(nearestChapter(chapters, 31.2, 3)?.title).toBe('Opening');
    expect(snapBound(chapters, 118.4, 3)).toBe(120);
    expect(snapBound(chapters, 110, 3)).toBe(110);
    const weak = { type: 'intro' as const, start: 30, end: 120, confidence: 0.72, extensionId: 'intro-skipper', updatedAt: 1, usable: false };
    expect(suggestedSegment(episode({ automatic: [weak] }), 'intro')).toEqual({ start: 30, end: 120 });
  });
  it('distinguishes film and episode analysis steps', () => {
    const film = episode({ chapterReading: { status: 'completed', error: null, updatedAt: 1 }, fingerprint: { status: 'missing', error: null, updatedAt: null }, detection: { status: 'missing', error: null, updatedAt: null } });
    expect(analysisState(film, 'film').label).toBe('segment_status_analysed');
    expect(analysisSteps(film, 'film')).toHaveLength(1);
    expect(analysisSteps(episode(), 'episode')).toHaveLength(3);
  });
  it('keeps credits anchored to the end of each file', () => {
    expect(copySegment('credits', 1300, 1390, 1400, 1450)).toEqual({ start: 1350, end: 1440 });
    expect(copySegment('intro', 30, 120, 1400, 1450)).toEqual({ start: 30, end: 120 });
    expect(copySegment('intro', 30, 120, 1400, 100)).toBeNull();
  });
  it('counts pending and failed analyses', () => {
    const summary = seasonSummary([
      episode({ fingerprint: { status: 'queued', error: null, updatedAt: 1 } }),
      episode({ fingerprint: { status: 'error', error: 'no chromaprint', updatedAt: 1 } }),
      episode()
    ]);
    expect(summary).toMatchObject({ total: 3, pending: 1, failed: 1 });
  });
});
