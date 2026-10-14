import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase } from '../../fixtures/accounts';
import { oidcProvider } from '../../fixtures/oidc';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-SSO-01..02 — OpenID Connect single sign-on (BPMS-140, docs/06 §3.3, E2E-R4-01): an owner sets
// the workspace's provider (its discovery document must answer); a member whose verified email is
// in the trusted domain signs in through the provider (code + PKCE + nonce) and gets a session in the
// workspace with amr `sso`; later sign-ins match by subject; a reused state, a wrong nonce, an
// unverified email and an untrusted domain are refused back to the sign-in page; with SSO required,
// password sessions are refused for members but not the owner.

const CSRF = { 'x-requested-with': 'procera' };
const domain = () => `sso-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}.test`;

async function member(request: APIRequestContext, owner: Session, ws: string, email: string, name: string) {
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
}

/** Runs the browser's redirects by hand: start → provider → callback. Returns the final redirect. */
async function signIn(c: APIRequestContext, ws: string) {
  const start = await c.get(`/api/v1/auth/sso/start?workspace=${ws}&return_to=/work`, { maxRedirects: 0 });
  expect(start.status(), await start.text()).toBe(303);
  const idp = await c.get(start.headers().location!, { maxRedirects: 0 });
  expect(idp.status()).toBe(302);
  const back = new URL(idp.headers().location!);
  const cb = await c.get(`${back.pathname}${back.search}`, { maxRedirects: 0 });
  expect(cb.status()).toBe(303);
  return { location: cb.headers().location!, callback: `${back.pathname}${back.search}` };
}

test.describe('single sign-on @r4 @sso', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-SSO-01 a member signs in through the provider and lands in the workspace; refusals go back to sign-in', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const idp = await oidcProvider({ clientId: 'procera', clientSecret: 's3cret' });
    const dom = domain();
    try {
      expect((await owner.c.get('/api/v1/sso', { headers: h })).ok()).toBe(true);
      const bad = await owner.c.put('/api/v1/sso', { headers: h, data: { issuer: 'https://10.0.0.9/realms/x', client_id: 'procera', domains: [dom] } });
      expect(bad.status()).toBe(422);
      const saved = await owner.c.put('/api/v1/sso', { headers: h, data: { issuer: idp.issuer, client_id: 'procera', client_secret: 's3cret', domains: [dom.toUpperCase()] } });
      expect(saved.status(), await saved.text()).toBe(200);
      const cfg = (await saved.json()).config;
      expect(cfg).toMatchObject({ issuer: idp.issuer, has_secret: true, domains: [dom], enabled: true, required: false });
      expect(JSON.stringify(cfg)).not.toContain('s3cret');

      const email = `ann@${dom}`;
      await member(request, owner, ws.id, email, 'Ann Asker');
      const anon = await client();
      expect((await (await anon.post('/api/v1/auth/sso/discover', { data: { email } })).json()).items).toEqual([expect.objectContaining({ id: ws.id })]);
      expect((await (await anon.post('/api/v1/auth/sso/discover', { data: { email: 'x@elsewhere.test' } })).json()).items).toEqual([]);

      // Signs in: the session is in the workspace, through SSO.
      idp.as({ sub: 'kc-ann', email, email_verified: true, amr: ['pwd', 'otp'] });
      const c = await client();
      const first = await signIn(c, ws.id);
      expect(first.location).toBe('/work');
      const tok = await (await c.post('/api/v1/auth/refresh', { headers: CSRF })).json();
      const me = await (await c.get('/api/v1/me', { headers: { authorization: `Bearer ${tok.access_token}` } })).json();
      expect(me).toMatchObject({ user: { email }, token: { workspace: ws.id, amr: ['sso', 'mfa'] } });
      expect((await c.get('/api/v1/work', { headers: { authorization: `Bearer ${tok.access_token}` } })).status()).toBe(200);

      // The same state can't be used twice.
      const again = await c.get(first.callback, { maxRedirects: 0 });
      expect(again.headers().location).toMatch(/^\/sign-in\?sso_error=/);

      // Later sign-ins match the subject even if the provider's email changes.
      idp.as({ sub: 'kc-ann', email: `ann.renamed@${dom}`, email_verified: true });
      expect((await signIn(await client(), ws.id)).location).toBe('/work');

      // Refusals: a wrong nonce, an unverified email, an untrusted domain, someone who isn't a member.
      idp.wrongNonce('evil');
      idp.as({ sub: 'kc-ann', email, email_verified: true });
      expect(decodeURIComponent((await signIn(await client(), ws.id)).location)).toContain('nonce');
      idp.wrongNonce(null);
      idp.as({ sub: 'kc-new', email: `bo@${dom}`, email_verified: false });
      expect(decodeURIComponent((await signIn(await client(), ws.id)).location)).toContain('verified email');
      idp.as({ sub: 'kc-new2', email: 'bo@elsewhere.test', email_verified: true });
      expect(decodeURIComponent((await signIn(await client(), ws.id)).location)).toContain('domain');
      idp.as({ sub: 'kc-new3', email: `stranger@${dom}`, email_verified: true });
      expect(decodeURIComponent((await signIn(await client(), ws.id)).location)).toContain('not a member');
    } finally { await idp.close(); }
  });

  test('E2E-R4-SSO-02 with SSO required, password sessions are refused for members but the owner keeps a way in', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const idp = await oidcProvider({ clientId: 'procera', clientSecret: 's3cret' });
    const dom = domain();
    try {
      const email = `mo@${dom}`;
      await member(request, owner, ws.id, email, 'Mo Member');
      const pw = await enter(email, ws.id); // a password session from before
      expect((await pw.c.get('/api/v1/work', { headers: pw.headers })).status()).toBe(200);
      const saved = await owner.c.put('/api/v1/sso', { headers: h, data: { issuer: idp.issuer, client_id: 'procera', client_secret: 's3cret', domains: [dom], required: true } });
      expect(saved.status(), await saved.text()).toBe(200);
      const refused = await pw.c.get('/api/v1/work', { headers: pw.headers });
      expect(refused.status()).toBe(403);
      expect((await refused.json()).code).toBe('sso_required');
      expect((await owner.c.get('/api/v1/work', { headers: h })).status()).toBe(200); // break-glass owner
      // Through the provider it works.
      idp.as({ sub: 'kc-mo', email, email_verified: true });
      const c = await client();
      await signIn(c, ws.id);
      const tok = await (await c.post('/api/v1/auth/refresh', { headers: CSRF })).json();
      expect((await c.get('/api/v1/work', { headers: { authorization: `Bearer ${tok.access_token}` } })).status()).toBe(200);
      // Removing SSO lets passwords back in.
      expect((await owner.c.delete('/api/v1/sso', { headers: h })).status()).toBe(204);
      expect((await pw.c.get('/api/v1/work', { headers: pw.headers })).status()).toBe(200);
    } finally { await idp.close(); }
  });
});
