type Field = {key:string;label:string;description?:string;fieldType:string;defaultValue?:string;required?:boolean;secret?:boolean;options?:{label:string;value:string}[]};
type Manifest = {id:string;permissions:string[];configSchema?:Field[]};
export function settingsFields(manifest:Manifest):Field[] {
  if(manifest.permissions.includes('avatars')) return [
    {key:'force_random_on_creation',label:'Avatar aléatoire pour les nouveaux utilisateurs',description:'Attribuer une image de la galerie aux comptes sans avatar.',fieldType:'boolean',defaultValue:'true'},
    {key:'allow_gravatar',label:'Autoriser l’import depuis Gravatar',description:'Les utilisateurs peuvent importer leur Gravatar depuis leur profil.',fieldType:'boolean',defaultValue:'false'},
    {key:'disallow_custom_avatars',label:'Verrouiller les imports utilisateurs',description:'Réserver les photos de profil aux avatars de la galerie.',fieldType:'boolean',defaultValue:'false'}
  ];
  return (manifest.configSchema??[]).filter(field=>!(manifest.permissions.includes('metadata.read')&&['language','default_language'].includes(field.key)));
}
export function settingsDefaults(manifest:Manifest):Record<string,string|boolean> {
  return Object.fromEntries(settingsFields(manifest).map(field=>[field.key,field.fieldType==='boolean'?field.defaultValue==='true':field.defaultValue??'']));
}
