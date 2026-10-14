import { createHash, createSign, generateKeyPairSync, randomUUID } from 'node:crypto';
import { type Server, createServer } from 'node:http';
import type { AddressInfo } from 'node:net';

/**
 * A minimal OpenID Connect provider for E2E (BPMS-140): discovery, JWKS (RS256), an authorize
 * endpoint that signs in `who` without a login screen, and a token endpoint that checks the code,
 * the PKCE verifier and the client secret, then returns an ID token. It listens on 127.0.0.1, which
 * e2e servers allow through `PROCERA_EGRESS_ALLOW`.
 */
export interface Who { sub: string; email?: string; email_verified?: boolean; amr?: string[] }

export async function oidcProvider(opts: { clientId: string; clientSecret: string }) {
  const { privateKey, publicKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
  const kid = randomUUID();
  const jwk = { ...(publicKey.export({ format: 'jwk' }) as Record<string, string>), kid, alg: 'RS256', use: 'sig' };
  const codes = new Map<string, { nonce: string; challenge: string; who: Who; redirect: string }>();
  const state = { who: { sub: 'user-1', email_verified: true } as Who, nonceOverride: null as string | null, tokenCalls: 0 };
  let issuer = '';

  const b64u = (b: Buffer | string) => Buffer.from(b).toString('base64url');
  const sign = (claims: Record<string, unknown>) => {
    const head = b64u(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid }));
    const body = b64u(JSON.stringify(claims));
    const sig = createSign('RSA-SHA256').update(`${head}.${body}`).sign(privateKey);
    return `${head}.${body}.${b64u(sig)}`;
  };

  const server: Server = createServer((req, res) => {
    const url = new URL(req.url ?? '/', issuer);
    const json = (code: number, v: unknown) => { res.writeHead(code, { 'content-type': 'application/json' }); res.end(JSON.stringify(v)); };
    if (url.pathname === '/.well-known/openid-configuration') {
      return json(200, { issuer, authorization_endpoint: `${issuer}/authorize`, token_endpoint: `${issuer}/token`, jwks_uri: `${issuer}/jwks`, response_types_supported: ['code'] });
    }
    if (url.pathname === '/jwks') return json(200, { keys: [jwk] });
    if (url.pathname === '/authorize') {
      const p = url.searchParams;
      if (p.get('client_id') !== opts.clientId || p.get('code_challenge_method') !== 'S256') return json(400, { error: 'invalid_request' });
      const code = randomUUID();
      codes.set(code, { nonce: p.get('nonce') ?? '', challenge: p.get('code_challenge') ?? '', who: { ...state.who }, redirect: p.get('redirect_uri') ?? '' });
      const back = new URL(p.get('redirect_uri')!);
      back.searchParams.set('code', code);
      back.searchParams.set('state', p.get('state') ?? '');
      res.writeHead(302, { location: back.toString() });
      return res.end();
    }
    if (url.pathname === '/token' && req.method === 'POST') {
      let raw = '';
      req.on('data', (c) => (raw += c));
      req.on('end', () => {
        state.tokenCalls++;
        const f = new URLSearchParams(raw);
        const c = codes.get(f.get('code') ?? '');
        codes.delete(f.get('code') ?? '');
        const pkce = createHash('sha256').update(f.get('code_verifier') ?? '').digest('base64url');
        if (!c || pkce !== c.challenge || f.get('client_id') !== opts.clientId || f.get('client_secret') !== opts.clientSecret || f.get('redirect_uri') !== c.redirect) {
          return json(400, { error: 'invalid_grant' });
        }
        const now = Math.floor(Date.now() / 1000);
        const id_token = sign({ iss: issuer, aud: opts.clientId, sub: c.who.sub, email: c.who.email, email_verified: c.who.email_verified, amr: c.who.amr, nonce: state.nonceOverride ?? c.nonce, iat: now, exp: now + 300 });
        return json(200, { id_token, access_token: 'opaque', token_type: 'Bearer', expires_in: 300 });
      });
      return;
    }
    json(404, { error: 'not_found' });
  });
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
  issuer = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  return {
    issuer,
    /** Who the next authorize request signs in. */
    as(who: Who) { state.who = who; },
    /** Puts a wrong nonce into the next ID tokens (null restores). */
    wrongNonce(v: string | null) { state.nonceOverride = v; },
    get tokenCalls() { return state.tokenCalls; },
    close: () => new Promise<void>((r) => server.close(() => r())),
  };
}
