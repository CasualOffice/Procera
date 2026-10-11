import { type APIRequestContext, expect } from '@playwright/test';
import { PW, client, mfaSession, seedUser, serverNow, uniqueEmail } from './accounts';

// Workspace onboarding helpers shared by the R1 packs.

export type Session = { c: APIRequestContext; headers: Record<string, string>; secret?: string; recovery?: string[] };

export async function sysAdmin(request: APIRequestContext) {
  const email = await seedUser(request, 'sysadmin');
  await request.post('/__test/system-roles', { data: { email, role: 'system.admin' } });
  return mfaSession(email);
}

export async function inviteToken(request: APIRequestContext, workspace: string, email: string) {
  const res = await request.get(`/__test/invitations/token?workspace=${workspace}&email=${encodeURIComponent(email)}`);
  expect(res.status()).toBe(200);
  return (await res.json()).token as string;
}

/**
 * Signs in with MFA (enrolling TOTP; call once per user per test) and switches to `workspace`.
 * Returns a session whose bearer is workspace-scoped, plus the TOTP secret for step-up.
 */
export async function enter(email: string, workspace: string): Promise<Session & { secret: string; recovery: string[] }> {
  const { c, headers, secret, recovery } = await mfaSession(email);
  const sw = await c.post('/api/v1/session/workspace', { headers, data: { workspace_id: workspace } });
  expect(sw.status()).toBe(200);
  return { c, headers: { authorization: `Bearer ${(await sw.json()).access_token}` }, secret, recovery };
}

/** System admin creates a workspace; the owner registers via the invitation and signs into it. */
export async function workspaceWithOwner(request: APIRequestContext, profile = 'standard') {
  const sys = await sysAdmin(request);
  const ownerEmail = uniqueEmail('owner');
  const created = await sys.c.post('/sysapi/v1/workspaces', { headers: sys.headers, data: { name: 'Acme', slug: `acme-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}`, profile, owner_email: ownerEmail } });
  expect(created.ok(), `create workspace: ${created.status()} ${await created.text()}`).toBe(true);
  const ws = await created.json();
  const token = await inviteToken(request, ws.id, ownerEmail);
  const anon = await client();
  const reg = await anon.post('/api/v1/invitations/register', { data: { token, display_name: 'Olivia', password: PW } });
  expect(reg.status()).toBe(201);
  // Emails go out immediately in test workspaces: the product defaults (daily digest, quiet hours
  // 20:00–08:00) depend on the time of day. Delivery-preference tests turn them back on (BPMS-048).
  for (const [key, value] of [['notify.digest.schedule', 'off'], ['notify.quiet_hours.enabled', false]] as const) {
    expect((await request.post('/__test/settings', { data: { workspace: ws.id, key, value } })).ok()).toBe(true);
  }
  return { sys, ws, ownerEmail, owner: await enter(ownerEmail, ws.id) };
}

/**
 * Re-signs the session's token with auth_time = server now (test control), so a step-up action
 * works even if another test moved the shared clock. Returns headers with the fresh bearer.
 */
export async function freshAuth(s: Session): Promise<Record<string, string>> {
  const token = (s.headers.authorization ?? '').replace(/^Bearer /, '');
  const { auth_time } = JSON.parse(Buffer.from(token.split('.')[1] ?? '', 'base64url').toString());
  const now = Math.floor((await serverNow(s.c)) / 1000);
  const res = await s.c.post('/__test/session/backdate', { headers: s.headers, data: { seconds: auth_time - now } });
  expect(res.status()).toBe(200);
  return { authorization: `Bearer ${(await res.json()).access_token}` };
}

