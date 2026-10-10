import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, mfaSession, passwordSession, seedUser, uniqueEmail } from '../../fixtures/accounts';

// E2E-R1-MEM-01..08 — onboarding, members, roles and grants (BPMS-023/024; docs/23).

type Session = { c: APIRequestContext; headers: Record<string, string> };

async function sysAdmin(request: APIRequestContext) {
  const email = await seedUser(request, 'sysadmin');
  await request.post('/__test/system-roles', { data: { email, role: 'system.admin' } });
  return mfaSession(email);
}

async function inviteToken(request: APIRequestContext, workspace: string, email: string) {
  const res = await request.get(`/__test/invitations/token?workspace=${workspace}&email=${encodeURIComponent(email)}`);
  expect(res.status()).toBe(200);
  return (await res.json()).token as string;
}

/** Signs in and switches to `workspace`; returns a session whose bearer is workspace-scoped. */
async function enter(email: string, workspace: string): Promise<Session> {
  const { c, token } = await passwordSession(email);
  const sw = await c.post('/api/v1/session/workspace', { headers: { authorization: `Bearer ${token}` }, data: { workspace_id: workspace } });
  expect(sw.status()).toBe(200);
  return { c, headers: { authorization: `Bearer ${(await sw.json()).access_token}` } };
}

/** System admin creates a workspace; the owner registers via the invitation and signs into it. */
async function workspaceWithOwner(request: APIRequestContext) {
  const sys = await sysAdmin(request);
  const ownerEmail = uniqueEmail('owner');
  const ws = await (await sys.c.post('/sysapi/v1/workspaces', { headers: sys.headers, data: { name: 'Acme', slug: `acme-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}`, profile: 'standard', owner_email: ownerEmail } })).json();
  const token = await inviteToken(request, ws.id, ownerEmail);
  const anon = await client();
  const reg = await anon.post('/api/v1/invitations/register', { data: { token, display_name: 'Olivia', password: PW } });
  expect(reg.status()).toBe(201);
  return { sys, ws, ownerEmail, owner: await enter(ownerEmail, ws.id) };
}

test.describe('members and access @r1 @members', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-MEM-01 owner onboarding activates the workspace; invitation is single-use', async ({ request }) => {
    const { sys, ws, ownerEmail, owner } = await workspaceWithOwner(request);
    expect((await (await sys.c.get(`/sysapi/v1/workspaces/${ws.id}`, { headers: sys.headers })).json()).state).toBe('active');
    const mine = await (await owner.c.get('/api/v1/me/workspaces', { headers: owner.headers })).json();
    expect(mine.active).toBe(ws.id);
    expect(mine.items.map((w: { workspace_id: string }) => w.workspace_id)).toContain(ws.id);
    const perms = await (await owner.c.get('/api/v1/me/permissions', { headers: owner.headers })).json();
    expect(perms.permissions).toEqual(expect.arrayContaining(['member:invite', 'role:manage', 'workspace:delete.request']));
    expect((await request.get(`/__test/invitations/token?workspace=${ws.id}&email=${encodeURIComponent(ownerEmail)}`)).status()).toBe(404);
  });

  test('E2E-R1-MEM-02 invite → accept by the invited email only; token never returned to the inviter', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const adamEmail = await seedUser(request, 'adam');
    const inv = await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: adamEmail, roles: ['workspace.admin'] } });
    expect(inv.status()).toBe(201);
    expect(JSON.stringify(await inv.json())).not.toMatch(/token/);
    const token = await inviteToken(request, ws.id, adamEmail);
    const mallory = await passwordSession(await seedUser(request, 'mallory'));
    expect((await mallory.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${mallory.token}` }, data: { token } })).status()).toBe(403);
    const adam = await passwordSession(adamEmail);
    const ok = await adam.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${adam.token}` }, data: { token } });
    expect(ok.status()).toBe(200);
    expect((await ok.json()).workspace_id).toBe(ws.id);
    const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
    expect(list.items.map((m: { email: string }) => m.email)).toEqual(expect.arrayContaining([adamEmail]));
  });

  test('E2E-R1-MEM-03 workspace isolation: a non-member cannot switch into or read another workspace', async ({ request }) => {
    const a = await workspaceWithOwner(request);
    const b = await workspaceWithOwner(request);
    const { c, token } = await passwordSession(a.ownerEmail);
    expect((await c.post('/api/v1/session/workspace', { headers: { authorization: `Bearer ${token}` }, data: { workspace_id: b.ws.id } })).status()).toBe(404);
    const noWs = await c.get('/api/v1/members', { headers: { authorization: `Bearer ${token}` } });
    expect(noWs.status()).toBe(400);
    expect((await noWs.json()).code).toBe('no_workspace');
  });

  test('E2E-R1-MEM-04 custom role: no escalation, built-ins immutable, grant takes effect immediately', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const catalog = await (await owner.c.get('/api/v1/permission-catalog', { headers: owner.headers })).json();
    expect(catalog.templates.map((t: { key: string }) => t.key)).toEqual(expect.arrayContaining(['workspace.owner', 'member', 'analyst']));
    const roles = await (await owner.c.get('/api/v1/roles', { headers: owner.headers })).json();
    const builtin = roles.items.find((r: { key: string }) => r.key === 'member');
    expect((await owner.c.patch(`/api/v1/roles/${builtin.id}`, { headers: owner.headers, data: { permissions: ['member:view'] } })).status()).toBe(409);
    const created = await owner.c.post('/api/v1/roles', { headers: owner.headers, data: { key: 'people.viewer', name: 'People viewer', permissions: ['member:view', 'role:view'] } });
    expect(created.status()).toBe(201);

    // Bob joins as a plain member (who can see the directory) and cannot view roles until granted the custom role.
    const bobEmail = await seedUser(request, 'bob');
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: bobEmail, roles: ['member'] } });
    const bobToken = await inviteToken(request, ws.id, bobEmail);
    const bobPw = await passwordSession(bobEmail);
    const accepted = await (await bobPw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${bobPw.token}` }, data: { token: bobToken } })).json();
    const bob = await enter(bobEmail, ws.id);
    expect((await bob.c.get('/api/v1/members', { headers: bob.headers })).status()).toBe(200);
    const denied = await bob.c.get('/api/v1/roles', { headers: bob.headers });
    expect(denied.status()).toBe(403);
    expect((await denied.json()).detail).toBe('role:view');
    // Bob cannot create a role carrying permissions he lacks.
    expect((await bob.c.post('/api/v1/roles', { headers: bob.headers, data: { key: 'sneaky', name: 'S', permissions: ['role:manage'] } })).status()).toBe(403);

    const grant = await owner.c.post('/api/v1/role-grants', { headers: owner.headers, data: { membership_id: accepted.membership_id, role_key: 'people.viewer', reason: 'HR audit' } });
    expect(grant.status()).toBe(201);
    expect((await bob.c.get('/api/v1/roles', { headers: bob.headers })).status()).toBe(200);
    const { id } = await grant.json();
    expect((await owner.c.delete(`/api/v1/role-grants/${id}`, { headers: owner.headers })).status()).toBe(204);
    expect((await bob.c.get('/api/v1/roles', { headers: bob.headers })).status()).toBe(403);
  });

  test('E2E-R1-MEM-05 deactivation guards and immediate loss of access', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const adminEmail = await seedUser(request, 'admin');
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: adminEmail, roles: ['workspace.admin'] } });
    const token = await inviteToken(request, ws.id, adminEmail);
    const pw = await passwordSession(adminEmail);
    const { membership_id } = await (await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } })).json();
    const admin = await enter(adminEmail, ws.id);
    expect((await admin.c.post('/api/v1/members/invitations', { headers: admin.headers, data: { email: uniqueEmail('eve'), roles: ['workspace.owner'] } })).status()).toBe(403);
    const ownerId = (await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json()).items.find((m: { roles: string[] }) => m.roles.includes('workspace.owner')).id;
    const last = await admin.c.post(`/api/v1/members/${ownerId}/deactivate`, { headers: admin.headers });
    expect([403, 409]).toContain(last.status());
    expect((await admin.c.post(`/api/v1/members/${membership_id}/deactivate`, { headers: admin.headers })).status()).toBe(409);
    expect((await owner.c.post(`/api/v1/members/${membership_id}/deactivate`, { headers: owner.headers })).status()).toBe(204);
    expect((await admin.c.get('/api/v1/roles', { headers: admin.headers })).status()).toBe(404);
  });

  test('E2E-R1-MEM-06 suspended workspace: read-only blocks writes (423), restore re-enables them', async ({ request }) => {
    const { sys, ws, owner } = await workspaceWithOwner(request);
    const cur = await (await sys.c.get(`/sysapi/v1/workspaces/${ws.id}`, { headers: sys.headers })).json();
    expect((await sys.c.post(`/sysapi/v1/workspaces/${ws.id}/suspend`, { headers: { ...sys.headers, 'if-match': `"${cur.revision}"` }, data: { mode: 'read_only', reason: 'billing' } })).status()).toBe(200);
    expect((await owner.c.get('/api/v1/members', { headers: owner.headers })).status()).toBe(200);
    const write = await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: uniqueEmail('x'), roles: ['member'] } });
    expect(write.status()).toBe(423);
    expect((await sys.c.post(`/sysapi/v1/workspaces/${ws.id}/restore`, { headers: sys.headers })).status()).toBe(200);
    expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: uniqueEmail('y'), roles: ['member'] } })).status()).toBe(201);
  });

  test('E2E-R1-MEM-07 pending invitations: list, revoke kills the token, resend issues a new one', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const email = await seedUser(request, 'carol');
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
    const first = await inviteToken(request, ws.id, email);
    const pending = await (await owner.c.get('/api/v1/members/invitations', { headers: owner.headers })).json();
    const inv = pending.items.find((i: { email: string }) => i.email === email);
    expect(inv.role_keys).toEqual(['member']);
    expect((await owner.c.delete(`/api/v1/members/invitations/${inv.id}`, { headers: owner.headers })).status()).toBe(204);
    expect((await owner.c.delete(`/api/v1/members/invitations/${inv.id}`, { headers: owner.headers })).status()).toBe(404);
    const carol = await passwordSession(email);
    const auth = { authorization: `Bearer ${carol.token}` };
    expect((await carol.c.post('/api/v1/invitations/accept', { headers: auth, data: { token: first } })).status()).toBe(404);
    // Resend = invite again: a fresh token works.
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
    const second = await inviteToken(request, ws.id, email);
    expect(second).not.toBe(first);
    expect((await carol.c.post('/api/v1/invitations/accept', { headers: auth, data: { token: second } })).status()).toBe(200);
  });

  test('E2E-R1-MEM-08 admins cannot strip the owner; last owner is kept; role preview; access changes are audited', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const adminEmail = await seedUser(request, 'admin2');
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: adminEmail, roles: ['workspace.admin'] } });
    const token = await inviteToken(request, ws.id, adminEmail);
    const pw = await passwordSession(adminEmail);
    await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });
    const admin = await enter(adminEmail, ws.id);
    const grants = await (await owner.c.get('/api/v1/role-grants', { headers: owner.headers })).json();
    const ownerGrant = grants.items.find((g: { role_key: string }) => g.role_key === 'workspace.owner');
    const strip = await admin.c.delete(`/api/v1/role-grants/${ownerGrant.id}`, { headers: admin.headers });
    expect(strip.status()).toBe(403);
    expect((await strip.json()).code).toBe('escalation');
    const self = await owner.c.delete(`/api/v1/role-grants/${ownerGrant.id}`, { headers: owner.headers });
    expect(self.status()).toBe(409);
    expect((await self.json()).code).toBe('last_owner');

    const role = await (await owner.c.post('/api/v1/roles', { headers: owner.headers, data: { key: 'hr.viewer', name: 'HR viewer', permissions: ['member:view'] } })).json();
    const members = (await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json()).items;
    const adminId = members.find((m: { email: string }) => m.email === adminEmail).id;
    expect((await owner.c.post('/api/v1/role-grants', { headers: owner.headers, data: { membership_id: adminId, role_key: 'hr.viewer' } })).status()).toBe(201);
    const preview = await (await owner.c.post(`/api/v1/roles/${role.id}/preview`, { headers: owner.headers, data: { permissions: ['role:view'] } })).json();
    expect(preview).toEqual({ added: ['role:view'], removed: ['member:view'], affected_members: [adminId] });
    expect((await owner.c.patch(`/api/v1/roles/${role.id}`, { headers: owner.headers, data: { permissions: ['role:view'] } })).status()).toBe(204);

    const chain = await (await request.get(`/__test/audit/verify?workspace=${ws.id}`)).json();
    expect(chain.ok).toBe(true);
    expect(chain.checked).toBeGreaterThanOrEqual(6); // owner invite + join, admin invite + join, role created, granted, updated
  });
});
