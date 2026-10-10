import { type APIRequestContext, expect, request as pwRequest, test } from '@playwright/test';
import { totp } from '../../fixtures/totp';
import { serverNow } from '../../fixtures/accounts';

// E2E-R1-AUTH-01..07 — authentication over the real HTTP API (BPMS-021/022, ADR-0020, docs/06 §3).
// Requires an e2e build with a database (PROCERA_DATABASE_URL + PROCERA_MASTER_KEY + PROCERA_INSECURE_COOKIES=1).
const PW = 'correct horse battery staple';
const CSRF = { 'x-requested-with': 'procera' };
const BASE = process.env.PROCERA_BASE_URL ?? 'http://127.0.0.1:18080';

async function hasDatabase(request: APIRequestContext) {
  return (await (await request.get('/readyz')).json()).database === 'ok';
}

async function newUser(request: APIRequestContext, tag: string) {
  const email = `${tag}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}@acme.test`;
  const res = await request.post('/__test/users', { data: { email, display_name: tag, password: PW } });
  expect(res.status()).toBe(201);
  return email;
}

/** A fresh API context = a fresh "browser" with its own cookie jar. */
async function client() {
  return pwRequest.newContext({ baseURL: BASE, extraHTTPHeaders: { accept: 'application/json' } });
}

test.describe('authentication @r1 @auth', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-AUTH-01 login returns a short-lived bearer token; /me works only with it', async ({ request }) => {
    const email = await newUser(request, 'alice');
    const c = await client();
    const res = await c.post('/api/v1/auth/login', { data: { email, password: PW } });
    expect(res.status()).toBe(200);
    expect(res.headers()['cache-control']).toBe('no-store');
    const body = await res.json();
    expect(body.status).toBe('authenticated');
    expect(body.token_type).toBe('Bearer');
    expect(body.expires_in).toBeLessThanOrEqual(900);
    expect(res.headers()['set-cookie']).toMatch(/procera_refresh=.*HttpOnly.*SameSite=Strict/);
    expect(body.access_token).not.toContain(body.user.id ? 'refresh' : '');
    const me = await c.get('/api/v1/me', { headers: { authorization: `Bearer ${body.access_token}` } });
    expect(me.status()).toBe(200);
    expect((await me.json()).user.email).toBe(email);
    const anon = await c.get('/api/v1/me');
    expect(anon.status()).toBe(401);
    expect(anon.headers()['content-type']).toContain('application/problem+json');
    const forged = await c.get('/api/v1/me', { headers: { authorization: `Bearer ${body.access_token}x` } });
    expect(forged.status()).toBe(401);
  });

  test('E2E-R1-AUTH-02 wrong password and unknown account give the same answer', async ({ request }) => {
    const email = await newUser(request, 'bob');
    const c = await client();
    const a = await c.post('/api/v1/auth/login', { data: { email, password: 'wrong password!!' } });
    const b = await c.post('/api/v1/auth/login', { data: { email: `nobody-${Date.now()}@acme.test`, password: PW } });
    expect(a.status()).toBe(401);
    expect(b.status()).toBe(401);
    expect((await a.json()).code).toBe((await b.json()).code);
  });

  test('E2E-R1-AUTH-03 refresh rotates; replaying an old refresh token revokes the session', async ({ request }) => {
    const email = await newUser(request, 'carol');
    const c = await client();
    await c.post('/api/v1/auth/login', { data: { email, password: PW } });
    const cookieBefore = (await c.storageState()).cookies.find((k) => k.name.endsWith('procera_refresh'))!.value;
    const r1 = await c.post('/api/v1/auth/refresh', { headers: CSRF });
    expect(r1.status()).toBe(200);
    const cookieAfter = (await c.storageState()).cookies.find((k) => k.name.endsWith('procera_refresh'))!.value;
    expect(cookieAfter).not.toBe(cookieBefore);
    // An attacker replays the stolen, already-rotated token.
    const attacker = await client();
    const replay = await attacker.post('/api/v1/auth/refresh', { headers: { ...CSRF, cookie: `procera_refresh=${cookieBefore}` } });
    expect(replay.status()).toBe(401);
    expect((await replay.json()).code).toBe('token_reused');
    // The legitimate session is revoked too.
    expect((await c.post('/api/v1/auth/refresh', { headers: CSRF })).status()).toBe(401);
  });

  test('E2E-R1-AUTH-04 cookie endpoints require the anti-CSRF header', async ({ request }) => {
    const email = await newUser(request, 'dan');
    const c = await client();
    await c.post('/api/v1/auth/login', { data: { email, password: PW } });
    const noHeader = await c.post('/api/v1/auth/refresh');
    expect(noHeader.status()).toBe(403);
    expect((await noHeader.json()).code).toBe('csrf');
  });

  test('E2E-R1-AUTH-05 logout revokes the refresh token and clears the cookie', async ({ request }) => {
    const email = await newUser(request, 'erin');
    const c = await client();
    await c.post('/api/v1/auth/login', { data: { email, password: PW } });
    const out = await c.post('/api/v1/auth/logout', { headers: CSRF });
    expect(out.status()).toBe(204);
    expect(out.headers()['set-cookie']).toMatch(/Max-Age=0/);
    expect((await c.post('/api/v1/auth/refresh', { headers: CSRF })).status()).toBe(401);
  });

  test('E2E-R1-AUTH-06 TOTP enrolment, MFA login, code replay rejected, recovery code works once', async ({ request }) => {
    const email = await newUser(request, 'fay');
    const c = await client();
    const access = (await (await c.post('/api/v1/auth/login', { data: { email, password: PW } })).json()).access_token;
    const auth = { authorization: `Bearer ${access}` };
    const enr = await (await c.post('/api/v1/me/mfa/totp', { headers: auth })).json();
    expect(enr.otpauth_uri).toContain('otpauth://totp/Procera:');
    const now = await serverNow(c);
    const confirm = await c.post('/api/v1/me/mfa/totp/confirm', { headers: auth, data: { code: totp(enr.secret, now) } });
    expect(confirm.status()).toBe(200);
    const { recovery_codes } = await confirm.json();
    expect(recovery_codes).toHaveLength(10);

    const step1 = await (await c.post('/api/v1/auth/login', { data: { email, password: PW } })).json();
    expect(step1.status).toBe('mfa_required');
    expect((await c.get('/api/v1/me', { headers: { authorization: `Bearer ${step1.mfa_token}` } })).status()).toBe(401);
    const code = totp(enr.secret, now, 1);
    const ok = await c.post('/api/v1/auth/mfa', { data: { mfa_token: step1.mfa_token, code } });
    expect(ok.status()).toBe(200);
    expect((await (await c.get('/api/v1/me', { headers: { authorization: `Bearer ${(await ok.json()).access_token}` } })).json()).token.amr).toEqual(['pwd', 'otp']);
    expect((await c.post('/api/v1/auth/mfa', { data: { mfa_token: step1.mfa_token, code } })).status()).toBe(401);
    expect((await c.post('/api/v1/auth/mfa', { data: { mfa_token: step1.mfa_token, code: recovery_codes[0] } })).status()).toBe(200);
    expect((await c.post('/api/v1/auth/mfa', { data: { mfa_token: step1.mfa_token, code: recovery_codes[0] } })).status()).toBe(401);
  });

  test('E2E-R1-AUTH-07 sign-out-everywhere kills access tokens immediately; auth events are audited', async ({ request }) => {
    const email = await newUser(request, 'gus');
    const c1 = await client();
    const c2 = await client();
    const a1 = (await (await c1.post('/api/v1/auth/login', { data: { email, password: PW } })).json()).access_token;
    const a2 = (await (await c2.post('/api/v1/auth/login', { data: { email, password: PW } })).json()).access_token;
    expect((await c2.get('/api/v1/me', { headers: { authorization: `Bearer ${a2}` } })).status()).toBe(200);
    expect((await c1.post('/api/v1/me/sign-out-everywhere', { headers: { authorization: `Bearer ${a1}` } })).status()).toBe(204);
    // Device 2's still-unexpired JWT is rejected (token version bumped). Cache TTL ≤ 30 s; device 2 is checked
    // on the same server whose cache entry for this user was just invalidated.
    expect((await c2.get('/api/v1/me', { headers: { authorization: `Bearer ${a2}` } })).status()).toBe(401);
    expect((await c2.post('/api/v1/auth/refresh', { headers: CSRF })).status()).toBe(401);
    const audit = await (await request.get('/__test/audit/verify')).json();
    expect(audit.ok).toBe(true);
    expect(audit.checked).toBeGreaterThan(0);
  });
});
