import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser, uniqueEmail } from '../../fixtures/accounts';
import { enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-MEM-01..10 — onboarding, members, roles and grants (BPMS-023/024; docs/23).

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
    const { c, headers } = a.owner;
    expect((await c.post('/api/v1/session/workspace', { headers, data: { workspace_id: b.ws.id } })).status()).toBe(404);
    // Leave the workspace (home view): workspace APIs then need a selection.
    const home = await (await c.post('/api/v1/session/workspace', { headers, data: { workspace_id: null } })).json();
    const noWs = await c.get('/api/v1/members', { headers: { authorization: `Bearer ${home.access_token}` } });
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

  test('E2E-R1-MEM-09 roles granted to a group follow group membership; department-scoped admins reach only their subtree', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const join = async (tag: string) => {
      const email = await seedUser(request, tag);
      await owner.c.post('/api/v1/members/invitations', { headers: h, data: { email, roles: ['member'] } });
      const token = await inviteToken(request, ws.id, email);
      const pw = await passwordSession(email);
      const { membership_id } = await (await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } })).json();
      return { email, id: membership_id as string };
    };
    const ann = await join('grp-ann');
    const g = await (await owner.c.post('/api/v1/groups', { headers: h, data: { name: 'Role viewers' } })).json();
    await owner.c.post(`/api/v1/groups/${g.id}/members`, { headers: h, data: { membership_id: ann.id } });
    expect((await owner.c.post('/api/v1/role-grants', { headers: h, data: { group_id: g.id, role_key: 'analyst' } })).status()).toBe(201);
    expect((await owner.c.post('/api/v1/role-grants', { headers: h, data: { group_id: g.id, role_key: 'workspace.owner' } })).status()).toBe(422);
    expect((await owner.c.post('/api/v1/role-grants', { headers: h, data: { role_key: 'analyst' } })).status()).toBe(422);
    const annS = await enter(ann.email, ws.id);
    const perms = async () => (await (await annS.c.get('/api/v1/me/permissions', { headers: annS.headers })).json()).permissions as string[];
    expect(await perms()).toContain('report:view');
    await owner.c.delete(`/api/v1/groups/${g.id}/members/${ann.id}`, { headers: h });
    expect(await perms()).not.toContain('report:view');

    // Department-scoped admin: EMEA admin may deactivate a UK member, not an APAC member.
    const emea = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'EMEA' } })).json();
    const uk = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'UK', parent_id: emea.id } })).json();
    const apac = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'APAC' } })).json();
    const boss = await join('emea-admin');
    const inUk = await join('uk-staff');
    const inApac = await join('apac-staff');
    await owner.c.patch(`/api/v1/members/${inUk.id}`, { headers: h, data: { department_id: uk.id } });
    await owner.c.patch(`/api/v1/members/${inApac.id}`, { headers: h, data: { department_id: apac.id } });
    expect((await owner.c.post('/api/v1/role-grants', { headers: h, data: { membership_id: boss.id, role_key: 'workspace.admin', scope_kind: 'department', scope_id: emea.id } })).status()).toBe(201);
    const bossS = await enter(boss.email, ws.id);
    expect((await bossS.c.post(`/api/v1/members/${inApac.id}/deactivate`, { headers: bossS.headers })).status()).toBe(403);
    expect((await bossS.c.post(`/api/v1/members/${inUk.id}/deactivate`, { headers: bossS.headers })).status()).toBe(204);
  });

  test('E2E-R1-MEM-10 MFA is required by role: an admin without MFA is refused at workspace entry, a member is not', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const join = async (tag: string, role: string) => {
      const email = await seedUser(request, tag);
      await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: [role] } });
      const token = await inviteToken(request, ws.id, email);
      const pw = await passwordSession(email);
      await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });
      const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: { authorization: `Bearer ${pw.token}` }, data: { workspace_id: ws.id } })).json();
      return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` } };
    };
    const admin = await join('nomfa-admin', 'workspace.admin');
    const res = await admin.c.get('/api/v1/members', { headers: admin.headers });
    expect([res.status(), (await res.json()).code]).toEqual([403, 'mfa_required']);
    const plain = await join('nomfa-member', 'member');
    expect((await plain.c.get('/api/v1/members', { headers: plain.headers })).status()).toBe(200);
  });
});
