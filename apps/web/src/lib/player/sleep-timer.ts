/** Sleep timer: a delay in minutes, or the end of the current episode or film. */
export type SleepChoice = number | 'end';
export const SLEEP_MINUTES: readonly number[] = [15, 30, 45, 60, 90, 120];

export type SleepTimer = { kind: 'off' } | { kind: 'at'; deadline: number } | { kind: 'end' };
export const SLEEP_OFF: SleepTimer = { kind: 'off' };

export function startSleep(choice: SleepChoice | null, now: number): SleepTimer {
  if (choice === null) return SLEEP_OFF;
  if (choice === 'end') return { kind: 'end' };
  return SLEEP_MINUTES.includes(choice) ? { kind: 'at', deadline: now + choice * 60000 } : SLEEP_OFF;
}

/** Seconds left, `null` when the timer waits for the end of the media or is off. */
export function sleepRemaining(timer: SleepTimer, now: number): number | null {
  return timer.kind === 'at' ? Math.max(0, Math.ceil((timer.deadline - now) / 1000)) : null;
}

export function sleepDue(timer: SleepTimer, now: number): boolean {
  return timer.kind === 'at' && now >= timer.deadline;
}

/** "12:05" or "1:02:05" for the menu badge. */
export function sleepLabel(seconds: number): string {
  const whole = Math.max(0, Math.round(seconds));
  const hours = Math.floor(whole / 3600), minutes = Math.floor(whole % 3600 / 60), rest = String(whole % 60).padStart(2, '0');
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${rest}` : `${minutes}:${rest}`;
}
