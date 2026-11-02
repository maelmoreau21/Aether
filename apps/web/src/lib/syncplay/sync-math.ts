import type { PartyState } from './types';

/**
 * Calculates clock offset relative to server using NTP algorithm.
 * @param clientSendMs Client timestamp when ping was sent (t0)
 * @param serverEpochMs Server timestamp recorded by server (t1)
 * @param clientRecvMs Client timestamp when pong was received (t2)
 * @returns Estimated offset such that serverTime = clientTime + offset
 */
export function calculateClockOffset(
  clientSendMs: number,
  serverEpochMs: number,
  clientRecvMs: number
): { offsetMs: number; roundTripMs: number } {
  const roundTripMs = Math.max(0, clientRecvMs - clientSendMs);
  const offsetMs = serverEpochMs - (clientSendMs + roundTripMs / 2);
  return { offsetMs, roundTripMs };
}

/**
 * Computes live expected playback position based on server clock and state.
 */
export function calculateTargetPosition(
  state: PartyState,
  basePosition: number,
  positionUpdatedServerMs: number,
  currentServerEpochMs: number
): number {
  if (state === 'playing') {
    const elapsedSeconds = Math.max(
      0,
      (currentServerEpochMs - positionUpdatedServerMs) / 1000
    );
    return Math.max(0, basePosition + elapsedSeconds);
  }
  return Math.max(0, basePosition);
}

export interface DriftCorrection {
  action: 'none' | 'rate' | 'seek';
  playbackRate: number;
  drift: number;
  targetPosition: number;
}

export interface DriftOptions {
  toleranceSeconds?: number; // threshold under which playback is considered in sync (default: 0.25s)
  rateThresholdSeconds?: number; // max drift threshold for smooth playbackRate adjustment (default: 1.5s)
  rateAdjustmentSpeed?: number; // speedup/slowdown factor (default: 0.05 => 1.05x or 0.95x)
}

/**
 * Evaluates whether current video playback is drifting and determines smooth or seek correction.
 * Avoids disruptive seek jumps for small discrepancies.
 */
export function evaluateDriftCorrection(
  currentLocalPosition: number,
  targetPosition: number,
  options?: DriftOptions
): DriftCorrection {
  const tolerance = options?.toleranceSeconds ?? 0.25;
  const maxRateThreshold = options?.rateThresholdSeconds ?? 1.5;
  const rateAdjustment = options?.rateAdjustmentSpeed ?? 0.05;

  const drift = currentLocalPosition - targetPosition;
  const absDrift = Math.abs(drift);

  if (absDrift < tolerance) {
    return {
      action: 'none',
      playbackRate: 1.0,
      drift,
      targetPosition
    };
  }

  if (absDrift <= maxRateThreshold) {
    // Smooth correction: adjust playbackRate slightly without jumping
    const playbackRate = drift < 0 ? 1.0 + rateAdjustment : 1.0 - rateAdjustment;
    return {
      action: 'rate',
      playbackRate,
      drift,
      targetPosition
    };
  }

  // Large difference: discrete seek jump required
  return {
    action: 'seek',
    playbackRate: 1.0,
    drift,
    targetPosition
  };
}
