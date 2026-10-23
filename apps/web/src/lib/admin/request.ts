import { invalidateCatalog } from '../catalog/page-cache';
import { get } from 'svelte/store';
import { session } from '../auth/session';

export async function adminRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const current = get(session);
  if (current?.user.role !== 'administrator') throw new Error('Accès administrateur requis.');
  const response = await fetch(path, {
    ...init, credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(path.includes('/extensions/') ? 70000 : 30000),
    headers: { 'content-type': 'application/json', 'x-csrf-token': current.csrfToken, ...init.headers }
  });
  if (!response.ok) {
    const detail = await response.json().catch(() => null) as { error?: string; pluginError?: string } | null;
    const messages: Record<string,string> = {
      missing_api_key: 'La clé API du fournisseur est absente. Configurez cette extension.',
      external_id_required: 'Ce fournisseur exige un identifiant externe. Complétez les métadonnées de l’œuvre ou appliquez d’abord celles d’un fournisseur de recherche.',
      unsupported_operation: 'Ce paquet ne prend pas en charge cette opération. Vérifiez sa version.',
      ambiguous_match: 'Plusieurs résultats correspondent. Choisissez le bon résultat dans la recherche.',
      extension_busy: 'Une extension est déjà en cours d’exécution. Réessayez après sa fin.',
      extension_timeout: 'Le délai maximal de l’extension est dépassé.',
      rate_limited: 'Le fournisseur limite les requêtes. Réessayez plus tard.',
      image_host_denied: 'Cet hôte d’images ne figure pas dans les domaines approuvés du paquet.',
      invalid_output: 'La réponse de l’extension ne respecte pas le format attendu.'
    };
    const code = detail?.pluginError ?? detail?.error;
    if (code && messages[code]) throw new Error(messages[code]);
    if (detail?.pluginError) throw new Error(`Extension : ${detail.pluginError}`);
    throw new Error(response.status === 401 ? 'Session expirée. Reconnectez-vous.' : response.status === 403 ? 'Accès refusé.' : response.status === 409 ? 'Ce nom existe déjà ou la limite du serveur est atteinte.' : response.status === 400 ? 'Vérifiez les champs et les droits sur les bibliothèques sélectionnées.' : 'Opération impossible. Réessayez.');
  }
  if (init.method && init.method !== 'GET') {
    const library = path.match(/\/libraries\/([^/]+)/)?.[1];
    invalidateCatalog(library ? decodeURIComponent(library) : undefined);
  }
  return response.status === 204 ? undefined as T : response.json() as Promise<T>;
}

export interface UserGroup { id: string; name: string; isDefault?: boolean; userIds: string[]; libraryIds: string[] }
