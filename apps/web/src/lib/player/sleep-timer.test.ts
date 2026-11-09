import { describe, expect, it } from 'vitest';
import { SLEEP_OFF, sleepDue, sleepLabel, sleepRemaining, startSleep } from './sleep-timer';

describe('sleep timer', () => {
  it('counts down from the chosen delay and ignores unknown delays', () => {
    const timer = startSleep(30, 1000);
    expect(timer).toEqual({ kind: 'at', deadline: 1000 + 30 * 60000 });
    expect(sleepRemaining(timer, 1000 + 29 * 60000)).toBe(60);
    expect(sleepDue(timer, 1000 + 30 * 60000 - 1)).toBe(false);
    expect(sleepDue(timer, 1000 + 30 * 60000)).toBe(true);
    expect(startSleep(7, 0)).toEqual(SLEEP_OFF);
    expect(startSleep(null, 0)).toEqual(SLEEP_OFF);
  });
  it('waits for the end of the media without a countdown', () => {
    const timer = startSleep('end', 0);
    expect(sleepRemaining(timer, 99)).toBeNull();
    expect(sleepDue(timer, Number.MAX_SAFE_INTEGER)).toBe(false);
  });
  it('formats the remaining time', () => {
    expect(sleepLabel(65)).toBe('1:05');
    expect(sleepLabel(3725)).toBe('1:02:05');
  });
});
