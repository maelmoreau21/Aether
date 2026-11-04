import { expect, test } from 'vitest';
import { corePortraitUrl, creditPeople, initials } from './credits';

const portrait = '/api/v1/works/w1/people/0123456789abcdef0123456789abcdef/image';

test('only Core-served portraits are loaded, never provider URLs', () => {
  expect(corePortraitUrl(portrait)).toBe(portrait);
  expect(corePortraitUrl('https://image.tmdb.org/t/p/w185/a.jpg')).toBe('');
  expect(corePortraitUrl('//evil.example/api/v1/works/w1/people/0123456789abcdef0123456789abcdef/image')).toBe('');
  expect(corePortraitUrl(null)).toBe('');
});

test('cast keeps portraits, crew reuses the cast portrait of the same person', () => {
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
    ['director:greta gerwig', 'Réalisateur', portrait],
    ['director:someone else', 'Réalisateur', ''],
    ['writer:greta gerwig', 'Scénario', portrait]
  ]);
});

test('initials fall back gracefully', () => {
  expect(initials('Jean-Paul Belmondo')).toBe('JB');
  expect(initials('Mary Elizabeth Winstead')).toBe('MW');
  expect(initials('Zendaya')).toBe('Z');
  expect(initials('élodie bouchez')).toBe('ÉB');
});
