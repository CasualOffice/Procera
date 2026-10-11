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
  await login(own, ownerEmail);
  // Owners must use MFA (auth.mfa.required_roles): enrol and sign in again (not contract steps).
  const ownPw = await (await own.post('/api/v1/auth/login', { data: { email: ownerEmail, password: PW } })).json();
  const ownEnr = await (await own.post('/api/v1/me/mfa/totp', { headers: bearer(ownPw.access_token) })).json();
  const ownNow = await serverNow(own);
  await own.post('/api/v1/me/mfa/totp/confirm', { headers: bearer(ownPw.access_token), data: { code: totp(ownEnr.secret, ownNow) } });
  const ownStep1 = await (await own.post('/api/v1/auth/login', { data: { email: ownerEmail, password: PW } })).json();
  const ownLogin = await (await own.post('/api/v1/auth/mfa', { data: { mfa_token: ownStep1.mfa_token, code: totp(ownEnr.secret, ownNow, 1) } })).json();
  const sw = await (await step('POST', '/api/v1/session/workspace', () => own.post('/api/v1/session/workspace', { headers: bearer(ownLogin.access_token), data: { workspace_id: ws.id } }))).json();
  const oh = bearer(sw.access_token);

  // 5. Members, roles and grants.
  const adamEmail = await seed('adam');
  await step('POST', '/api/v1/members/invitations', () => own.post('/api/v1/members/invitations', { headers: oh, data: { email: adamEmail, roles: ['member'] } }));
  const adam = await ctx();
  const adamLogin = await login(adam, adamEmail);
  await step('POST', '/api/v1/session/step-up', () => adam.post('/api/v1/session/step-up', { headers: bearer(adamLogin.access_token), data: { password: PW } }));
  const joined = await (await step('POST', '/api/v1/invitations/accept', async () => adam.post('/api/v1/invitations/accept', { headers: bearer(adamLogin.access_token), data: { token: await tokenFor(adamEmail) } }))).json();
  const role = await (await step('POST', '/api/v1/roles', () => own.post('/api/v1/roles', { headers: oh, data: { key: 'audit.viewer', name: 'Audit viewer', permissions: ['role:view'] } }))).json();
  const grant = await (await step('POST', '/api/v1/role-grants', () => own.post('/api/v1/role-grants', { headers: oh, data: { membership_id: joined.membership_id, role_key: 'audit.viewer', reason: 'drill' } }))).json();
  await step('POST', '/api/v1/roles/{id}/preview', () => own.post(`/api/v1/roles/${role.id}/preview`, { headers: oh, data: { permissions: ['role:view', 'member:view'] } }));
  await step('PATCH', '/api/v1/roles/{id}', () => own.patch(`/api/v1/roles/${role.id}`, { headers: oh, data: { permissions: ['role:view', 'member:view'] } }));
  await step('DELETE', '/api/v1/role-grants/{id}', () => own.delete(`/api/v1/role-grants/${grant.id}`, { headers: oh }));
  const carol = uniqueEmail('carol');
  const inv = await (await own.post('/api/v1/members/invitations', { headers: oh, data: { email: carol, roles: ['member'] } })).json();
  await step('DELETE', '/api/v1/members/invitations/{id}', () => own.delete(`/api/v1/members/invitations/${inv.id}`, { headers: oh }));
  // Directory (adam is still active here).
  const dept = await (await step('POST', '/api/v1/departments', () => own.post('/api/v1/departments', { headers: oh, data: { name: 'Audit Dept' } }))).json();
  await step('PATCH', '/api/v1/departments/{id}', () => own.patch(`/api/v1/departments/${dept.id}`, { headers: oh, data: { name: 'Audit Department' } }));
  await step('PATCH', '/api/v1/members/{id}', () => own.patch(`/api/v1/members/${joined.membership_id}`, { headers: oh, data: { title: 'Auditor' } }));
  const grp = await (await step('POST', '/api/v1/groups', () => own.post('/api/v1/groups', { headers: oh, data: { name: 'Audit Group' } }))).json();
  await step('POST', '/api/v1/groups/{id}/members', () => own.post(`/api/v1/groups/${grp.id}/members`, { headers: oh, data: { membership_id: joined.membership_id } }));
  await step('DELETE', '/api/v1/groups/{id}/members/{membership_id}', () => own.delete(`/api/v1/groups/${grp.id}/members/${joined.membership_id}`, { headers: oh }));
  await step('DELETE', '/api/v1/groups/{id}', () => own.delete(`/api/v1/groups/${grp.id}`, { headers: oh }));
  await own.put('/api/v1/settings/directory.positions.enabled', { headers: oh, data: { value: true } });
  const pos = await (await step('POST', '/api/v1/positions', () => own.post('/api/v1/positions', { headers: oh, data: { title: 'Audit lead' } }))).json();
  await step('DELETE', '/api/v1/positions/{id}', () => own.delete(`/api/v1/positions/${pos.id}`, { headers: oh }));
  await step('POST', '/api/v1/members/import', () => own.post('/api/v1/members/import?mode=apply', { headers: { ...oh, 'content-type': 'text/csv' }, data: `email,title\n${adamEmail},Reviewer\n` }));
  await step('DELETE', '/api/v1/departments/{id}', () => own.delete(`/api/v1/departments/${dept.id}`, { headers: oh }));

  // Library: Adam reviews as a reviewer (granted outside the contract steps).
  await own.post('/api/v1/role-grants', { headers: oh, data: { membership_id: joined.membership_id, role_key: 'reviewer' } });
  const adamWs = await (await adam.post('/api/v1/session/workspace', { headers: bearer(adamLogin.access_token), data: { workspace_id: ws.id } })).json();
  const ah = bearer(adamWs.access_token);
  const sop = await (await step('POST', '/api/v1/artifacts', () => own.post('/api/v1/artifacts', { headers: oh, data: { kind: 'sop', key: 'audit.sop', title: 'Audit SOP', content: { blocks: [{ type: 'paragraph', text: 'v1' }] } } }))).json();
  await step('PATCH', '/api/v1/versions/{id}', () => own.patch(`/api/v1/versions/${sop.version.id}`, { headers: oh, data: { content: { blocks: [{ type: 'paragraph', text: 'v1.1' }] } } }));
  await step('POST', '/api/v1/versions/{id}/submit', () => own.post(`/api/v1/versions/${sop.version.id}/submit`, { headers: oh }));
  await step('POST', '/api/v1/versions/{id}/publish', () => own.post(`/api/v1/versions/${sop.version.id}/publish`, { headers: oh, data: {} }));
  // Library governance (BPMS-044): cadence, recertification, verification, change requests.
  await step('PUT', '/api/v1/artifacts/{id}/review-cadence', () => own.put(`/api/v1/artifacts/${sop.artifact.id}/review-cadence`, { headers: oh, data: { days: 90 } }));
  await step('POST', '/api/v1/artifacts/{id}/recertify', () => own.post(`/api/v1/artifacts/${sop.artifact.id}/recertify`, { headers: oh, data: { comment: 'still right' } }));
  await step('POST', '/api/v1/artifacts/{id}/verification', () => own.post(`/api/v1/artifacts/${sop.artifact.id}/verification`, { headers: oh }));
  await step('DELETE', '/api/v1/artifacts/{id}/verification', () => own.delete(`/api/v1/artifacts/${sop.artifact.id}/verification`, { headers: oh }));
  await step('PUT', '/api/v1/me/notification-preferences', () => own.put('/api/v1/me/notification-preferences', { headers: oh, data: { digest: 'daily' } }));
  await step('PUT', '/api/v1/artifacts/{id}/applicability', () => own.put(`/api/v1/artifacts/${sop.artifact.id}/applicability`, { headers: oh, data: { category: 'Audit' } }));
  const cr = await (await step('POST', '/api/v1/artifacts/{id}/change-requests', () => own.post(`/api/v1/artifacts/${sop.artifact.id}/change-requests`, { headers: oh, data: { title: 'Add a step' } }))).json();
  await step('POST', '/api/v1/change-requests/{id}/resolve', () => own.post(`/api/v1/change-requests/${cr.id}/resolve`, { headers: oh, data: { decision: 'accepted' } }));
  // Campaigns: everyone acknowledges the published SOP; Adam (member) completes his assignment.
  const cmp = await (await step('POST', '/api/v1/campaigns', () => own.post('/api/v1/campaigns', { headers: oh, data: { version_id: sop.version.id, audience: { all: true } } }))).json();
  await step('POST', '/api/v1/campaigns/{id}/activate', () => own.post(`/api/v1/campaigns/${cmp.id}/activate`, { headers: oh }));
  await step('POST', '/api/v1/campaigns/{id}/remind', () => own.post(`/api/v1/campaigns/${cmp.id}/remind`, { headers: oh }));
  const adamAcks = await (await adam.get('/api/v1/me/acknowledgements?state=pending', { headers: ah })).json();
  await step('POST', '/api/v1/acknowledgements/{id}/complete', () => adam.post(`/api/v1/acknowledgements/${adamAcks.items[0].id}/complete`, { headers: ah, data: {} }));
  await step('POST', '/api/v1/campaigns/{id}/close', () => own.post(`/api/v1/campaigns/${cmp.id}/close`, { headers: oh, data: {} }));
  const cmp2 = await (await own.post('/api/v1/campaigns', { headers: oh, data: { version_id: sop.version.id, audience: { all: true } } })).json();
  await step('POST', '/api/v1/campaigns/{id}/cancel', () => own.post(`/api/v1/campaigns/${cmp2.id}/cancel`, { headers: oh, data: { reason: 'drill' } }));
  const pol = await (await own.post('/api/v1/artifacts', { headers: oh, data: { kind: 'policy', key: 'audit.policy', title: 'Audit policy', criticality: 'controlled', content: { blocks: [{ type: 'paragraph', text: 'p' }] } } })).json();
  await own.post(`/api/v1/versions/${pol.version.id}/submit`, { headers: oh });
  await step('POST', '/api/v1/versions/{id}/withdraw', () => own.post(`/api/v1/versions/${pol.version.id}/withdraw`, { headers: oh }));
  await own.post(`/api/v1/versions/${pol.version.id}/submit`, { headers: oh });
  await step('POST', '/api/v1/versions/{id}/reviews', () => adam.post(`/api/v1/versions/${pol.version.id}/reviews`, { headers: ah, data: { decision: 'approve' } }));
  await step('POST', '/api/v1/versions/{id}/retire', () => own.post(`/api/v1/versions/${sop.version.id}/retire`, { headers: oh, data: { reason: 'drill' } }));
  await step('POST', '/api/v1/artifacts/{id}/versions', () => own.post(`/api/v1/artifacts/${sop.artifact.id}/versions`, { headers: oh }));

  // Announcements: targeted at members (no approval needed, so the owner may approve their own submission); Adam receives it.
  const annBody = { title: 'Audit notice', audience: { roles: ['member'] }, require_ack: true };
  const ann = await (await step('POST', '/api/v1/announcements', () => own.post('/api/v1/announcements', { headers: oh, data: annBody }))).json();
  await step('PATCH', '/api/v1/announcements/{id}', () => own.patch(`/api/v1/announcements/${ann.id}`, { headers: oh, data: { ...annBody, title: 'Audit notice (edited)' } }));
  await step('POST', '/api/v1/announcements/{id}/submit', () => own.post(`/api/v1/announcements/${ann.id}/submit`, { headers: oh }));
  await step('POST', '/api/v1/announcements/{id}/reject', () => own.post(`/api/v1/announcements/${ann.id}/reject`, { headers: oh, data: { comment: 'drill' } }));
  await own.post(`/api/v1/announcements/${ann.id}/submit`, { headers: oh });
  await step('POST', '/api/v1/announcements/{id}/approve', () => own.post(`/api/v1/announcements/${ann.id}/approve`, { headers: oh }));
  await step('POST', '/api/v1/announcements/{id}/reads', () => adam.post(`/api/v1/announcements/${ann.id}/reads`, { headers: ah }));
  await step('POST', '/api/v1/announcements/{id}/acks', () => adam.post(`/api/v1/announcements/${ann.id}/acks`, { headers: ah }));
  const ann2 = await (await own.post('/api/v1/announcements', { headers: oh, data: { title: 'Audit notice 2', audience: { roles: ['member'] } } })).json();
  await step('POST', '/api/v1/announcements/{id}/publish', () => own.post(`/api/v1/announcements/${ann2.id}/publish`, { headers: oh }));
  await step('POST', '/api/v1/announcements/{id}/archive', () => own.post(`/api/v1/announcements/${ann2.id}/archive`, { headers: oh, data: { reason: 'drill' } }));
  // Audit explorer: saved queries, and an export (step-up: the owner's MFA sign-in above is recent).
  const sq = await (await step('POST', '/api/v1/audit/saved-queries', () => own.post('/api/v1/audit/saved-queries', { headers: oh, data: { name: 'Library changes', filters: { action: 'version.' } } }))).json();
  await step('DELETE', '/api/v1/audit/saved-queries/{id}', () => own.delete(`/api/v1/audit/saved-queries/${sq.id}`, { headers: oh }));
  await step('POST', '/api/v1/audit/exports', () => own.post('/api/v1/audit/exports', { headers: oh, data: { format: 'jsonl', filters: { category: 'content' } } }));

  await step('POST', '/api/v1/members/{id}/deactivate', () => own.post(`/api/v1/members/${joined.membership_id}/deactivate`, { headers: oh }));

  // Notifications (exempt: personal read state). Adam's join put a notification in the owner's inbox.
  await request.post('/__test/drain');
  const inbox = await (await own.get('/api/v1/notifications', { headers: oh })).json();
  if (inbox.items.length) {
    await step('POST', '/api/v1/notifications/{id}/read', () => own.post(`/api/v1/notifications/${inbox.items[0].id}/read`, { headers: oh }));
  }
  await step('POST', '/api/v1/notifications/read-all', () => own.post('/api/v1/notifications/read-all', { headers: oh }));

  // Settings: a workspace value, and a platform floor re-set to its current value (no effect on other tests).
  await step('PUT', '/api/v1/settings/{key}', () => own.put('/api/v1/settings/locale.week_start', { headers: oh, data: { value: 'sun' } }));
  const floors = await (await sys.get('/sysapi/v1/settings', { headers: sh })).json();
  const maxSize = floors.items.find((f: { key: string }) => f.key === 'files.max_size').value;
  await step('PUT', '/sysapi/v1/settings/{key}', () => sys.put('/sysapi/v1/settings/files.max_size', { headers: sh, data: { value: maxSize, reason: 'audit drill (unchanged)' } }));

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
