/**
 * WebAuthn glue (BPMS-022): turns the server's JSON ceremony options into browser options and the
 * browser's credential back into JSON (binary fields base64url), without relying on the newer
 * `parseCreationOptionsFromJSON` / `toJSON` helpers that not every browser has yet.
 */
import { api } from './api';

type Json = Record<string, unknown>;

export function passkeysSupported(): boolean {
  return typeof window !== 'undefined' && 'PublicKeyCredential' in window && !!navigator.credentials;
}

function fromB64(s: string): ArrayBuffer {
  const b = atob(s.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((s.length + 3) % 4));
  const out = new Uint8Array(b.length);
  for (let i = 0; i < b.length; i++) out[i] = b.charCodeAt(i);
  return out.buffer;
}

function toB64(buf: ArrayBuffer | null | undefined): string | undefined {
  if (!buf) return undefined;
  let s = '';
  for (const byte of new Uint8Array(buf)) s += String.fromCharCode(byte);
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

const descriptors = (list: unknown) => ((list as Json[] | undefined) ?? []).map((d) => ({ ...d, id: fromB64(d.id as string) })) as PublicKeyCredentialDescriptor[];

function serialize(c: PublicKeyCredential): Json {
  const r = c.response as AuthenticatorAttestationResponse & AuthenticatorAssertionResponse;
  const response: Json = { clientDataJSON: toB64(r.clientDataJSON) };
  if ('attestationObject' in r) {
    response.attestationObject = toB64(r.attestationObject);
    response.transports = typeof r.getTransports === 'function' ? r.getTransports() : [];
  }
  if ('authenticatorData' in r) {
    response.authenticatorData = toB64(r.authenticatorData);
    response.signature = toB64(r.signature);
  }
  return { id: c.id, rawId: toB64(c.rawId), type: c.type, response };
}

/** Thrown when the person closes the browser prompt or it times out; not an error to shout about. */
export class PasskeyCancelled extends Error {}

function cancelled(e: unknown): boolean {
  return e instanceof DOMException && (e.name === 'NotAllowedError' || e.name === 'AbortError');
}

/** Registration: fetch options, run `navigator.credentials.create()`, return the credential JSON. */
export async function createPasskey(): Promise<Json> {
  const { public_key: o } = await api<{ public_key: Json }>('/api/v1/me/mfa/webauthn/options', { method: 'POST' });
  const user = o.user as Json;
  try {
    const c = (await navigator.credentials.create({
      publicKey: {
        ...(o as object),
        challenge: fromB64(o.challenge as string),
        user: { ...user, id: fromB64(user.id as string) },
        excludeCredentials: descriptors(o.excludeCredentials),
      } as PublicKeyCredentialCreationOptions,
    })) as PublicKeyCredential | null;
    if (!c) throw new PasskeyCancelled();
    return serialize(c);
  } catch (e) {
    if (cancelled(e)) throw new PasskeyCancelled();
    throw e;
  }
}

/** Assertion for sign-in or step-up: `options` comes from the matching `/options` endpoint. */
export async function getPasskey(options: { public_key: Json }): Promise<Json> {
  const o = options.public_key;
  try {
    const c = (await navigator.credentials.get({
      publicKey: { ...(o as object), challenge: fromB64(o.challenge as string), allowCredentials: descriptors(o.allowCredentials) } as PublicKeyCredentialRequestOptions,
    })) as PublicKeyCredential | null;
    if (!c) throw new PasskeyCancelled();
    return serialize(c);
  } catch (e) {
    if (cancelled(e)) throw new PasskeyCancelled();
    throw e;
  }
}
