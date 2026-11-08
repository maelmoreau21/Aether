import { tr } from '../i18n';
export function providerErrorText(code: string): string {
  const messages: Record<string,string> = {
    unauthorized: tr('the_api_key_is_refused_by'),
    missing_api_key: tr('this_provider_s_api_key_is'),
    network_error: tr('unable_to_connect_to_the_provider'),
    provider_timeout: tr('the_provider_timed_out'),
    extension_execution_trapped: tr('the_provider_s_module_failed_while'),
    extension_output_too_large: tr('the_provider_returned_too_much_data'),
    extension_module_invalid: tr('the_installed_module_is_incompatible_update'),
    extension_failed: tr('the_installed_module_failed_check_its'),
    not_found: tr('no_image_for_this_title_at'),
    parse_error: tr('catalog_the_provider_s_response_is_unreadable'),
    provider_error: tr('the_provider_returned_an_error'),
  };
  return messages[code] ?? code;
}
