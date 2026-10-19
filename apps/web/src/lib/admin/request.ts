import { get } from 'svelte/store';
import { session } from '../auth/session';

export async function adminRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const current = get(session);
  if (current?.user.role !== 'administrator') throw new Error('Accès administrateur requis.');
  const response = await fetch(path, {
    ...init, credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(30000),
    headers: { 'content-type': 'application/json', 'x-csrf-token': current.csrfToken, ...init.headers }
  });
  if (!response.ok) throw new Error(response.status === 401 ? 'Session expirée. Reconnectez-vous.' : response.status === 403 ? 'Accès refusé.' : response.status === 409 ? 'Ce nom existe déjà ou la limite du serveur est atteinte.' : response.status === 400 ? 'Vérifiez les champs et les droits sur les bibliothèques sélectionnées.' : 'Opération impossible. Réessayez.');
  return response.status === 204 ? undefined as T : response.json() as Promise<T>;
}

export interface UserGroup { id: string; name: string; userIds: string[]; libraryIds: string[] }
