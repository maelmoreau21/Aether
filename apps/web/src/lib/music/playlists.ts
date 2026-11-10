import {get} from 'svelte/store';
import {session} from '../auth/session';
import { tr } from '../i18n';
export interface Playlist {id:string;name:string;visibility:'private'|'shared'|'public'|'collection';owned:boolean;automatic?:boolean;favorites?:boolean;systemKey?:string}
export async function playlistRequest<T>(url:string,method='GET',body?:unknown):Promise<T>{
  const response=await fetch(url,{method,cache:'no-store',headers:{'content-type':'application/json','x-csrf-token':get(session)?.csrfToken??''},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(10000)});
  if(!response.ok)throw new Error(response.status===403?tr('this_action_is_not_allowed_check'):response.status===404?tr('playlist_or_media_unavailable'):tr('unable_to_modify_the_playlist_try'));
  return (response.status===204?undefined:await response.json()) as T;
}
