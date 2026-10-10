import { type APIRequestContext, type APIResponse, expect, request as pwRequest, test } from '@playwright/test';
import { BASE, PW, hasDatabase, serverNow, uniqueEmail } from '../../fixtures/accounts';
import { totp } from '../../fixtures/totp';

// E2E-R1-AUD-01 — audit coverage contract (FR-T01, BPMS-026). Calls every mutating route once and checks
// the audit events it appended against the route table served by /__test/audit/contract. Events are
// attributed by a unique User-Agent, so tests running in parallel cannot interfere.

const CSRF = { 'x-requested-with': 'procera' };
type Entry = { method: string; path: string; audit: { kind: 'emits'; alternatives: string[][] } | { kind: 'exempt'; reason: string } };
type Ev = { chain: string; seq: number; action: string };

test('E2E-R1-AUD-01 every mutating route appends exactly its contracted audit events @r1 @audit', async ({ request }) => {
  test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  const ua = `procera-audit-contract/${Date.now()}-${Math.random().toString(36).slice(2)}`;
  const ctx = () => pwRequest.newContext({ baseURL: BASE, extraHTTPHeaders: { accept: 'application/json', 'user-agent': ua } });
  const observed = new Map<string, string[]>();
  let wsId: string | undefined;

  const events = async (): Promise<Ev[]> => {
    const q = new URLSearchParams({ user_agent: ua, ...(wsId ? { workspace: wsId } : {}) });
    return (await (await request.get(`/__test/audit/events?${q}`)).json()).items;
  };
  /** Runs one call to `method path` (template), records the audit actions it appended. */
  const step = async (method: string, path: string, call: () => Promise<APIResponse>) => {
    const before = new Set((await events()).map((e) => `${e.chain}:${e.seq}`));
    const res = await call();
    expect(res.ok(), `${method} ${path} → ${res.status()} ${await res.text()}`).toBe(true);
    const fresh = (await events()).filter((e) => !before.has(`${e.chain}:${e.seq}`)).map((e) => e.action).sort();
    observed.set(`${method} ${path}`, fresh);
    return res;
  };
  const bearer = (t: string) => ({ authorization: `Bearer ${t}` });
  const seed = async (tag: string) => {
    const email = uniqueEmail(tag);
    expect((await request.post('/__test/users', { data: { email, display_name: tag, password: PW } })).status()).toBe(201);
    return email;
  };
  const login = async (c: APIRequestContext, email: string) =>
    (await (await step('POST', '/api/v1/auth/login', () => c.post('/api/v1/auth/login', { data: { email, password: PW } }))).json());

  // Bootstrap is one-shot per database and owned by E2E-R1-SYS-01; its events are checked by the
  // platform integration test (bootstrap_is_single_use_and_creates_the_owner).

  // 2. System admin: password login, TOTP enrolment, MFA login.
  const sysEmail = await seed('sys');
  await request.post('/__test/system-roles', { data: { email: sysEmail, role: 'system.admin' } });
  const sys = await ctx();
  const pwOnly = await login(sys, sysEmail);
  const enr = await (await step('POST', '/api/v1/me/mfa/totp', () => sys.post('/api/v1/me/mfa/totp', { headers: bearer(pwOnly.access_token) }))).json();
  const now = await serverNow(sys);
  await step('POST', '/api/v1/me/mfa/totp/confirm', () => sys.post('/api/v1/me/mfa/totp/confirm', { headers: bearer(pwOnly.access_token), data: { code: totp(enr.secret, now) } }));
  const step1 = await login(sys, sysEmail);
  expect(step1.status).toBe('mfa_required');
  const mfa = await (await step('POST', '/api/v1/auth/mfa', () => sys.post('/api/v1/auth/mfa', { data: { mfa_token: step1.mfa_token, code: totp(enr.secret, now, 1) } }))).json();
  const sh = bearer(mfa.access_token);

  // 3. Workspace lifecycle on the system plane.
  const ownerEmail = uniqueEmail('owner');
  const ws = await (await step('POST', '/sysapi/v1/workspaces', async () => {
    const r = await sys.post('/sysapi/v1/workspaces', { headers: sh, data: { name: 'Audit Co', slug: `audit-${Date.now().toString(36)}`, profile: 'standard', owner_email: ownerEmail } });
    if (r.ok()) wsId = (await r.json()).id;
    return r;
  })).json();
  const renamed = await (await step('PATCH', '/sysapi/v1/workspaces/{id}', () => sys.patch(`/sysapi/v1/workspaces/${ws.id}`, { headers: sh, data: { name: 'Audit Company' } }))).json();
  await step('PUT', '/sysapi/v1/workspaces/{id}/quotas', () => sys.put(`/sysapi/v1/workspaces/${ws.id}/quotas`, { headers: sh, data: { ...renamed.quotas, max_members: 500 } }));
  await step('POST', '/sysapi/v1/workspaces/{id}/suspend', () => sys.post(`/sysapi/v1/workspaces/${ws.id}/suspend`, { headers: sh, data: { mode: 'read_only', reason: 'audit drill' } }));
  await step('POST', '/sysapi/v1/workspaces/{id}/restore', () => sys.post(`/sysapi/v1/workspaces/${ws.id}/restore`, { headers: sh }));

  // 4. Owner onboarding and workspace selection.
  const tokenFor = async (email: string) => (await (await request.get(`/__test/invitations/token?workspace=${ws.id}&email=${encodeURIComponent(email)}`)).json()).token as string;
  const anon = await ctx();
  await step('POST', '/api/v1/invitations/register', async () => anon.post('/api/v1/invitations/register', { data: { token: await tokenFor(ownerEmail), display_name: 'Owner', password: PW } }));
  const own = await ctx();
  const ownLogin = await login(own, ownerEmail);
  const sw = await (await step('POST', '/api/v1/session/workspace', () => own.post('/api/v1/session/workspace', { headers: bearer(ownLogin.access_token), data: { workspace_id: ws.id } }))).json();
  const oh = bearer(sw.access_token);

  // 5. Members, roles and grants.
  const adamEmail = await seed('adam');
  await step('POST', '/api/v1/members/invitations', () => own.post('/api/v1/members/invitations', { headers: oh, data: { email: adamEmail, roles: ['member'] } }));
  const adam = await ctx();
  const adamLogin = await login(adam, adamEmail);
  const joined = await (await step('POST', '/api/v1/invitations/accept', async () => adam.post('/api/v1/invitations/accept', { headers: bearer(adamLogin.access_token), data: { token: await tokenFor(adamEmail) } }))).json();
  const role = await (await step('POST', '/api/v1/roles', () => own.post('/api/v1/roles', { headers: oh, data: { key: 'audit.viewer', name: 'Audit viewer', permissions: ['role:view'] } }))).json();
  const grant = await (await step('POST', '/api/v1/role-grants', () => own.post('/api/v1/role-grants', { headers: oh, data: { membership_id: joined.membership_id, role_key: 'audit.viewer', reason: 'drill' } }))).json();
  await step('POST', '/api/v1/roles/{id}/preview', () => own.post(`/api/v1/roles/${role.id}/preview`, { headers: oh, data: { permissions: ['role:view', 'member:view'] } }));
  await step('PATCH', '/api/v1/roles/{id}', () => own.patch(`/api/v1/roles/${role.id}`, { headers: oh, data: { permissions: ['role:view', 'member:view'] } }));
  await step('DELETE', '/api/v1/role-grants/{id}', () => own.delete(`/api/v1/role-grants/${grant.id}`, { headers: oh }));
  const carol = uniqueEmail('carol');
  const inv = await (await own.post('/api/v1/members/invitations', { headers: oh, data: { email: carol, roles: ['member'] } })).json();
  await step('DELETE', '/api/v1/members/invitations/{id}', () => own.delete(`/api/v1/members/invitations/${inv.id}`, { headers: oh }));
  await step('POST', '/api/v1/members/{id}/deactivate', () => own.post(`/api/v1/members/${joined.membership_id}/deactivate`, { headers: oh }));

  // 6. Sessions: refresh (exempt), logout, sign-out-everywhere; then schedule deletion.
  await step('POST', '/api/v1/auth/refresh', () => own.post('/api/v1/auth/refresh', { headers: CSRF }));
  await step('POST', '/api/v1/auth/logout', () => own.post('/api/v1/auth/logout', { headers: CSRF }));
  await step('POST', '/api/v1/me/sign-out-everywhere', () => adam.post('/api/v1/me/sign-out-everywhere', { headers: bearer(adamLogin.access_token) }));
  await step('POST', '/sysapi/v1/workspaces/{id}/schedule-deletion', () => sys.post(`/sysapi/v1/workspaces/${ws.id}/schedule-deletion`, { headers: sh, data: { grace_days: 30 } }));

  // 7. Compare with the contract: every route covered, every call appended exactly its contracted events.
  const contract: Entry[] = await (await request.get('/__test/audit/contract')).json();
  const oneShot = ['POST /sysapi/v1/bootstrap'];
  const problems: string[] = [];
  for (const e of contract) {
    const key = `${e.method} ${e.path}`;
    const got = observed.get(key);
    if (!got) {
      if (!oneShot.includes(key)) problems.push(`${key}: not exercised by this pack`);
      continue;
    }
    if (e.audit.kind === 'exempt') {
      if (got.length) problems.push(`${key}: exempt but appended ${got.join(', ')}`);
    } else if (!e.audit.alternatives.some((alt) => JSON.stringify(alt) === JSON.stringify(got))) {
      problems.push(`${key}: appended [${got.join(', ')}], contract allows ${JSON.stringify(e.audit.alternatives)}`);
    }
  }
  for (const key of observed.keys()) if (!contract.some((e) => `${e.method} ${e.path}` === key)) problems.push(`${key}: missing from the contract`);
  expect(problems).toEqual([]);
  const chain = await (await request.get(`/__test/audit/verify?workspace=${ws.id}`)).json();
  expect(chain.ok).toBe(true);
});
