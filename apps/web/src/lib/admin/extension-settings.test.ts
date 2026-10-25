import {describe,it,expect} from 'vitest';
import {settingsFields,settingsDefaults} from './extension-settings';
describe('extension configuration',()=>{
 it('keeps fanart limited to its API key without mutating the signed schema',()=>{const manifest={id:'fanart-tv',permissions:['metadata.read'],configSchema:['api_key','user_key','base_url','language'].map(key=>({key,label:key,fieldType:'text'}))};expect(settingsFields(manifest).map(field=>field.key)).toEqual(['api_key']);expect(manifest.configSchema).toHaveLength(4);expect(settingsDefaults(manifest).base_url).toBe('https://webservice.fanart.tv/v3');});
 it('uses the library language and fills an official endpoint on legacy packages',()=>{const manifest={id:'tmdb',permissions:['metadata.read'],configSchema:[{key:'language',label:'Langue',fieldType:'select',defaultValue:'en-US'}]};expect(settingsFields(manifest).map(field=>field.key)).toEqual(['base_url']);expect(settingsDefaults(manifest).base_url).toBe('https://api.themoviedb.org/3');});
 it('keeps declared endpoints for third-party providers',()=>{const manifest={id:'third-party',permissions:['metadata.read'],configSchema:[{key:'base_url',label:'URL',fieldType:'text',defaultValue:'https://example.org/provider'}]};expect(settingsDefaults(manifest).base_url).toBe('https://example.org/provider');});
 it('gives avatar managers exactly the three relevant settings',()=>{expect(settingsFields({id:'avatars',permissions:['avatars']}).map(field=>field.key)).toEqual(['force_random_on_creation','allow_gravatar','disallow_custom_avatars']);});
});
