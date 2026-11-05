import { expect, test } from 'vitest';
import { corePortraitUrl, creditPeople, initials } from './credits';

const portrait = '/api/v1/works/w1/people/0123456789abcdef0123456789abcdef/image';

test('only Core-served portraits are loaded, never provider URLs', () => {
  expect(corePortraitUrl(portrait)).toBe(portrait);
  expect(corePortraitUrl('https://image.tmdb.org/t/p/w185/a.jpg')).toBe('');
  expect(corePortraitUrl('//evil.example/api/v1/works/w1/people/0123456789abcdef0123456789abcdef/image')).toBe('');
  expect(corePortraitUrl(null)).toBe('');
});

test('cast keeps portraits without assigning them to crew by name alone', () => {
  const people = creditPeople({
    actors: [
      { name: ' Greta Gerwig ', role: 'Herself', imageUrl: portrait, thumbUrl: 'https://image.tmdb.org/x.jpg' },
      { name: 'No Photo', thumbUrl: 'https://image.tmdb.org/y.jpg' },
      { name: '' }
    ],
    directors: ['Greta Gerwig', 'Someone Else'],
    writers: ['Greta Gerwig']
  });
  expect(people.map(p => [p.key, p.role, p.photoUrl])).toEqual([
    ['actor:greta gerwig', 'Herself', portrait],
    ['actor:no photo', 'Distribution', ''],
    ['director:greta gerwig', 'Réalisateur', ''],
    ['director:someone else', 'Réalisateur', ''],
    ['writer:greta gerwig', 'Scénario', '']
  ]);
});

test('initials fall back gracefully', () => {
  expect(initials('Jean-Paul Belmondo')).toBe('JB');
  expect(initials('Mary Elizabeth Winstead')).toBe('MW');
  expect(initials('Zendaya')).toBe('Z');
  expect(initials('élodie bouchez')).toBe('ÉB');
});

test('crew uses only Core portraits and falls back to initials for remote URLs', () => {
  const members = creditPeople({ directors: ['Director', 'Other'], crewCredits: [
    { name: 'Director', role: 'Director', imageUrl: portrait },
    { name: 'Other', role: 'Director', imageUrl: 'https://image.tmdb.org/a.jpg' }
  ] });
  expect(members.map(member => member.photoUrl)).toEqual([portrait, '']);
});

test('ambiguous same-name crew credits do not inherit a portrait', () => {
  const members = creditPeople({ directors: ['Same Name'], crewCredits: [
    { name: 'Same Name', role: 'Director', imageUrl: portrait },
    { name: 'Same Name', role: 'Director', imageUrl: portrait }
  ] });
  expect(members[0].photoUrl).toBe('');
});
