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
      400: 'Saisissez les 8 caractères du code affiché sur le nouvel appareil.',
      401: 'Reconnectez-vous avant d’autoriser cet appareil.',
      403: 'Cette connexion n’est pas autorisée. Actualisez la page et réessayez.',
      404: 'Ce code est introuvable, a expiré ou a déjà été utilisé.',
      410: 'Ce QR code a expiré. Générez un nouveau code.',
      429: 'Trop de tentatives. Patientez une minute avant de réessayer.'
    };
    throw new Error(messages[response.status] ?? 'Connexion à cet appareil indisponible. Réessayez.');
  }
  return response.status === 204 ? undefined as T : await response.json() as T;
}
