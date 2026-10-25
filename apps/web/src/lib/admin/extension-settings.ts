type Field = {key:string;label:string;description?:string;fieldType:string;defaultValue?:string;required?:boolean;secret?:boolean;options?:{label:string;value:string}[]};
type Manifest = {id:string;permissions:string[];configSchema?:Field[]};
// Presentation compatibility for already signed packages; never changes their manifest.
const endpoints: Record<string,string> = {
  tmdb:'https://api.themoviedb.org/3', imdb:'https://v3.sg.media-imdb.com/suggestion',
  omdb:'https://www.omdbapi.com', discogs:'https://api.discogs.com',
  musicbrainz:'https://musicbrainz.org/ws/2', lrclib:'https://lrclib.net/api',
  'fanart-tv':'https://webservice.fanart.tv/v3'
};
export function settingsFields(manifest:Manifest):Field[] {
  if(manifest.permissions.includes('avatars')) return [
    {key:'force_random_on_creation',label:'Avatar aléatoire pour les nouveaux utilisateurs',description:'Attribuer une image de la galerie aux comptes sans avatar.',fieldType:'boolean',defaultValue:'true'},
    {key:'allow_gravatar',label:'Autoriser l’import depuis Gravatar',description:'Les utilisateurs peuvent importer leur Gravatar depuis leur profil.',fieldType:'boolean',defaultValue:'false'},
    {key:'disallow_custom_avatars',label:'Verrouiller les imports utilisateurs',description:'Réserver les photos de profil aux avatars de la galerie.',fieldType:'boolean',defaultValue:'false'}
  ];
  let fields=(manifest.configSchema??[]).filter(field=>!(manifest.permissions.includes('metadata.read')&&['language','default_language'].includes(field.key)));
  if(manifest.id==='fanart-tv') return fields.filter(field=>field.key==='api_key').map(field=>({...field,label:'API Key',description:'Clé API disponible dans votre compte fanart.tv.'}));
  fields=fields.map(field=>field.key==='base_url'?{...field,label:'Adresse du fournisseur',description:'L’adresse officielle est renseignée par défaut.',defaultValue:field.defaultValue||endpoints[manifest.id]}:field);
  if(endpoints[manifest.id]&&!fields.some(field=>field.key==='base_url')) fields.push({key:'base_url',label:'Adresse du fournisseur',fieldType:'text',defaultValue:endpoints[manifest.id]});
  return fields;
}
export function settingsDefaults(manifest:Manifest):Record<string,string|boolean> {
  const defaults=Object.fromEntries(settingsFields(manifest).map(field=>[field.key,field.fieldType==='boolean'?field.defaultValue==='true':field.defaultValue??'']));
  if(manifest.id==='fanart-tv')defaults.base_url=endpoints[manifest.id];
  return defaults;
}
