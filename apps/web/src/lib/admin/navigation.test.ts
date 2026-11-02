import { describe, expect, it } from 'vitest';
import { parseAdminHash } from './navigation';

describe('administration task navigation', () => {
  it('opens task details and preserves encoded task identifiers', () => {
    expect(parseAdminHash('#tasks-section?task=scan-films%3Fspecial')).toEqual({ section: '#tasks-section', task: 'scan-films?special' });
    expect(parseAdminHash('#tasks-section')).toEqual({ section: '#tasks-section', task: null });
    expect(parseAdminHash('#trickplay-section')).toEqual({ section: '#trickplay-section', task: null });
  });
  it('never carries a task query into another or unknown section', () => {
    expect(parseAdminHash('#backups-section?task=backup')).toEqual({ section: '#backups-section', task: null });
    expect(parseAdminHash('#bogus?task=backup')).toEqual({ section: '#overview', task: null });
    expect(parseAdminHash('#tasks-section?task=')).toEqual({ section: '#tasks-section', task: null });
  });
});
