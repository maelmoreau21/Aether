import { describe, expect, it } from 'vitest';
import { adminDestinations, adminSearchKeywords, adminSectionHashes, parseAdminHash, searchAdmin } from './navigation';

describe('administration task navigation', () => {
  it('opens task details and preserves encoded task identifiers', () => {
    expect(parseAdminHash('#tasks-section?task=scan-films%3Fspecial')).toEqual({ section: '#tasks-section', task: 'scan-films?special' });
    expect(parseAdminHash('#tasks-section')).toEqual({ section: '#tasks-section', task: null });
  });
  it('never carries a task query into another or unknown section', () => {
    expect(parseAdminHash('#backups-section?task=backup')).toEqual({ section: '#backups-section', task: null });
    expect(parseAdminHash('#bogus?task=backup')).toEqual({ section: '#overview', task: null });
    expect(parseAdminHash('#tasks-section?task=')).toEqual({ section: '#tasks-section', task: null });
  });
  it('redirects historical global Trickplay links to library settings', () => {
    expect(parseAdminHash('#trickplay-section')).toEqual({ section: '#libraries-section', task: null });
    expect(parseAdminHash('#trickplay-section?task=thumbnail')).toEqual({ section: '#libraries-section', task: null });
    expect(adminSectionHashes.has('#trickplay-section')).toBe(false);
  });
});

describe('administration grouping', () => {
  it('keeps every historical section and native statistics reachable', async () => {
    const { adminEntries, adminEntryFor } = await import('./navigation');
    expect(adminEntries).toHaveLength(14);
    for (const hash of ['#overview', '#statistics-section', '#general-section', '#network-section', '#libraries-section', '#artwork-section', '#nodes-section', '#avatars-section', '#users-section', '#groups-section', '#api-keys-section', '#tasks-section', '#backups-section', '#extensions-section']) {
      expect(parseAdminHash(hash).section).toBe(hash);
      expect(adminEntryFor(hash)).toBeDefined();
    }
    expect(adminDestinations.some(destination => destination.hash === '#trickplay-section')).toBe(false);
    expect(adminEntryFor('#network-section')?.label).toBe('Paramètres du serveur');
  });
  it('keeps operations tracking and automation reachable beside scheduled tasks', async () => {
    const { adminEntryFor } = await import('./navigation');
    for (const hash of ['#operations-section', '#automation-section']) {
      expect(parseAdminHash(hash).section).toBe(hash);
      expect(adminEntryFor(hash)?.hash).toBe('#tasks-section');
    }
    expect(parseAdminHash('#operations-section?task=backup').task).toBeNull();
  });
  it('groups API keys under System and application shortcuts under Extensions', async () => {
    const { adminNavigation, adminEntryFor } = await import('./navigation');
    expect(adminNavigation.find(group => group.label === 'Système')?.links.some(link => link.hash === '#api-keys-section')).toBe(true);
    expect(adminNavigation.find(group => group.label === 'Extensions')?.links.map(link => link.hash)).toEqual(['#extensions-section', '#requests-section', '#reports-section']);
    expect(adminEntryFor('#acquisition-section')?.extensionId).toBe('media-requests');
    expect(parseAdminHash('#reports-section').section).toBe('#reports-section');
  });
});

describe('administration settings search', () => {
  it('keeps metadata language configuration in the library', () => {
    expect(searchAdmin('Langue préférée').map(result => result.hash)).toEqual(['#libraries-section']);
    expect(searchAdmin('Pays des classifications').map(result => result.hash)).toEqual(['#libraries-section']);
  });
  it('returns nothing for an empty or blank query', () => {
    expect(searchAdmin('')).toEqual([]);
    expect(searchAdmin('   ')).toEqual([]);
  });
  it('matches without regard to accents or case', () => {
    const accented = searchAdmin('Qualité JPEG');
    expect(accented.map(result => result.hash)).toContain('#libraries-section');
    expect(searchAdmin('QUALITE jpeg')).toEqual(accented);
    expect(searchAdmin('controle parental').map(result => [result.match, result.hash])).toContainEqual(['Contrôle parental', '#users-section']);
    expect(searchAdmin('jeton d\'appairage').map(result => result.hash)).toContain('#nodes-section');
  });
  it('reaches tab destinations and names the owning section', () => {
    const trickplay = searchAdmin('trickplay');
    expect(trickplay).toHaveLength(1);
    expect(trickplay[0]).toMatchObject({ hash: '#libraries-section', match: 'Trickplay' });
    expect(trickplay[0].entry.label).toBe('Bibliothèques');
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
