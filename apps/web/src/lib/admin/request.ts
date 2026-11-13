import { invalidateCatalog } from '../catalog/page-cache';
import { get } from 'svelte/store';
import { session } from '../auth/session';

import { tr } from '../i18n';
export async function adminRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const current = get(session);
  if (current?.user.role !== 'administrator') throw new Error(tr('administrator_access_required'));
  const response = await fetch(path, {
    ...init, credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(path.includes('/avatar-packs') ? 310000 : path.includes('/extensions/') ? 70000 : 30000),
    headers: { ...(init.body instanceof FormData ? {} : {'content-type':'application/json'}), 'x-csrf-token': current.csrfToken, ...init.headers }
  });
  if (!response.ok) {
    const detail = await response.json().catch(() => null) as { error?: string; pluginError?: string; code?: string; message?: string } | null;
    if (path.startsWith('/api/v1/admin/backups') && detail?.code?.startsWith('backup_') && typeof detail.message === 'string') {
      throw new Error(detail.message);
    }
    const messages: Record<string,string> = {
      avatar_collection_full: tr('this_collection_cannot_contain_more_than'),
      avatar_migration_required: tr('first_transfer_the_former_collections_under'),
      avatar_migration_id_conflict: tr('the_transfer_is_not_possible_collection'),
      plugin_storage_failed: tr('the_server_cannot_save_this_extension'),
      plugin_storage_permission_denied: tr('the_server_cannot_access_extension_storage'),
      plugin_storage_conflict: tr('another_version_of_the_package_already'),
      plugin_storage_integrity_failed: tr('a_file_of_the_installed_package'),
      plugin_storage_missing: tr('a_file_of_the_extension_is'),
      invalid_avatar_upload: tr('choose_a_name_and_png_jpeg'),
      github_avatar_repository_required: tr('paste_the_root_link_of_a'),
      avatar_import_busy: tr('a_collection_is_already_being_imported'),
      avatar_import_timeout: tr('the_import_exceeded_five_minutes_try'),
      avatar_pack_size_limit: tr('choose_a_folder_containing_1_to'),
      avatar_source_too_large: tr('this_repository_is_too_large_to'),
      avatar_pack_limit: tr('the_server_already_contains_32_collections'),
      invalid_avatar_image: tr('an_image_in_this_collection_is'),
      missing_api_key: tr('the_provider_s_api_key_is'),
      external_id_required: tr('this_provider_requires_an_external'),
      unsupported_operation: tr('this_package_does_not_support_this'),
      ambiguous_match: tr('several_results_match_choose_the_right'),
      extension_busy: tr('an_extension_is_already_running_try'),
      extension_timeout: tr('the_extension_s_maximum_time_was'),
      metadata_unidentified: tr('media_not_identified_automatically_it_will'),
      not_found: tr('this_provider_does_not_know_this'),
      parse_error: tr('the_provider_s_response_is_unreadable'),
      invalid_imdb_id: tr('no_imdb_identifier_is_known_for'),
      rate_limited: tr('the_provider_is_rate_limiting_requests'),
      image_host_denied: tr('this_image_host_is_not_among'),
      invalid_output: tr('the_extension_s_response_does_not'),
      incompatible_server_version: tr('this_extension_is_not_compatible_with'),
      invalid_package_digest: tr('the_package_fingerprint_does_not_match'),
      invalid_package: tr('the_extension_package_is_corrupted_or'),
      catalogue_package_mismatch: tr('the_downloaded_package_does_not_match'),
      invalid_public_key: tr('the_publisher_s_public_key_is'),
      invalid_signature: tr('the_extension_s_cryptographic_signature_is'),
      invalid_manifest: tr('the_extension_manifest_is_invalid'),
      module_too_large: tr('the_webassembly_module_exceeds_the_maximum'),
      invalid_module: tr('the_extension_s_webassembly_module_is'),
      invalid_digest: tr('the_module_s_sha_256_fingerprint'),
      incompatible_api: tr('the_extension_uses_an_incompatible_api'),
      repository_https_public_host_required: tr('the_repository_address_must_be_a'),
      repository_address_forbidden: tr('the_repository_s_ip_address_is'),
      repository_unavailable: tr('the_plugin_repository_is_currently'),
      repository_http_error: tr('the_repository_server_returned_a_download'),
      repository_response_too_large: tr('the_repository_response_exceeds_the_allowed'),
      repository_redirect_forbidden: tr('the_repository_redirects_to_a_disallowed'),
      invalid_repository_manifest: tr('the_repository_manifest_is_invalid_format'),
      invalid_package_archive: tr('the_package_archive_is_corrupted_or'),
      package_archive_ambiguous: tr('the_archive_contains_several_packages_a'),
      package_archive_without_package: tr('the_archive_contains_no_package_json'),
      package_too_large: tr('the_package_exceeds_the_allowed_size'),
      repository_limit: tr('the_limit_of_registered_repositories_has'),
      repository_exists: tr('this_repository_is_already_registered'),
      unknown_repository: tr('repository_not_found'),
      unknown_catalogue_version: tr('this_extension_version_does_not_exist'),
      repository_changed: tr('the_catalogue_has_changed_please_refresh'),
      extension_exists: tr('this_extension_is_already_installed'),
      extension_downgrade_incompatible: tr('this_older_version_cannot_be_installed'),
      extension_downgrade_snapshot_exists: tr('the_existing_backup_contains_private_data'),
      extension_changed: tr('the_installed_version_has_changed_refresh'),
      extension_version_not_newer: tr('the_proposed_version_must_be_higher'),
      publisher_key_mismatch: tr('the_signing_key_does_not_match'),
      extension_update_compatibility_unverified: tr('the_update_is_refused_data_compatibility'),
      extension_update_requires_authorization: tr('the_update_requires_new_access'),
      extension_limit: tr('the_limit_of_32_installed_extensions'),
      extension_unavailable: tr('this_extension_is_not_available'),
      task_already_active: tr('sched_err_already_active'),
      task_unavailable: tr('sched_err_unavailable'),
      task_not_active: tr('sched_err_not_active'),
      task_not_found: tr('sched_err_not_found'),
      trigger_invalid: tr('sched_err_trigger_invalid'),
      trigger_duplicate: tr('sched_err_trigger_duplicate'),
      trigger_limit: tr('sched_err_trigger_limit'),
      trigger_not_found: tr('sched_err_trigger_not_found'),
      trigger_unsupported: tr('sched_err_trigger_unsupported')
    };
    const code = detail?.pluginError ?? detail?.error;
    if (code && messages[code]) throw new Error(messages[code]);
    if (detail?.pluginError) throw new Error(tr('extension_2', { pluginError: detail.pluginError }));
    if (detail?.error) throw new Error(tr('error', { error: detail.error }));
    throw new Error(response.status === 401 ? tr('session_expired_sign_in_again') : response.status === 403 ? tr('admin_access_denied') : response.status === 409 ? tr('this_name_already_exists_or_the') : response.status === 400 ? tr('invalid_request_check_the_values_entered') : tr('operation_not_possible_try_again'));
  }
  if (init.method && init.method !== 'GET') {
    const library = path.match(/\/libraries\/([^/]+)/)?.[1];
    invalidateCatalog(library ? decodeURIComponent(library) : undefined);
  }
  return response.status === 204 ? undefined as T : response.json() as Promise<T>;
}

export interface UserGroup { id: string; name: string; isDefault?: boolean; systemRole?: 'user' | 'administrator' | null; userIds: string[]; libraryIds: string[]; allowedKinds?: string[]; allowMediaDownload?: boolean | null }
