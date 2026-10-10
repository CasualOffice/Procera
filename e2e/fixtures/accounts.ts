import { type APIRequestContext, expect, request as pwRequest } from '@playwright/test';
import { totp } from './totp';

export const PW = 'correct horse battery staple';
export const BASE = process.env.PROCERA_BASE_URL ?? 'http://127.0.0.1:18080';

export async function hasDatabase(request: APIRequestContext) {
  return (await (await request.get('/readyz')).json()).database === 'ok';
}

export function uniqueEmail(tag: string) {
  return `${tag}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}@acme.test`;
}

/** A fresh API context = a fresh "browser" with its own cookie jar. */
export async function client() {
  return pwRequest.newContext({ baseURL: BASE, extraHTTPHeaders: { accept: 'application/json' } });
}

export async function seedUser(request: APIRequestContext, tag: string) {
  const email = uniqueEmail(tag);
  const res = await request.post('/__test/users', { data: { email, display_name: tag, password: PW } });
  expect(res.status()).toBe(201);
  return email;
}

/** Signs in with password only and returns the access token. */
export async function passwordSession(email: string) {
  const c = await client();
  const body = await (await c.post('/api/v1/auth/login', { data: { email, password: PW } })).json();
  expect(body.status).toBe('authenticated');
  return { c, token: body.access_token as string };
}

/** Server time (the e2e clock can be advanced by tests), so TOTP codes match the server. */
export async function serverNow(request: APIRequestContext): Promise<number> {
  return Date.parse((await (await request.get('/version')).json()).server_time);
}

/** Enrols TOTP, then signs in again with MFA. Returns an access token with amr=[pwd,otp]. */
export async function mfaSession(email: string) {
  const { c, token } = await passwordSession(email);
  const auth = { authorization: `Bearer ${token}` };
  const enr = await (await c.post('/api/v1/me/mfa/totp', { headers: auth })).json();
  const now = await serverNow(c);
  expect((await c.post('/api/v1/me/mfa/totp/confirm', { headers: auth, data: { code: totp(enr.secret, now) } })).status()).toBe(200);
  const step1 = await (await c.post('/api/v1/auth/login', { data: { email, password: PW } })).json();
  const done = await (await c.post('/api/v1/auth/mfa', { data: { mfa_token: step1.mfa_token, code: totp(enr.secret, now, 1) } })).json();
  return { c, token: done.access_token as string, headers: { authorization: `Bearer ${done.access_token}` } };
}
