import {describe,it,expect} from 'vitest';
import {settingsFields,settingsDefaults} from './extension-settings';
describe('extension configuration',()=>{
 it('uses the signed schema independently of provider identity',()=>{const manifest={id:'any-provider',permissions:['metadata.read'],configSchema:['api_key','user_key','base_url','language'].map(key=>({key,label:key,fieldType:'text'}))};expect(settingsFields(manifest).map(field=>field.key)).toEqual(['api_key','user_key','base_url']);expect(manifest.configSchema).toHaveLength(4);expect(settingsDefaults(manifest).base_url).toBe('');});
 it('does not invent an endpoint for a package that did not declare one',()=>{const manifest={id:'any-provider',permissions:['metadata.read'],configSchema:[{key:'language',label:'Langue',fieldType:'select',defaultValue:'en-US'}]};expect(settingsFields(manifest)).toEqual([]);expect(settingsDefaults(manifest)).toEqual({});});
 it('keeps declared endpoints for third-party providers',()=>{const manifest={id:'third-party',permissions:['metadata.read'],configSchema:[{key:'base_url',label:'URL',fieldType:'text',defaultValue:'https://example.org/provider'}]};expect(settingsDefaults(manifest).base_url).toBe('https://example.org/provider');});
 it('keeps avatar policy outside extension settings',()=>{expect(settingsFields({id:'avatars',permissions:['avatars'],configSchema:[{key:'allow_gravatar',label:'Gravatar',fieldType:'boolean'},{key:'custom',label:'Custom',fieldType:'text'}]}).map(field=>field.key)).toEqual(['custom']);});
});
