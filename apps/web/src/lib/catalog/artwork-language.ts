import { tr } from '../i18n';
/** Selection categories overlap. A VO tag never claims historical release provenance. */
export function artworkCategories(language?: string | null, original?: string | null): string[] {
  const base = (tag?: string | null) => tag?.trim().toLowerCase().split('-')[0];
  const value = base(language);
  if (!value || ['und', 'xx', '00', 'zxx'].includes(value)) return [tr('no_language')];
  const categories = [];
  if (value === base(original)) categories.push('VO');
  if (value === 'fr') categories.push('VF');
  if (value === 'en') categories.push('EN');
  return categories.length ? categories : [tr('catalog_other_language')];
}
