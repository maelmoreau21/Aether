type Field = {key:string;label:string;description?:string;fieldType:string;defaultValue?:string;required?:boolean;secret?:boolean;options?:{label:string;value:string}[]};
type Manifest = {id:string;permissions:string[];configSchema?:Field[]};
export function settingsFields(manifest:Manifest):Field[] {
  return (manifest.configSchema??[]).filter(field=>!(manifest.permissions.includes('metadata.read')&&['language','default_language'].includes(field.key)) && !(manifest.permissions.includes('avatars')&&['force_random_on_creation','allow_gravatar','disallow_custom_avatars'].includes(field.key)));
}
export function settingsDefaults(manifest:Manifest):Record<string,string|boolean> {
  return Object.fromEntries(settingsFields(manifest).map(field=>[field.key,field.fieldType==='boolean'?field.defaultValue==='true':field.defaultValue??'']));
}
