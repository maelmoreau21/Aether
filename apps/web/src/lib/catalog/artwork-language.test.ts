import { expect, test } from 'vitest';
import { artworkCategories } from './artwork-language';
test('categories overlap without inventing original-language metadata', () => {
  expect(artworkCategories('en-US', 'en')).toEqual(['VO', 'EN']);
  expect(artworkCategories('fr-CA', 'ja')).toEqual(['VF']);
  expect(artworkCategories('ja', 'ja')).toEqual(['VO']);
  expect(artworkCategories('en-GB', 'ja')).toEqual(['EN']);
  expect(artworkCategories('ja', null)).toEqual(['Autre langue']);
  expect(artworkCategories(null, 'en')).toEqual(['Sans langue']);
});
