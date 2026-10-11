import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { type Options, softAuthenticator } from '../../fixtures/passkey';

// E2E-R1-AUTH-20..23 — WebAuthn passkeys as a second factor and for step-up (BPMS-022, docs/06 §3.6,
// docs/12 §3/§11). A software ES256 authenticator answers for the server's relying party.

const bearer = (t: string) => ({ authorization: `Bearer ${t}` });

/** Registers a passkey for a password-only account; returns the registration body and the authenticator. */
async function enrolPasskey(request: APIRequestContext, tag: string) {
  const email = await seedUser(request, tag);
  const { c, token } = await passwordSession(email);
  const key = await softAuthenticator(request);
  const opts: Options = await (await c.post('/api/v1/me/mfa/webauthn/options', { headers: bearer(token) })).json();
  const res = await c.post('/api/v1/me/mfa/webauthn', { headers: bearer(token), data: { name: 'Work laptop', credential: key.register(opts) } });
  expect(res.status(), await res.text()).toBe(201);
  return { email, c, token, key, body: await res.json(), opts };
}

/** Password, then passkey. Returns the access token. */
async function signInWithPasskey(c: APIRequestContext, email: string, key: Awaited<ReturnType<typeof softAuthenticator>>) {
  const step1 = await (await c.post('/api/v1/auth/login', { data: { email, password: PW } })).json();
  expect(step1.status).toBe('mfa_required');
  const opts = await (await c.post('/api/v1/auth/mfa/webauthn/options', { data: { mfa_token: step1.mfa_token } })).json();
  const done = await c.post('/api/v1/auth/mfa', { data: { mfa_token: step1.mfa_token, credential: key.assert(opts) } });
  expect(done.status(), await done.text()).toBe(200);
  return (await done.json()).access_token as string;
}

test.describe('passkeys @r1 @auth', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-AUTH-20 register a passkey: options, single-use challenge, recovery codes once, duplicates refused', async ({ request }) => {
    const { c, token, key, body, opts } = await enrolPasskey(request, 'pk-reg');
    expect(opts.public_key.rp?.id).toBeTruthy();
    expect(body.passkey).toMatchObject({ name: 'Work laptop', transports: ['internal'] });
    expect(body.recovery_codes).toHaveLength(10);
    expect(JSON.stringify(body.passkey)).not.toContain('secret');
    // The same challenge cannot be used twice.
    const replay = await c.post('/api/v1/me/mfa/webauthn', { headers: bearer(token), data: { name: 'Again', credential: key.register(opts) } });
    expect(replay.status()).toBe(422);
    expect((await replay.json()).code).toBe('passkey_invalid');
    // The authenticator is excluded and cannot be registered twice.
    const opts2 = await (await c.post('/api/v1/me/mfa/webauthn/options', { headers: bearer(token) })).json();
    expect(opts2.public_key.excludeCredentials).toHaveLength(1);
    const dup = await c.post('/api/v1/me/mfa/webauthn', { headers: bearer(token), data: { name: 'Again', credential: key.register(opts2) } });
    expect(dup.status()).toBe(409);
    // A second passkey does not reissue recovery codes.
    const other = await softAuthenticator(request);
    const opts3 = await (await c.post('/api/v1/me/mfa/webauthn/options', { headers: bearer(token) })).json();
    const second = await (await c.post('/api/v1/me/mfa/webauthn', { headers: bearer(token), data: { name: 'Phone', credential: other.register(opts3) } })).json();
    expect(second.recovery_codes).toBeNull();
    const list = await (await c.get('/api/v1/me/mfa/webauthn', { headers: bearer(token) })).json();
    expect(list.items.map((p: { name: string }) => p.name)).toEqual(['Work laptop', 'Phone']);
    const me = await (await c.get('/api/v1/me', { headers: bearer(token) })).json();
    expect(me.user).toMatchObject({ mfa_enabled: true, mfa_methods: ['webauthn'] });
    // Unauthenticated callers get nothing.
    expect((await (await client()).post('/api/v1/me/mfa/webauthn/options')).status()).toBe(401);
  });

  test('E2E-R1-AUTH-21 sign in with password + passkey; phishing origin and replayed assertions fail', async ({ request }) => {
    const { email, key } = await enrolPasskey(request, 'pk-login');
    const c = await client();
    const step1 = await (await c.post('/api/v1/auth/login', { data: { email, password: PW } })).json();
    expect(step1).toMatchObject({ status: 'mfa_required', methods: ['webauthn', 'recovery_code'] });
    // A response made for another site is rejected (and spends the challenge).
    const opts = await (await c.post('/api/v1/auth/mfa/webauthn/options', { data: { mfa_token: step1.mfa_token } })).json();
    expect(opts.public_key.allowCredentials).toHaveLength(1);
    const real = key.origin;
    key.origin = 'https://procera.evil.test';
    const phish = await c.post('/api/v1/auth/mfa', { data: { mfa_token: step1.mfa_token, credential: key.assert(opts) } });
    expect(phish.status()).toBe(401);
    expect((await phish.json()).code).toBe('passkey_rejected');
    key.origin = real;
    expect((await c.post('/api/v1/auth/mfa', { data: { mfa_token: step1.mfa_token, credential: key.assert(opts) } })).status()).toBe(401);
    // Neither or both factors is a validation error.
    expect((await c.post('/api/v1/auth/mfa', { data: { mfa_token: step1.mfa_token } })).status()).toBe(422);
    const token = await signInWithPasskey(c, email, key);
    const me = await (await c.get('/api/v1/me', { headers: bearer(token) })).json();
    expect(me.token.amr).toEqual(['pwd', 'hwk']);
    // A cloned authenticator (counter reset) is refused.
    key.counter = 0;
    const s2 = await (await c.post('/api/v1/auth/login', { data: { email, password: PW } })).json();
    const o2 = await (await c.post('/api/v1/auth/mfa/webauthn/options', { data: { mfa_token: s2.mfa_token } })).json();
    const cloned = await c.post('/api/v1/auth/mfa', { data: { mfa_token: s2.mfa_token, credential: key.assert(o2) } });
    expect(cloned.status()).toBe(401);
    expect((await cloned.json()).detail).toContain('counter');
  });

  test('E2E-R1-AUTH-22 step up with a passkey; a sign-in challenge cannot be reused for step-up', async ({ request }) => {
    const { email, key } = await enrolPasskey(request, 'pk-stepup');
    const c = await client();
    const token = await signInWithPasskey(c, email, key);
    const old = (await (await c.post('/__test/session/backdate', { headers: bearer(token), data: { seconds: 3600 } })).json()).access_token;
    // Without a factor, an MFA account cannot step up, and the password alone is not enough.
    expect((await c.post('/api/v1/session/step-up', { headers: bearer(old), data: { password: PW } })).status()).toBe(401);
    const s1 = await (await c.post('/api/v1/auth/login', { data: { email, password: PW } })).json();
    const loginOpts = await (await c.post('/api/v1/auth/mfa/webauthn/options', { data: { mfa_token: s1.mfa_token } })).json();
    const wrong = await c.post('/api/v1/session/step-up', { headers: bearer(old), data: { credential: key.assert(loginOpts) } });
    expect((await wrong.json()).code).toBe('passkey_rejected');
    const opts = await (await c.post('/api/v1/session/step-up/webauthn/options', { headers: bearer(old) })).json();
    const up = await c.post('/api/v1/session/step-up', { headers: bearer(old), data: { credential: key.assert(opts) } });
    expect(up.status(), await up.text()).toBe(200);
    const fresh = await (await c.get('/api/v1/me', { headers: bearer((await up.json()).access_token) })).json();
    const stale = await (await c.get('/api/v1/me', { headers: bearer(old) })).json();
    expect(fresh.token.auth_time).toBeGreaterThan(stale.token.auth_time);
    expect(fresh.token.amr).toContain('hwk');
  });

  test('E2E-R1-AUTH-23 removing a passkey needs step-up; another user’s passkey is 404; the last one turns MFA off', async ({ request }) => {
    const { email, key } = await enrolPasskey(request, 'pk-remove');
    const c = await client();
    const token = await signInWithPasskey(c, email, key);
    const [pk] = (await (await c.get('/api/v1/me/mfa/webauthn', { headers: bearer(token) })).json()).items;
    const old = (await (await c.post('/__test/session/backdate', { headers: bearer(token), data: { seconds: 600 } })).json()).access_token;
    const denied = await c.delete(`/api/v1/me/mfa/webauthn/${pk.id}`, { headers: bearer(old) });
    expect(denied.status()).toBe(401);
    expect((await denied.json()).code).toBe('step_up_required');
    // Someone else cannot remove it, even freshly signed in.
    const mallory = await passwordSession(await seedUser(request, 'pk-mallory'));
    expect((await mallory.c.delete(`/api/v1/me/mfa/webauthn/${pk.id}`, { headers: bearer(mallory.token) })).status()).toBe(404);
    expect((await c.delete(`/api/v1/me/mfa/webauthn/${pk.id}`, { headers: bearer(token) })).status()).toBe(204);
    expect((await (await c.get('/api/v1/me/mfa/webauthn', { headers: bearer(token) })).json()).items).toEqual([]);
    const again = await (await c.post('/api/v1/auth/login', { data: { email, password: PW } })).json();
    expect(again.status).toBe('authenticated');
    const audit = await (await request.get('/__test/audit/verify')).json();
    expect(audit.ok).toBe(true);
  });
});
