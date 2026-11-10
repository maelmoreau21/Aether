// WebAuthn ceremonies in the browser. Core sends options as JSON (base64url binary fields)
// and verifies the result; nothing secret is kept here.

export interface RequestOptionsJSON { challenge: string; rpId: string; timeout: number; userVerification: UserVerificationRequirement }
export interface CreationOptionsJSON {
  challenge: string;
  rp: { id: string; name: string };
  user: { id: string; name: string; displayName: string };
  pubKeyCredParams: { type: 'public-key'; alg: number }[];
  timeout: number;
  attestation: AttestationConveyancePreference;
  authenticatorSelection: AuthenticatorSelectionCriteria;
  excludeCredentials: { type: 'public-key'; id: string }[];
}
export interface AssertionJSON { credentialId: string; clientDataJson: string; authenticatorData: string; signature: string; userHandle?: string }
export interface RegistrationJSON { credentialId: string; clientDataJson: string; authenticatorData: string; publicKey: string; publicKeyAlgorithm: number }

export function passkeysSupported(): boolean {
  return typeof window !== 'undefined' && typeof window.PublicKeyCredential === 'function' && !!navigator.credentials;
}

export function toBase64Url(data: ArrayBuffer | Uint8Array): string {
  const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function fromBase64Url(text: string): Uint8Array<ArrayBuffer> {
  const base64 = text.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((text.length + 3) % 4);
  const binary = atob(base64);
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export async function getAssertion(options: RequestOptionsJSON): Promise<AssertionJSON> {
  const credential = (await navigator.credentials.get({
    publicKey: {
      challenge: fromBase64Url(options.challenge),
      rpId: options.rpId,
      timeout: options.timeout,
      userVerification: options.userVerification
    }
  })) as PublicKeyCredential | null;
  if (!credential) throw new Error('passkey_cancelled');
  const response = credential.response as AuthenticatorAssertionResponse;
  return {
    credentialId: toBase64Url(credential.rawId),
    clientDataJson: toBase64Url(response.clientDataJSON),
    authenticatorData: toBase64Url(response.authenticatorData),
    signature: toBase64Url(response.signature),
    userHandle: response.userHandle ? toBase64Url(response.userHandle) : undefined
  };
}

export async function createPasskey(options: CreationOptionsJSON): Promise<RegistrationJSON> {
  const credential = (await navigator.credentials.create({
    publicKey: {
      challenge: fromBase64Url(options.challenge),
      rp: options.rp,
      user: { ...options.user, id: fromBase64Url(options.user.id) },
      pubKeyCredParams: options.pubKeyCredParams,
      timeout: options.timeout,
      attestation: options.attestation,
      authenticatorSelection: options.authenticatorSelection,
      excludeCredentials: options.excludeCredentials.map((c) => ({ type: c.type, id: fromBase64Url(c.id) }))
    }
  })) as PublicKeyCredential | null;
  if (!credential) throw new Error('passkey_cancelled');
  const response = credential.response as AuthenticatorAttestationResponse;
  const publicKey = response.getPublicKey?.();
  const algorithm = response.getPublicKeyAlgorithm?.();
  if (!publicKey || typeof algorithm !== 'number') throw new Error('passkey_unsupported');
  return {
    credentialId: toBase64Url(credential.rawId),
    clientDataJson: toBase64Url(response.clientDataJSON),
    authenticatorData: toBase64Url(response.getAuthenticatorData()),
    publicKey: toBase64Url(publicKey),
    publicKeyAlgorithm: algorithm
  };
}
