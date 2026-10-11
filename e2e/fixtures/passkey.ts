import type { APIRequestContext } from '@playwright/test';
import { createHash, generateKeyPairSync, randomBytes, sign, type KeyObject } from 'node:crypto';

/**
 * A software WebAuthn authenticator (ES256) for API-level E2E (BPMS-022). It answers the server's
 * `{public_key}` ceremony options with responses in the browser's JSON shape, like a real
 * platform authenticator with attestation `none`. Browser journeys use Chromium's CDP virtual
 * authenticator instead (see `virtualAuthenticator`).
 */

const b64 = (b: Buffer) => b.toString('base64url');
const sha256 = (b: Buffer | string) => createHash('sha256').update(b).digest();

// Minimal CBOR encoder (RFC 8949): unsigned/negative integers, byte and text strings, arrays, maps.
type Cbor = number | string | Buffer | Cbor[] | Map<Cbor, Cbor>;
function head(major: number, n: number): Buffer {
  if (n < 24) return Buffer.from([(major << 5) | n]);
  if (n < 256) return Buffer.from([(major << 5) | 24, n]);
  if (n < 65536) { const b = Buffer.alloc(3); b[0] = (major << 5) | 25; b.writeUInt16BE(n, 1); return b; }
  const b = Buffer.alloc(5); b[0] = (major << 5) | 26; b.writeUInt32BE(n, 1); return b;
}
export function cbor(v: Cbor): Buffer {
  if (typeof v === 'number') return v >= 0 ? head(0, v) : head(1, -1 - v);
  if (typeof v === 'string') { const s = Buffer.from(v, 'utf8'); return Buffer.concat([head(3, s.length), s]); }
  if (Buffer.isBuffer(v)) return Buffer.concat([head(2, v.length), v]);
  if (Array.isArray(v)) return Buffer.concat([head(4, v.length), ...v.map(cbor)]);
  return Buffer.concat([head(5, v.size), ...[...v].flatMap(([k, x]) => [cbor(k), cbor(x)])]);
}

export type Options = { public_key: { challenge: string; rp?: { id: string }; rpId?: string } };

export class SoftAuthenticator {
  readonly id = randomBytes(32);
  counter = 0;
  private readonly key: KeyObject;
  private readonly cose: Buffer;

  /** `origin` must be one the server allows (its public URL, loopback IPs read as `localhost`). */
  constructor(public origin: string, public rpId = new URL(origin).hostname) {
    const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'P-256' });
    this.key = privateKey;
    const jwk = publicKey.export({ format: 'jwk' });
    this.cose = cbor(new Map<Cbor, Cbor>([[1, 2], [3, -7], [-1, 1], [-2, Buffer.from(jwk.x!, 'base64url')], [-3, Buffer.from(jwk.y!, 'base64url')]]));
  }

  private authData(attested: boolean): Buffer {
    const count = Buffer.alloc(4);
    count.writeUInt32BE(this.counter);
    const flags = Buffer.from([0x01 | 0x04 | (attested ? 0x40 : 0)]); // UP, UV, AT
    const parts = [sha256(this.rpId), flags, count];
    if (attested) {
      const len = Buffer.alloc(2);
      len.writeUInt16BE(this.id.length);
      parts.push(Buffer.alloc(16), len, this.id, this.cose);
    }
    return Buffer.concat(parts);
  }

  private clientData(type: string, o: Options) {
    return Buffer.from(JSON.stringify({ type, challenge: o.public_key.challenge, origin: this.origin, crossOrigin: false }));
  }

  /** Answer to `navigator.credentials.create()`. */
  register(o: Options) {
    const att = cbor(new Map<Cbor, Cbor>([['fmt', 'none'], ['attStmt', new Map()], ['authData', this.authData(true)]]));
    return { id: b64(this.id), rawId: b64(this.id), type: 'public-key', response: { clientDataJSON: b64(this.clientData('webauthn.create', o)), attestationObject: b64(att), transports: ['internal'] } };
  }

  /** Answer to `navigator.credentials.get()`; bumps the signature counter. */
  assert(o: Options) {
    this.counter += 1;
    const cd = this.clientData('webauthn.get', o);
    const ad = this.authData(false);
    const signature = sign('sha256', Buffer.concat([ad, sha256(cd)]), { key: this.key, dsaEncoding: 'der' });
    return { id: b64(this.id), rawId: b64(this.id), type: 'public-key', response: { clientDataJSON: b64(cd), authenticatorData: b64(ad), signature: b64(signature) } };
  }
}

/** The relying party the server enforces (its public URL; a loopback IP reads as `localhost`). */
export async function relyingParty(request: APIRequestContext): Promise<{ rp_id: string; origin: string }> {
  const r = await (await request.get('/__test/webauthn')).json();
  return { rp_id: r.rp_id, origin: r.origins[0] };
}

/** A software authenticator for the server's relying party. */
export async function softAuthenticator(request: APIRequestContext) {
  const { rp_id, origin } = await relyingParty(request);
  return new SoftAuthenticator(origin, rp_id);
}
