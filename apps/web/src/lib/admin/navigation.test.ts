import { describe, expect, it } from 'vitest';
import { adminDestinations, adminSearchKeywords, adminSectionHashes, parseAdminHash, searchAdmin } from './navigation';

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

describe('administration grouping', () => {
  it('keeps every historical section and native statistics reachable', async () => {
    const { adminEntries, adminEntryFor } = await import('./navigation');
    expect(adminEntries).toHaveLength(12);
    for (const hash of ['#overview', '#statistics-section', '#general-section', '#network-section', '#libraries-section', '#artwork-section', '#trickplay-section', '#nodes-section', '#avatars-section', '#users-section', '#groups-section', '#api-keys-section', '#tasks-section', '#backups-section', '#extensions-section']) {
      expect(parseAdminHash(hash).section).toBe(hash);
      expect(adminEntryFor(hash)).toBeDefined();
    }
    expect(adminEntryFor('#trickplay-section')?.label).toBe('Métadonnées et images');
    expect(adminEntryFor('#network-section')?.label).toBe('Paramètres du serveur');
  });
});

describe('administration settings search', () => {
  it('returns nothing for an empty or blank query', () => {
    expect(searchAdmin('')).toEqual([]);
    expect(searchAdmin('   ')).toEqual([]);
  });
  it('matches without regard to accents or case', () => {
    const accented = searchAdmin('Qualité JPEG');
    expect(accented.map(result => result.hash)).toContain('#trickplay-section');
    expect(searchAdmin('QUALITE jpeg')).toEqual(accented);
    expect(searchAdmin('controle parental').map(result => [result.match, result.hash])).toContainEqual(['Contrôle parental', '#users-section']);
    expect(searchAdmin('jeton d\'appairage').map(result => result.hash)).toContain('#nodes-section');
  });
  it('reaches tab destinations and names the owning section', () => {
    const trickplay = searchAdmin('trickplay').find(result => result.match === 'Trickplay');
    expect(trickplay?.hash).toBe('#trickplay-section');
    expect(trickplay?.entry.label).toBe('Métadonnées et images');
    const network = searchAdmin('adresse publique');
    expect(network[0]).toMatchObject({ hash: '#network-section', match: 'Adresse publique' });
    expect(network[0].entry.label).toBe('Paramètres du serveur');
    expect(searchAdmin('avatars').some(result => result.hash === '#avatars-section')).toBe(true);
    expect(searchAdmin('paramètres du serveur')[0]?.hash).toBe('#general-section');
  });
  it('indexes every destination and only known destinations', () => {
    for (const destination of adminDestinations) expect(adminSearchKeywords[destination.hash]?.length).toBeGreaterThan(0);
    for (const hash of Object.keys(adminSearchKeywords)) expect(adminSectionHashes.has(hash)).toBe(true);
    expect(searchAdmin('zzz introuvable')).toEqual([]);
  });
});
