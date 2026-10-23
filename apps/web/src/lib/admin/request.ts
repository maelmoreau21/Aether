import { invalidateCatalog } from '../catalog/page-cache';
import { get } from 'svelte/store';
import { session } from '../auth/session';

export async function adminRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const current = get(session);
  if (current?.user.role !== 'administrator') throw new Error('Accès administrateur requis.');
  const response = await fetch(path, {
    ...init, credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(path.includes('/avatar-packs') ? 310000 : path.includes('/extensions/') ? 70000 : 30000),
    headers: { ...(init.body instanceof FormData ? {} : {'content-type':'application/json'}), 'x-csrf-token': current.csrfToken, ...init.headers }
  });
  if (!response.ok) {
    const detail = await response.json().catch(() => null) as { error?: string; pluginError?: string } | null;
    const messages: Record<string,string> = {
      plugin_storage_failed: 'Le dossier data/plugins est indisponible ou un fichier installé est altéré. Vérifiez les droits du volume.',
      invalid_avatar_upload: 'Choisissez un nom et des images PNG, JPEG ou WebP pour cette collection.',
      github_avatar_repository_required: 'Collez le lien racine d’un dépôt GitHub public, sans /tree/ ni paramètre.',
      avatar_import_busy: 'Une collection est déjà en cours d’import.',
      avatar_import_timeout: 'L’import a dépassé cinq minutes. Réessayez avec une plus petite collection.',
      avatar_pack_size_limit: 'Choisissez un dossier contenant de 1 à 500 images, de 2 Mo maximum chacune.',
      avatar_source_too_large: 'Ce dépôt est trop volumineux pour être parcouru.',
      avatar_pack_limit: 'Le serveur contient déjà 32 collections.',
      invalid_avatar_image: 'Une image de cette collection est invalide. Aucun avatar n’a été importé.',
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
