import {describe,it,expect} from 'vitest';
import {pluginText} from './plugin-text';
describe('historical plugin text',()=>{it('repairs repeated console decoding without changing normal text',()=>{
 expect(pluginText('s├®lection')).toBe('sélection');
 expect(pluginText('M├â┬®tadonn├â┬®es')).toBe('Métadonnées');
 expect(pluginText('M├âãÆ├é┬®tadonn├âãÆ├é┬®es')).toBe('Métadonnées');
 expect(pluginText('Sélection · Français — café')).toBe('Sélection · Français — café');
});});
