import { type APIRequestContext, type APIResponse, expect, request as pwRequest, test } from '@playwright/test';
import { BASE, PW, hasDatabase, serverNow, uniqueEmail } from '../../fixtures/accounts';
import { softAuthenticator } from '../../fixtures/passkey';
import { totp } from '../../fixtures/totp';
import { freshAuth } from '../../fixtures/workspace';
import { pdfPages, upload } from '../../fixtures/files';
import { oidcProvider } from '../../fixtures/oidc';

// E2E-R1-AUD-01 — audit coverage contract (FR-T01, BPMS-026). Calls every mutating route once and checks
// the audit events it appended against the route table served by /__test/audit/contract. Events are
// attributed by a unique User-Agent, so tests running in parallel cannot interfere.

const CSRF = { 'x-requested-with': 'procera' };
type Entry = { method: string; path: string; audit: { kind: 'emits'; alternatives: string[][] } | { kind: 'exempt'; reason: string } };
type Ev = { chain: string; seq: number; action: string };

test('E2E-R1-AUD-01 every mutating route appends exactly its contracted audit events @r1 @audit', async ({ request }) => {
  test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  test.slow(); // walks every mutating route once (about 25 s alone; longer beside the parallel packs)
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

  // 2b. Passkeys (BPMS-022): enrol, sign in with password + passkey, step-up options, removal.
  const pkEmail = await seed('passkey');
  const pkc = await ctx();
  const key = await softAuthenticator(request);
  const pkPw = await (await pkc.post('/api/v1/auth/login', { data: { email: pkEmail, password: PW } })).json();
  const regOpts = await (await step('POST', '/api/v1/me/mfa/webauthn/options', () => pkc.post('/api/v1/me/mfa/webauthn/options', { headers: bearer(pkPw.access_token) }))).json();
  const reg = await (await step('POST', '/api/v1/me/mfa/webauthn', () => pkc.post('/api/v1/me/mfa/webauthn', { headers: bearer(pkPw.access_token), data: { name: 'Audit key', credential: key.register(regOpts) } }))).json();
  const pkStep1 = await (await pkc.post('/api/v1/auth/login', { data: { email: pkEmail, password: PW } })).json();
  const mfaOpts = await (await step('POST', '/api/v1/auth/mfa/webauthn/options', () => pkc.post('/api/v1/auth/mfa/webauthn/options', { data: { mfa_token: pkStep1.mfa_token } }))).json();
  const pkLogin = await (await pkc.post('/api/v1/auth/mfa', { data: { mfa_token: pkStep1.mfa_token, credential: key.assert(mfaOpts) } })).json();
  await step('POST', '/api/v1/session/step-up/webauthn/options', () => pkc.post('/api/v1/session/step-up/webauthn/options', { headers: bearer(pkLogin.access_token) }));
  await step('DELETE', '/api/v1/me/mfa/webauthn/{id}', () => pkc.delete(`/api/v1/me/mfa/webauthn/${reg.passkey.id}`, { headers: bearer(pkLogin.access_token) }));

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
  await step('POST', '/sysapi/v1/workspaces/{id}/owner-invitation/link', () => sys.post(`/sysapi/v1/workspaces/${ws.id}/owner-invitation/link`, { headers: sh }));

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
  await step('POST', '/api/v1/members/invitations/{id}/link', () => own.post(`/api/v1/members/invitations/${inv.id}/link`, { headers: oh }));
  await step('DELETE', '/api/v1/members/invitations/{id}', () => own.delete(`/api/v1/members/invitations/${inv.id}`, { headers: oh }));
  // Directory (adam is still active here).
  const dept = await (await step('POST', '/api/v1/departments', () => own.post('/api/v1/departments', { headers: oh, data: { name: 'Audit Dept' } }))).json();
  await step('PATCH', '/api/v1/departments/{id}', () => own.patch(`/api/v1/departments/${dept.id}`, { headers: oh, data: { name: 'Audit Department' } }));
  await step('PUT', '/api/v1/settings/departments/{id}/{key}', () => own.put(`/api/v1/settings/departments/${dept.id}/locale.timezone`, { headers: oh, data: { value: 'Europe/Berlin' } }));
  await step('DELETE', '/api/v1/settings/departments/{id}/{key}', () => own.delete(`/api/v1/settings/departments/${dept.id}/locale.timezone`, { headers: oh }));
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

  // Requests and approvals (BPMS-103): Adam asks for leave, the owner (his manager) approves; a second
  // request is cancelled.
  const ownerMembership = ((await (await own.get('/api/v1/members', { headers: oh })).json()).items as { id: string }[]).find((m) => m.id !== joined.membership_id)!.id;
  await own.patch(`/api/v1/members/${joined.membership_id}`, { headers: oh, data: { manager_membership_id: ownerMembership } });
  const adamReqWs = await (await adam.post('/api/v1/session/workspace', { headers: bearer(adamLogin.access_token), data: { workspace_id: ws.id } })).json();
  const rh = bearer(adamReqWs.access_token);
  const leave = { type: 'hr.leave-request', input: { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23' } };
  await step('POST', '/api/v1/requests/preview', () => adam.post('/api/v1/requests/preview', { headers: rh, data: leave }));
  await step('POST', '/api/v1/requests', () => adam.post('/api/v1/requests', { headers: { ...rh, 'idempotency-key': `aud-${ua}-1` }, data: leave }));
  const [item] = (await (await own.get('/api/v1/work?state=open', { headers: oh })).json()).items;
  // Conversation (BPMS-214): Adam comments on his request, then watches and unwatches it.
  const [mine] = (await (await adam.get('/api/v1/requests?scope=mine', { headers: rh })).json()).items as { id: string }[];
  await step('POST', '/api/v1/requests/{id}/comments', () => adam.post(`/api/v1/requests/${mine.id}/comments`, { headers: rh, data: { body: 'audit drill' } }));
  await step('PUT', '/api/v1/requests/{id}/watch', () => adam.put(`/api/v1/requests/${mine.id}/watch`, { headers: rh }));
  await step('DELETE', '/api/v1/requests/{id}/watch', () => adam.delete(`/api/v1/requests/${mine.id}/watch`, { headers: rh }));
  await step('POST', '/api/v1/work/{id}/decisions', () => own.post(`/api/v1/work/${item.id}/decisions`, { headers: { ...oh, 'if-match': `"${item.revision}"` }, data: { outcome: 'approved' } }));
  const second = await (await adam.post('/api/v1/requests', { headers: { ...rh, 'idempotency-key': `aud-${ua}-2` }, data: leave })).json();
  // Admin oversight (BPMS-088): the owner reassigns the open step (to themselves; Adam is excluded by SoD).
  const [open] = ((await (await own.get('/api/v1/work?state=open', { headers: oh })).json()).items as { id: string; request: { id: string } }[]).filter((w) => w.request.id === second.id);
  await step('POST', '/api/v1/work/{id}/reassign', () => own.post(`/api/v1/work/${open.id}/reassign`, { headers: oh, data: { membership_id: ownerMembership, reason: 'audit drill' } }));
  // Ops (PR-09): the owner retries one technical incident and dismisses another, each with a reason.
  const plant = async () => (await (await request.post('/__test/incidents', { data: { workspace: ws.id, request: second.id } })).json()).id as string;
  const [inc1, inc2] = [await plant(), await plant()];
  await step('POST', '/api/v1/operations/incidents/{source}/{id}/retry', () => own.post(`/api/v1/operations/incidents/job/${inc1}/retry`, { headers: oh, data: { reason: 'audit drill' } }));
  await step('POST', '/api/v1/operations/incidents/{source}/{id}/dismiss', () => own.post(`/api/v1/operations/incidents/job/${inc2}/dismiss`, { headers: oh, data: { reason: 'audit drill' } }));
  await step('POST', '/api/v1/requests/{id}/cancel', () => adam.post(`/api/v1/requests/${second.id}/cancel`, { headers: rh, data: { reason: 'plans changed' } }));
  // Out of office (BPMS-086): Adam sets and clears his own; the owner sets and clears Adam's.
  const serverDay = await serverNow(own); // the shared e2e clock may be ahead of the wall clock
  const ooo = { starts_on: new Date(serverDay).toISOString().slice(0, 10), ends_on: new Date(serverDay + 7 * 86_400_000).toISOString().slice(0, 10), delegate: ownerMembership };
  await step('PUT', '/api/v1/me/out-of-office', () => adam.put('/api/v1/me/out-of-office', { headers: rh, data: ooo }));
  await step('DELETE', '/api/v1/me/out-of-office', () => adam.delete('/api/v1/me/out-of-office', { headers: rh }));
  await step('PUT', '/api/v1/members/{id}/out-of-office', () => own.put(`/api/v1/members/${joined.membership_id}/out-of-office`, { headers: oh, data: ooo }));
  await step('DELETE', '/api/v1/members/{id}/out-of-office', () => own.delete(`/api/v1/members/${joined.membership_id}/out-of-office`, { headers: oh }));

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
  // Collections, owner, RACI (BPMS-041).
  const col = await (await step('POST', '/api/v1/collections', () => own.post('/api/v1/collections', { headers: oh, data: { name: 'Audit collection' } }))).json();
  await step('PATCH', '/api/v1/collections/{id}', () => own.patch(`/api/v1/collections/${col.id}`, { headers: oh, data: { name: 'Audit collection 2' } }));
  await step('PUT', '/api/v1/collections/{id}/visibility', () => own.put(`/api/v1/collections/${col.id}/visibility`, { headers: oh, data: { visibility: null } }));
  await step('PUT', '/api/v1/artifacts/{id}/collection', () => own.put(`/api/v1/artifacts/${sop.artifact.id}/collection`, { headers: oh, data: { collection_id: col.id } }));
  await own.put(`/api/v1/artifacts/${sop.artifact.id}/collection`, { headers: oh, data: { collection_id: null } });
  await step('DELETE', '/api/v1/collections/{id}', () => own.delete(`/api/v1/collections/${col.id}`, { headers: oh }));
  await step('PUT', '/api/v1/artifacts/{id}/owner', () => own.put(`/api/v1/artifacts/${sop.artifact.id}/owner`, { headers: oh, data: { membership_id: joined.membership_id } }));
  await step('PUT', '/api/v1/artifacts/{id}/raci', () => own.put(`/api/v1/artifacts/${sop.artifact.id}/raci`, { headers: oh, data: { accountable: [{ membership_id: joined.membership_id }] } }));
  await step('POST', '/api/v1/security/encryption/rotate', () => own.post('/api/v1/security/encryption/rotate', { headers: oh, data: { reason: 'drill' } }));
  await step('POST', '/api/v1/audit/query', () => own.post('/api/v1/audit/query', { headers: oh, data: { filter: { field: 'category', op: 'eq', value: 'content' }, page: { size: 1 } } }));
  // Template packs (BPMS-106).
  await step('POST', '/api/v1/templates/{key}/install', () => own.post('/api/v1/templates/core-sops/install', { headers: oh, data: { documents: ['reporting-an-incident'] } }));
  // Saved views and exports (BPMS-028).
  const vw = await (await step('POST', '/api/v1/views', () => own.post('/api/v1/views', { headers: oh, data: { list: 'library.documents', name: 'Audit view', state: { q: '' } } }))).json();
  await step('PATCH', '/api/v1/views/{id}', () => own.patch(`/api/v1/views/${vw.id}`, { headers: oh, data: { name: 'Audit view 2' } }));
  await step('DELETE', '/api/v1/views/{id}', () => own.delete(`/api/v1/views/${vw.id}`, { headers: oh }));
  await step('POST', '/api/v1/exports', () => own.post('/api/v1/exports', { headers: oh, data: { list: 'library.documents', format: 'csv', rows: 1, columns: ['title'] } }));
  // Files (BPMS-060/062).
  const fc = await (await step('POST', '/api/v1/files', () => own.post('/api/v1/files', { headers: oh, data: { name: 'a.txt', size: 5 } }))).json();
  await step('PUT', '/api/v1/files/{id}/content', () => own.put(`/api/v1/files/${fc.file.id}/content?offset=0`, { headers: { ...oh, 'content-type': 'application/octet-stream' }, data: Buffer.from('hello') }));
  await step('POST', '/api/v1/files/{id}/complete', () => own.post(`/api/v1/files/${fc.file.id}/complete`, { headers: oh }));
  const draftV = await (await own.post('/api/v1/artifacts', { headers: oh, data: { kind: 'sop', key: 'audit.files', title: 'Files SOP', content: { blocks: [] } } })).json();
  await step('POST', '/api/v1/files/{id}/links', () => own.post(`/api/v1/files/${fc.file.id}/links`, { headers: oh, data: { object_type: 'artifact_version', object_id: draftV.version.id } }));
  await step('DELETE', '/api/v1/files/{id}/links/{object_type}/{object_id}', () => own.delete(`/api/v1/files/${fc.file.id}/links/artifact_version/${draftV.version.id}`, { headers: oh }));
  const ret = await step('POST', '/api/v1/files/retention', () => own.post('/api/v1/files/retention', { headers: oh, data: { file_ids: [fc.file.id], class: 'operational', reason: 'Scratch upload' } }));
  expect(ret.status(), await ret.text()).toBe(200);
  await step('PATCH', '/api/v1/workspace', () => own.patch('/api/v1/workspace', { headers: oh, data: { name: 'Audit Company' } }));
  await step('PATCH', '/api/v1/me/profile', () => own.patch('/api/v1/me/profile', { headers: oh, data: { display_name: 'Audit Owner', timezone: 'Asia/Kolkata' } }));
  // Account recovery and admin controls (docs/06 §3.6.1): Rita belongs to this workspace only.
  const ritaEmail = await seed('rita');
  await own.post('/api/v1/members/invitations', { headers: oh, data: { email: ritaEmail, roles: ['member'] } });
  const rita = await ctx();
  const ritaPw = await (await rita.post('/api/v1/auth/login', { data: { email: ritaEmail, password: PW } })).json();
  await rita.post('/api/v1/session/step-up', { headers: bearer(ritaPw.access_token), data: { password: PW } });
  const rj = await (await rita.post('/api/v1/invitations/accept', { headers: bearer(ritaPw.access_token), data: { token: await tokenFor(ritaEmail) } })).json();
  // Delegation (BPMS-212): Adam asks again; the owner hands the step to Rita.
  const third = await (await adam.post('/api/v1/requests', { headers: { ...rh, 'idempotency-key': `aud-${ua}-3` }, data: leave })).json();
  const [held] = ((await (await own.get('/api/v1/work?state=open', { headers: oh })).json()).items as { id: string; request: { id: string } }[]).filter((w) => w.request.id === third.id);
  await step('POST', '/api/v1/work/{id}/delegate', () => own.post(`/api/v1/work/${held.id}/delegate`, { headers: oh, data: { membership_id: rj.membership_id, note: 'audit drill' } }));
  // Email approvals (BPMS-145): Adam asks again; the owner approves from the link in the email.
  const fourth = await (await adam.post('/api/v1/requests', { headers: { ...rh, 'idempotency-key': `aud-${ua}-4` }, data: leave })).json();
  await request.post('/__test/drain');
  const mails = (await (await request.get(`/__test/mail?workspace=${ws.id}&to=${encodeURIComponent(ownerEmail)}`)).json()).items as { kind: string; text: string }[];
  const eaToken = mails.find((m) => m.kind === 'work_assigned' && m.text.includes(fourth.display_id))!.text.match(/\/approve\/([A-Za-z0-9_-]+)\?/)![1]!;
  await step('POST', '/api/v1/email-actions/{token}', () => request.post(`/api/v1/email-actions/${eaToken}`, { headers: { 'user-agent': ua }, data: { outcome: 'approved' } }));
  // Processes (BPMS-100): create from a template, save the draft, publish.
  const proc = await (await step('POST', '/api/v1/processes', () => own.post('/api/v1/processes', { headers: oh, data: { key: 'aud.process', title: 'Audit process', template: 'hr.leave-request' } }))).json();
  const pd = await (await own.get(`/api/v1/processes/${proc.id}`, { headers: oh })).json();
  await step('PUT', '/api/v1/processes/{id}/draft', () => own.put(`/api/v1/processes/${proc.id}/draft`, { headers: { ...oh, 'if-match': `"${pd.draft.revision}"` }, data: { content: pd.draft.content } }));
  await step('POST', '/api/v1/processes/validate', () => own.post('/api/v1/processes/validate', { headers: oh, data: { content: pd.draft.content } }));
  await step('POST', '/api/v1/processes/test-run', () => own.post('/api/v1/processes/test-run', { headers: oh, data: { content: pd.draft.content, input: { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23' } } }));
  await step('POST', '/api/v1/processes/who', () => own.post('/api/v1/processes/who', { headers: oh, data: { candidates: [{ type: 'role', ref: 'workspace.owner' }] } }));
  // E-signing (BPMS-107): a draft to the owner, send, sign (completes it); then a decline and a void.
  const pdf = await upload({ c: own, headers: oh }, 'audit.pdf', pdfPages(['Audit drill']));
  const envelope = async (sign: boolean) => {
    const e = sign
      ? await (await step('POST', '/api/v1/envelopes', () => own.post('/api/v1/envelopes', { headers: oh, data: { title: 'Audit drill', file_id: pdf.id } }))).json()
      : await (await own.post('/api/v1/envelopes', { headers: oh, data: { title: 'Audit drill', file_id: pdf.id } })).json();
    const call = (m: string, p: string, f: () => Promise<APIResponse>) => (sign ? step(m, p, f) : f());
    const r = await (await call('PUT', '/api/v1/envelopes/{id}/recipients', () => own.put(`/api/v1/envelopes/${e.id}/recipients`, { headers: oh, data: { recipients: [{ membership_id: ownerMembership }] } }))).json();
    await call('PUT', '/api/v1/envelopes/{id}/fields', () => own.put(`/api/v1/envelopes/${e.id}/fields`, { headers: oh, data: { fields: [{ recipient_id: r.recipients[0].id, kind: 'signature', page: 1, x: 0.1, y: 0.1, w: 0.3, h: 0.08 }] } }));
    await call('POST', '/api/v1/envelopes/{id}/send', () => own.post(`/api/v1/envelopes/${e.id}/send`, { headers: oh }));
    return { id: e.id as string, recipient: r.recipients[0].id as string, hash: e.document_hash as string };
  };
  const e1 = await envelope(true);
  await step('POST', '/api/v1/signing/{recipient}/sign', () => own.post(`/api/v1/signing/${e1.recipient}/sign`, { headers: oh, data: { document_hash: e1.hash, consent: true, adopted: { kind: 'typed', text: 'Audit Owner' } } }));
  const e2 = await envelope(false);
  await step('POST', '/api/v1/signing/{recipient}/decline', () => own.post(`/api/v1/signing/${e2.recipient}/decline`, { headers: oh, data: { reason: 'drill' } }));
  const e3 = await envelope(false);
  await step('POST', '/api/v1/envelopes/{id}/void', () => own.post(`/api/v1/envelopes/${e3.id}/void`, { headers: oh, data: { reason: 'drill' } }));
  // Bulk send, reminders (none due yet) and cancelling a batch (BPMS-107 slice 3).
  const bulk = await (await own.post('/api/v1/envelopes', { headers: oh, data: { title: 'Audit bulk', file_id: pdf.id, bulk: true } })).json();
  await own.put(`/api/v1/envelopes/${bulk.id}/fields`, { headers: oh, data: { fields: [{ recipient_id: bulk.recipients[0].id, kind: 'signature', page: 1, x: 0.1, y: 0.1, w: 0.3, h: 0.08 }] } });
  const batch = await (await step('POST', '/api/v1/envelopes/{id}/send-bulk', () => own.post(`/api/v1/envelopes/${bulk.id}/send-bulk`, { headers: oh, data: { audience: { all: true } } }))).json();
  await step('POST', '/api/v1/envelope-batches/{id}/remind', () => own.post(`/api/v1/envelope-batches/${batch.id}/remind`, { headers: oh }));
  await step('POST', '/api/v1/envelopes/{id}/remind', () => own.post(`/api/v1/envelopes/${batch.rows[0].envelope_id}/remind`, { headers: oh }));
  await step('POST', '/api/v1/envelope-batches/{id}/void', () => own.post(`/api/v1/envelope-batches/${batch.id}/void`, { headers: oh, data: { reason: 'drill' } }));
  // Webhooks (BPMS-143): create (to a public name; nothing is delivered here), change, test, replay, roll, delete.
  const hook = await (await step('POST', '/api/v1/webhooks', () => own.post('/api/v1/webhooks', { headers: oh, data: { url: 'https://hooks.example.com/procera', events: ['*'] } }))).json();
  await step('PATCH', '/api/v1/webhooks/{id}', () => own.patch(`/api/v1/webhooks/${hook.id}`, { headers: { ...oh, 'if-match': `"${hook.revision}"` }, data: { description: 'Audit drill' } }));
  const tested = await (await step('POST', '/api/v1/webhooks/{id}/test', () => own.post(`/api/v1/webhooks/${hook.id}/test`, { headers: oh }))).json();
  await step('POST', '/api/v1/webhooks/deliveries/{id}/replay', () => own.post(`/api/v1/webhooks/deliveries/${tested.id}/replay`, { headers: oh }));
  await step('POST', '/api/v1/webhooks/{id}/rotate-secret', () => own.post(`/api/v1/webhooks/${hook.id}/rotate-secret`, { headers: oh }));
  await step('DELETE', '/api/v1/webhooks/{id}', () => own.delete(`/api/v1/webhooks/${hook.id}`, { headers: oh }));
  // Single sign-on (BPMS-140): set the provider (its discovery document answers), then remove it.
  const idp = await oidcProvider({ clientId: 'procera', clientSecret: 'audit' });
  await step('PUT', '/api/v1/sso', () => own.put('/api/v1/sso', { headers: oh, data: { issuer: idp.issuer, client_id: 'procera', client_secret: 'audit', domains: ['audit-sso.test'] } }));
  await step('POST', '/api/v1/auth/sso/discover', () => own.post('/api/v1/auth/sso/discover', { headers: oh, data: { email: 'someone@audit-sso.test' } }));
  await step('DELETE', '/api/v1/sso', () => own.delete('/api/v1/sso', { headers: oh }));
  await idp.close();
  // HR locations and holidays (BPMS-222): a calendar with holidays, a location on it, a member's HR record.
  const hcal = await (await step('POST', '/api/v1/holiday-calendars', () => own.post('/api/v1/holiday-calendars', { headers: oh, data: { name: 'Audit holidays' } }))).json();
  await step('PATCH', '/api/v1/holiday-calendars/{id}', () => own.patch(`/api/v1/holiday-calendars/${hcal.id}`, { headers: oh, data: { name: 'Audit holidays 2' } }));
  await step('PUT', '/api/v1/holiday-calendars/{id}/holidays/{date}', () => own.put(`/api/v1/holiday-calendars/${hcal.id}/holidays/2030-01-01`, { headers: oh, data: { name: 'New Year' } }));
  await step('DELETE', '/api/v1/holiday-calendars/{id}/holidays/{date}', () => own.delete(`/api/v1/holiday-calendars/${hcal.id}/holidays/2030-01-01`, { headers: oh }));
  await step('POST', '/api/v1/holiday-calendars/{id}/import', () => own.post(`/api/v1/holiday-calendars/${hcal.id}/import`, { headers: oh, data: { text: '2030-12-25,Christmas' } }));
  await step('POST', '/api/v1/holiday-calendars/{id}/copy-year', () => own.post(`/api/v1/holiday-calendars/${hcal.id}/copy-year`, { headers: oh, data: { from: 2030, to: 2031 } }));
  const wloc = await (await step('POST', '/api/v1/work-locations', () => own.post('/api/v1/work-locations', { headers: oh, data: { name: 'Audit office', timezone: 'Europe/London', holiday_calendar_id: hcal.id } }))).json();
  await step('PATCH', '/api/v1/work-locations/{id}', () => own.patch(`/api/v1/work-locations/${wloc.id}`, { headers: oh, data: { name: 'Audit office', timezone: 'Europe/London', work_week: [1, 2, 3, 4], holiday_calendar_id: null } }));
  const myMembership = (await (await own.get('/api/v1/me/permissions', { headers: oh })).json()).membership_id as string;
  await step('PUT', '/api/v1/members/{id}/hr', () => own.put(`/api/v1/members/${myMembership}/hr`, { headers: oh, data: { employee_no: 'AUD-1', work_location_id: wloc.id } }));
  // Leave types, policies and balances (BPMS-223).
  const ltypes = await (await step('POST', '/api/v1/leave-types/defaults', () => own.post('/api/v1/leave-types/defaults', { headers: oh }))).json();
  const ltype = await (await step('POST', '/api/v1/leave-types', () => own.post('/api/v1/leave-types', { headers: oh, data: { name: 'Audit leave' } }))).json();
  await step('PATCH', '/api/v1/leave-types/{id}', () => own.patch(`/api/v1/leave-types/${ltype.id}`, { headers: oh, data: { name: 'Audit leave 2', sensitive: true } }));
  const lpol = await (await step('POST', '/api/v1/leave-policies', () => own.post('/api/v1/leave-policies', { headers: oh, data: { name: 'Audit policy', audience: { all: true, departments: [], groups: [], roles: [] }, rules: [{ leave_type_id: ltype.id, yearly_days: 2 }] } }))).json();
  await step('PATCH', '/api/v1/leave-policies/{id}', () => own.patch(`/api/v1/leave-policies/${lpol.id}`, { headers: oh, data: { name: 'Audit policy', audience: { all: true, departments: [], groups: [], roles: [] }, rules: [{ leave_type_id: ltype.id, yearly_days: 3 }] } }));
  await step('PUT', '/api/v1/members/{id}/leave-policy', () => own.put(`/api/v1/members/${myMembership}/leave-policy`, { headers: oh, data: { policy_id: ltypes.policy.id } }));
  await step('POST', '/api/v1/members/{id}/time-off/adjustments', () => own.post(`/api/v1/members/${myMembership}/time-off/adjustments`, { headers: oh, data: { leave_type_id: ltypes.types[0].id, days: 1, reason: 'Audit drill' } }));
  await step('DELETE', '/api/v1/holiday-calendars/{id}', () => own.delete(`/api/v1/holiday-calendars/${hcal.id}`, { headers: oh }));
  // API tokens (BPMS-144): create, revoke as owner, revoke as admin (a repeat is a no-op).
  const tok = await (await step('POST', '/api/v1/me/api-tokens', () => own.post('/api/v1/me/api-tokens', { headers: oh, data: { name: 'Audit token', permissions: ['request:view_own'] } }))).json();
  await step('DELETE', '/api/v1/me/api-tokens/{id}', () => own.delete(`/api/v1/me/api-tokens/${tok.id}`, { headers: oh }));
  const tok2 = await (await own.post('/api/v1/me/api-tokens', { headers: oh, data: { name: 'Audit token 2', permissions: ['request:view_own'] } })).json();
  await step('DELETE', '/api/v1/api-tokens/{id}', () => own.delete(`/api/v1/api-tokens/${tok2.id}`, { headers: oh }));
  await step('POST', '/api/v1/processes/{id}/publish', () => own.post(`/api/v1/processes/${proc.id}/publish`, { headers: oh }));
  const [pw1, pw2, pw3] = ['Audit drill one 2026!', 'Audit drill two 2026!', 'Audit drill three 2026!'];
  await step('POST', '/api/v1/me/password', () => rita.post('/api/v1/me/password', { headers: bearer(ritaPw.access_token), data: { current_password: PW, new_password: pw1 } }));
  await step('POST', '/api/v1/auth/password/forgot', () => rita.post('/api/v1/auth/password/forgot', { data: { email: ritaEmail } }));
  let resetToken = '';
  await expect.poll(async () => {
    const mail = JSON.stringify(await (await request.get(`/__test/mail?to=${encodeURIComponent(ritaEmail)}`)).json());
    resetToken = [...mail.matchAll(/reset-password\?token=([A-Za-z0-9_-]+)/g)].at(-1)?.[1] ?? '';
    return resetToken;
  }).not.toBe('');
  await step('POST', '/api/v1/auth/password/reset', () => rita.post('/api/v1/auth/password/reset', { data: { token: resetToken, password: pw2 } }));
  const ownUp = await freshAuth({ c: own, headers: oh });
  const sec = (action: string) => step('POST', `/api/v1/members/{id}/security/${action}`, () => own.post(`/api/v1/members/${rj.membership_id}/security/${action}`, { headers: ownUp, data: { reason: 'audit drill' } }));
  await sec('require-password-change');
  const forced = await (await step('POST', '/api/v1/auth/login', () => rita.post('/api/v1/auth/login', { data: { email: ritaEmail, password: pw2 } }))).json();
  expect(forced.status).toBe('password_change_required');
  await step('POST', '/api/v1/auth/password/expired', () => rita.post('/api/v1/auth/password/expired', { data: { change_token: forced.change_token, password: pw3 } }));
  await sec('reset-link');
  await sec('sign-out-everywhere');
  await sec('reset-mfa');
  await step('POST', '/api/v1/files/orphans/purge', () => own.post('/api/v1/files/orphans/purge', { headers: oh }));
  await step('DELETE', '/api/v1/files/{id}', () => own.delete(`/api/v1/files/${fc.file.id}`, { headers: oh }));
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
  const sop2 = await (await step('POST', '/api/v1/artifacts/{id}/versions', () => own.post(`/api/v1/artifacts/${sop.artifact.id}/versions`, { headers: oh }))).json();
  // A major v2 asks everyone to re-acknowledge (BPMS-045): the publish call also creates and launches the campaign.
  await own.post(`/api/v1/versions/${sop2.id}/submit`, { headers: oh });
  await step('POST', '/api/v1/versions/{id}/publish', () => own.post(`/api/v1/versions/${sop2.id}/publish`, { headers: oh, data: { change: 'major', change_reason: 'drill' } }));
  expect(observed.get('POST /api/v1/versions/{id}/publish')).toEqual(['campaign.activated', 'campaign.created', 'version.published']);

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
  // Reactivation restores access, so it needs a recent verification (docs/22 A-41); then Adam leaves again.
  const reFresh = await freshAuth({ c: own, headers: oh });
  await step('POST', '/api/v1/members/{id}/reactivate', () => own.post(`/api/v1/members/${joined.membership_id}/reactivate`, { headers: reFresh, data: { reason: 'audit drill' } }));
  await own.post(`/api/v1/members/${joined.membership_id}/deactivate`, { headers: oh });

  // Notifications (exempt: personal read state). Adam's join put a notification in the owner's inbox.
  await request.post('/__test/drain');
  const inbox = await (await own.get('/api/v1/notifications', { headers: oh })).json();
  if (inbox.items.length) {
    await step('POST', '/api/v1/notifications/{id}/read', () => own.post(`/api/v1/notifications/${inbox.items[0].id}/read`, { headers: oh }));
  }
  await step('POST', '/api/v1/notifications/read-all', () => own.post('/api/v1/notifications/read-all', { headers: oh }));

  // Settings: a workspace value, and a platform floor re-set to its current value (no effect on other tests).
  await step('PUT', '/api/v1/settings/{key}', () => own.put('/api/v1/settings/locale.week_start', { headers: oh, data: { value: 'sun' } }));
  const switchPlan = await own.get('/api/v1/settings/profile/plan?profile=enterprise', { headers: oh });
  const fresh = await freshAuth({ c: own, headers: oh });
  await step('POST', '/api/v1/settings/profile/apply', () => own.post('/api/v1/settings/profile/apply', { headers: { ...fresh, 'if-match': switchPlan.headers()['etag']! }, data: { profile: 'enterprise', reason: 'audit drill' } }));
  const floors = await (await sys.get('/sysapi/v1/settings', { headers: sh })).json();
  const maxSize = floors.items.find((f: { key: string }) => f.key === 'files.max_size').value;
  const sysFresh = await freshAuth({ c: sys, headers: sh });
  await step('PUT', '/sysapi/v1/settings/{key}', () => sys.put('/sysapi/v1/settings/files.max_size', { headers: sysFresh, data: { value: maxSize, reason: 'audit drill (unchanged)' } }));
  const starter = (await (await sys.get('/sysapi/v1/quota-defaults', { headers: sh })).json()).items[0];
  const { profile: _qp, revision: _qr, updated_at: _qu, ...starterLimits } = starter;
  await step('PUT', '/sysapi/v1/quota-defaults/{profile}', () => sys.put(`/sysapi/v1/quota-defaults/${starter.profile}`, { headers: sh, data: starterLimits }));
  // Email settings (BPMS-065): saved pointing at a closed local port. E2E builds deliver only through
  // PROCERA_SMTP_URL, so the shared queue is untouched; the test send fails, answers 200 with the error
  // and is audited all the same.
  const mail = await sys.get('/sysapi/v1/email', { headers: sh });
  await step('PUT', '/sysapi/v1/email', () => sys.put('/sysapi/v1/email', { headers: { ...sh, 'if-match': mail.headers()['etag']! }, data: { host: 'localhost', port: 1, tls_mode: 'none', from_address: 'no-reply@procera.test', from_name: 'Procera', reason: 'audit drill' } }));
  const probe = await (await step('POST', '/sysapi/v1/email/test', () => sys.post('/sysapi/v1/email/test', { headers: sh, data: { to: 'audit@procera.test' } }))).json();
  expect(probe).toMatchObject({ sent: false, error: { kind: 'connection_refused' } });

  // 6. Sessions: refresh (exempt), logout, sign-out-everywhere; then schedule deletion.
  await step('POST', '/api/v1/auth/refresh', () => own.post('/api/v1/auth/refresh', { headers: CSRF }));
  await step('POST', '/api/v1/auth/logout', () => own.post('/api/v1/auth/logout', { headers: CSRF }));
  await step('POST', '/api/v1/me/sign-out-everywhere', () => adam.post('/api/v1/me/sign-out-everywhere', { headers: bearer(adamLogin.access_token) }));
  await step('POST', '/sysapi/v1/workspaces/{id}/schedule-deletion', () => sys.post(`/sysapi/v1/workspaces/${ws.id}/schedule-deletion`, { headers: sh, data: { grace_days: 30, reason: 'customer request' } }));

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
