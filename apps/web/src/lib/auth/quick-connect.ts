import { tr } from '../i18n';
export const PENDING_DEVICE_CODE = 'media.pending-device-code';
export function normalizeDeviceCode(value: string): string | null {
  const code = value.trim().replace(/[ \u2010-\u2015-]/g, '').toUpperCase();
  return /^[0-9A-F]{8}$/.test(code) ? code : null;
}
export async function deviceRequest<T>(action: string, body: object, csrfToken?: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(`/api/v1/auth/device/${action}`, {
    method: 'POST', credentials: 'same-origin', cache: 'no-store',
    headers: { 'content-type': 'application/json', ...(csrfToken ? { 'x-csrf-token': csrfToken } : {}) },
    body: JSON.stringify(body), signal: signal ? AbortSignal.any([signal, AbortSignal.timeout(10000)]) : AbortSignal.timeout(10000)
  });
  if (!response.ok) {
    const messages: Record<number, string> = {
      400: tr('enter_the_8_characters_of_the'),
      401: tr('sign_in_again_before_authorising_this'),
      403: tr('this_sign_in_is_not_allowed'),
      404: tr('this_code_was_not_found_has'),
      410: tr('auth_this_qr_code_has_expired_generate'),
      429: tr('too_many_attempts_wait_a_minute')
    };
    throw new Error(messages[response.status] ?? tr('signing_in_to_this_device_is'));
  }
  return response.status === 204 ? undefined as T : await response.json() as T;
}
