import {get} from 'svelte/store';
import {session} from '../auth/session';
export interface Playlist {id:string;name:string;visibility:'private'|'shared'|'public';owned:boolean}
export async function playlistRequest<T>(url:string,method='GET',body?:unknown):Promise<T>{
  const response=await fetch(url,{method,cache:'no-store',headers:{'content-type':'application/json','x-csrf-token':get(session)?.csrfToken??''},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(10000)});
  if(!response.ok)throw new Error(response.status===403?'Cette action n’est pas autorisée. Vérifiez les options de partage et vos droits.':response.status===404?'Playlist ou média indisponible.':'Impossible de modifier la playlist. Réessayez.');
  return (response.status===204?undefined:await response.json()) as T;
}
