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
      plugin_storage_failed: 'Le serveur ne peut pas enregistrer cette extension. Consultez les journaux du serveur pour connaître la cause exacte.',
      plugin_storage_permission_denied: 'Le serveur ne peut pas accéder au stockage des extensions. Vérifiez les droits du dossier configuré par MEDIA_PLUGINS_PATH.',
      plugin_storage_conflict: 'Une autre version du paquet existe déjà sur disque. Désinstallez cette extension avant de la réinstaller. Si elle est absente des Installées, consultez les journaux du serveur.',
      plugin_storage_integrity_failed: 'Un fichier du paquet installé est altéré ou invalide. Désinstallez puis réinstallez cette extension.',
      plugin_storage_missing: 'Un fichier de l’extension manque sur le serveur. Désinstallez puis réinstallez cette extension.',
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
      invalid_output: 'La réponse de l’extension ne respecte pas le format attendu.',
      incompatible_server_version: 'Cette extension n’est pas compatible avec cette version du serveur.',
      invalid_package_digest: 'L’empreinte du paquet ne correspond pas au catalogue.',
      invalid_package: 'Le paquet de l’extension est corrompu ou invalide.',
      catalogue_package_mismatch: 'Le paquet téléchargé ne correspond pas au catalogue.',
      invalid_public_key: 'La clé publique de l’éditeur est invalide.',
      invalid_signature: 'La signature cryptographique de l’extension est invalide.',
      invalid_manifest: 'Le manifeste de l’extension est invalide.',
      module_too_large: 'Le module WebAssembly dépasse la taille maximale autorisée (256 Ko).',
      invalid_module: 'Le module WebAssembly de l’extension est invalide.',
      invalid_digest: 'L’empreinte SHA-256 du module est invalide.',
      incompatible_api: 'L’extension utilise une version d’API incompatible.',
      repository_https_public_host_required: 'L’adresse du dépôt doit être une URL HTTPS publique valide.',
      repository_address_forbidden: 'L’adresse IP du dépôt est interdite ou privée.',
      repository_unavailable: 'Le dépôt de plugins est actuellement injoignable.',
      repository_http_error: 'Le serveur du dépôt a renvoyé une erreur de téléchargement.',
      repository_response_too_large: 'La réponse du dépôt dépasse la taille autorisée.',
      repository_limit: 'La limite de dépôts enregistrés est atteinte.',
      repository_exists: 'Ce dépôt est déjà enregistré.',
      unknown_repository: 'Dépôt introuvable.',
      unknown_catalogue_version: 'Cette version de l’extension n’existe pas dans le catalogue.',
      repository_changed: 'Le catalogue a changé. Veuillez réactualiser les dépôts.',
      extension_exists: 'Cette extension est déjà installée.',
      extension_limit: 'La limite de 32 extensions installées est atteinte.',
      extension_unavailable: 'Cette extension n’est pas disponible.'
    };
    const code = detail?.pluginError ?? detail?.error;
    if (code && messages[code]) throw new Error(messages[code]);
    if (detail?.pluginError) throw new Error(`Extension : ${detail.pluginError}`);
    if (detail?.error) throw new Error(`Erreur : ${detail.error}`);
    throw new Error(response.status === 401 ? 'Session expirée. Reconnectez-vous.' : response.status === 403 ? 'Accès refusé.' : response.status === 409 ? 'Ce nom existe déjà ou la limite du serveur est atteinte.' : response.status === 400 ? 'Requête invalide. Vérifiez les paramètres saisis.' : 'Opération impossible. Réessayez.');
  }
  if (init.method && init.method !== 'GET') {
    const library = path.match(/\/libraries\/([^/]+)/)?.[1];
    invalidateCatalog(library ? decodeURIComponent(library) : undefined);
  }
  return response.status === 204 ? undefined as T : response.json() as Promise<T>;
}

export interface UserGroup { id: string; name: string; isDefault?: boolean; userIds: string[]; libraryIds: string[] }
