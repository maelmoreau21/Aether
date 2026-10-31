import { describe, expect, it } from 'vitest';
import {
  calculateClockOffset,
  calculateTargetPosition,
  evaluateDriftCorrection
} from './sync-math';

describe('SyncPlay pure math and drift algorithms', () => {
  it('calculates clock offset accurately using symmetric RTT', () => {
    // Client sent ping at 1000, server saw 1050, client got pong at 1100
    // RTT = 100ms, one-way delay = 50ms
    // Server time at send was ~1050 - 50 = 1000 => offset = 0
    const res = calculateClockOffset(1000, 1050, 1100);
    expect(res.roundTripMs).toBe(100);
    expect(res.offsetMs).toBe(0);

    // When client clock is 200ms behind server
    // Client t0=1000, Server t1=1250, Client t2=1100
    const resAhead = calculateClockOffset(1000, 1250, 1100);
    expect(resAhead.offsetMs).toBe(200);
  });

  it('calculates expected target position based on state and elapsed time', () => {
    // Paused: position stays basePosition
    expect(calculateTargetPosition('paused', 42.5, 10000, 20000)).toBe(42.5);
    expect(calculateTargetPosition('waiting', 0, 10000, 20000)).toBe(0);

    // Playing: 5 seconds elapsed (5000ms)
    const playingPos = calculateTargetPosition('playing', 10.0, 10000, 15000);
    expect(playingPos).toBeCloseTo(15.0, 2);
  });

  it('evaluates drift correction actions', () => {
    // Small drift (< 0.25s) => no action
    const tinyDrift = evaluateDriftCorrection(10.1, 10.0);
    expect(tinyDrift.action).toBe('none');
    expect(tinyDrift.playbackRate).toBe(1.0);

    // Moderate drift behind (-0.6s) => smooth speedup (1.05x)
    const behindDrift = evaluateDriftCorrection(9.4, 10.0);
    expect(behindDrift.action).toBe('rate');
    expect(behindDrift.playbackRate).toBe(1.05);

    // Moderate drift ahead (+0.8s) => smooth slowdown (0.95x)
    const aheadDrift = evaluateDriftCorrection(10.8, 10.0);
    expect(aheadDrift.action).toBe('rate');
    expect(aheadDrift.playbackRate).toBe(0.95);

    // Large drift (> 1.5s) => seek jump
    const largeDrift = evaluateDriftCorrection(12.0, 10.0);
    expect(largeDrift.action).toBe('seek');
    expect(largeDrift.targetPosition).toBe(10.0);
    expect(largeDrift.playbackRate).toBe(1.0);
  });
});
