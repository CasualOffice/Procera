import { expect, test } from '@playwright/test';
import { PW, hasDatabase, mfaSession, passwordSession, seedUser, serverNow, uniqueEmail } from '../../fixtures/accounts';
import { totp } from '../../fixtures/totp';
import { inviteToken, sysAdmin, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-SYS-06..09 — system console: workspaces grid, lifecycle with step-up, quotas, health and
// system audit (BPMS-065; FR-S02, S04, S08, S09). System admins see metadata and counts, never content.

const tag = () => `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}`;
/** Keys a console row may carry: lifecycle metadata, quotas and usage counts. Nothing else. */
const ROW_KEYS = ['created_at', 'deletion_due_at', 'id', 'isolation_tier', 'name', 'owner_invite_email', 'profile', 'quotas', 'region', 'revision', 'slug', 'state', 'suspended_reason', 'usage'];

async function systemUser(request: Parameters<typeof seedUser>[0], role: string) {
  const email = await seedUser(request, role.replace('system.', 'sys-'));
  expect((await request.post('/__test/system-roles', { data: { email, role } })).status()).toBe(204);
  return mfaSession(email);
}

test.describe('system console @r1 @system', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-SYS-06 workspaces grid lists metadata, quotas and usage counts with filters', async ({ request }) => {
    const { sys, ws, owner } = await workspaceWithOwner(request);
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: uniqueEmail('invitee'), roles: ['member'] } });
    const t = tag();
    const starter = await (await sys.c.post('/sysapi/v1/workspaces', { headers: sys.headers, data: { name: `Grid ${t}`, slug: `grid-${t}`, profile: 'starter', owner_email: uniqueEmail('o') } })).json();

    const all = await (await sys.c.get('/sysapi/v1/workspaces', { headers: sys.headers })).json();
    const row = all.items.find((w: { id: string }) => w.id === ws.id);
    expect(Object.keys(row).sort()).toEqual(ROW_KEYS);
    expect(row.usage).toMatchObject({ members: 1, invited: 1, storage_bytes: null });
    expect(Date.parse(row.usage.last_activity_at)).not.toBeNaN();
    expect(row.state).toBe('active');

    const byProfile = await (await sys.c.get('/sysapi/v1/workspaces?profile=starter', { headers: sys.headers })).json();
    expect(byProfile.items.every((w: { profile: string }) => w.profile === 'starter')).toBe(true);
    expect(byProfile.items.map((w: { id: string }) => w.id)).toContain(starter.id);
    const byName = await (await sys.c.get(`/sysapi/v1/workspaces?q=${encodeURIComponent(`GRID ${t}`)}`, { headers: sys.headers })).json();
    expect(byName.items.map((w: { id: string }) => w.id)).toEqual([starter.id]);
    const provisioning = await (await sys.c.get(`/sysapi/v1/workspaces?state=provisioning&q=grid-${t}`, { headers: sys.headers })).json();
    expect(provisioning.items).toHaveLength(1);
    expect((await sys.c.get('/sysapi/v1/workspaces?colour=red', { headers: sys.headers })).status()).toBe(400);

    const one = await sys.c.get(`/sysapi/v1/workspaces/${ws.id}`, { headers: sys.headers });
    expect(one.headers()['etag']).toBe(`"${row.revision}"`);
    expect((await one.json()).usage.members).toBe(1);

    // Operators read the grid; workspace owners cannot see the system plane at all.
    const op = await systemUser(request, 'system.operator');
    expect((await op.c.get('/sysapi/v1/workspaces', { headers: op.headers })).status()).toBe(200);
    expect((await owner.c.get('/sysapi/v1/workspaces', { headers: owner.headers })).status()).toBe(404);
  });

  test('E2E-R1-SYS-07 suspend, reactivate and schedule deletion need step-up and land on the system chain', async ({ request }) => {
    const { sys, ws, owner } = await workspaceWithOwner(request);
    const stale = { authorization: `Bearer ${(await (await sys.c.post('/__test/session/backdate', { headers: sys.headers, data: { seconds: 600 } })).json()).access_token}` };
    const denied = await sys.c.post(`/sysapi/v1/workspaces/${ws.id}/suspend`, { headers: stale, data: { mode: 'locked', reason: 'security incident' } });
    expect([denied.status(), (await denied.json()).code]).toEqual([401, 'step_up_required']);
    await request.post('/__test/clock', { data: { advance: 'PT30S' } });
    const up = await sys.c.post('/api/v1/session/step-up', { headers: stale, data: { code: totp(sys.secret, await serverNow(sys.c), 1) } });
    expect(up.status()).toBe(200);
    const fresh = { authorization: `Bearer ${(await up.json()).access_token}` };

    const locked = await sys.c.post(`/sysapi/v1/workspaces/${ws.id}/suspend`, { headers: fresh, data: { mode: 'locked', reason: 'security incident' } });
    expect(locked.status()).toBe(200);
    expect(await locked.json()).toMatchObject({ state: 'suspended_locked', suspended_reason: 'security incident', usage: { members: 1 } });
    const blocked = await owner.c.get('/api/v1/members', { headers: owner.headers });
    expect([blocked.status(), (await blocked.json()).code]).toEqual([423, 'workspace_suspended']);

    const ro = await sys.c.post(`/sysapi/v1/workspaces/${ws.id}/suspend`, { headers: fresh, data: { mode: 'read_only', reason: 'billing review' } });
    expect((await ro.json()).state).toBe('suspended_ro');
    expect((await owner.c.get('/api/v1/members', { headers: owner.headers })).status()).toBe(200);
    expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: uniqueEmail('x'), roles: ['member'] } })).status()).toBe(423);

    const back = await sys.c.post(`/sysapi/v1/workspaces/${ws.id}/restore`, { headers: { ...fresh, 'if-match': ro.headers()['etag']! } });
    expect((await back.json()).state).toBe('active');
    const noReason = await sys.c.post(`/sysapi/v1/workspaces/${ws.id}/schedule-deletion`, { headers: fresh, data: { grace_days: 30, reason: ' ' } });
    expect(noReason.status()).toBe(422);
    const del = await (await sys.c.post(`/sysapi/v1/workspaces/${ws.id}/schedule-deletion`, { headers: fresh, data: { grace_days: 14, reason: 'customer offboarding' } })).json();
    expect(del).toMatchObject({ state: 'pending_deletion', suspended_reason: 'customer offboarding' });

    // The workspace's lifecycle history is on the system chain, oldest last, with reasons.
    const history = await (await sys.c.get(`/sysapi/v1/system-audit/events?object_type=workspace&object_id=${ws.id}`, { headers: sys.headers })).json();
    expect(history.items.map((e: { action: string }) => e.action)).toEqual([
      'workspace.deletion_scheduled', 'workspace.restored', 'workspace.suspended', 'workspace.suspended', 'workspace.created',
    ]);
    expect(history.items[0].diff).toMatchObject({ reason: 'customer offboarding', state: { before: 'active', after: 'pending_deletion' } });
    expect(history.items[0].actor_name).toBe('sysadmin');
  });

  test('E2E-R1-SYS-08 quota defaults per profile and the member quota enforced at invite time', async ({ request }) => {
    const sys = await sysAdmin(request);
    const defaults = await (await sys.c.get('/sysapi/v1/quota-defaults', { headers: sys.headers })).json();
    expect(defaults.items.map((d: { profile: string }) => d.profile)).toEqual(['starter', 'standard', 'enterprise', 'regulated']);
    const regulated = defaults.items[3];
    expect(regulated.max_members).toBeNull();
    const { profile: _p, revision: _r, updated_at: _u, ...limits } = regulated;
    const stale = await sys.c.put('/sysapi/v1/quota-defaults/regulated', { headers: { ...sys.headers, 'if-match': `"${regulated.revision - 1}"` }, data: limits });
    expect(stale.status()).toBe(412);
    const bad = await sys.c.put('/sysapi/v1/quota-defaults/regulated', { headers: sys.headers, data: { ...limits, max_members: 0 } });
    expect(bad.status()).toBe(422);
    const set = await sys.c.put('/sysapi/v1/quota-defaults/regulated', { headers: { ...sys.headers, 'if-match': `"${regulated.revision}"` }, data: limits });
    expect(set.status()).toBe(200);
    expect(set.headers()['etag']).toBe(`"${regulated.revision + 1}"`);
    const op = await systemUser(request, 'system.operator');
    expect((await op.c.put('/sysapi/v1/quota-defaults/regulated', { headers: op.headers, data: limits })).status()).toBe(403);

    // Per-workspace limit: the owner holds one seat; one open invitation fills a limit of 2.
    const { sys: admin, ws, owner } = await workspaceWithOwner(request);
    const quotas = { ...ws.quotas, max_members: 2 };
    const q = await admin.c.put(`/sysapi/v1/workspaces/${ws.id}/quotas`, { headers: admin.headers, data: quotas });
    expect((await q.json()).quotas.max_members).toBe(2);
    const first = uniqueEmail('seat');
    expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: first, roles: ['member'] } })).status()).toBe(201);
    const over = await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: uniqueEmail('over'), roles: ['member'] } });
    expect(over.status()).toBe(429);
    expect(await over.json()).toMatchObject({ code: 'quota_exceeded' });
    expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: first, roles: ['member'] } })).status()).toBe(201);
    let grid = await (await admin.c.get(`/sysapi/v1/workspaces/${ws.id}`, { headers: admin.headers })).json();
    expect(grid.usage).toMatchObject({ members: 1, invited: 1 });
    // The invitee can still join: the open invitation reserved the seat.
    expect((await request.post('/__test/users', { data: { email: first, display_name: 'Seat', password: PW } })).status()).toBe(201);
    const pw = await passwordSession(first);
    const joined = await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token: await inviteToken(request, ws.id, first) } });
    expect(joined.status()).toBe(200);
    grid = await (await admin.c.get(`/sysapi/v1/workspaces/${ws.id}`, { headers: admin.headers })).json();
    expect(grid.usage).toMatchObject({ members: 2, invited: 0 });
  });

  test('E2E-R1-SYS-09 platform health and the system audit explorer; auditors read, never change', async ({ request }) => {
    const sys = await sysAdmin(request);
    const h = await (await sys.c.get('/sysapi/v1/health', { headers: sys.headers })).json();
    expect(['ok', 'degraded']).toContain(h.status);
    expect(h.database.status).toBe('ok');
    expect(h.version).toMatch(/^\d+\.\d+\.\d+/);
    expect(h.migrations.applied).toBe(h.migrations.expected);
    expect(h.migrations.status).toBe('ok');
    expect(h.outbox).toMatchObject({ dispatcher: 'running' });
    expect(typeof h.outbox.pending).toBe('number');
    expect(typeof h.mail.queued).toBe('number');
    expect(h.cache.backend).toBeTruthy();

    const auditor = await systemUser(request, 'system.auditor');
    const page = await (await auditor.c.get('/sysapi/v1/system-audit/events?action=workspace.&page[size]=5', { headers: auditor.headers })).json();
    expect(page.items.length).toBeGreaterThan(0);
    expect(page.items.every((e: { action: string }) => e.action.startsWith('workspace.'))).toBe(true);
    const seqs = page.items.map((e: { seq: number }) => e.seq);
    expect([...seqs].sort((a, b) => b - a)).toEqual(seqs);
    if (page.next_cursor) {
      const older = await (await auditor.c.get(`/sysapi/v1/system-audit/events?action=workspace.&page[size]=5&page[cursor]=${page.next_cursor}`, { headers: auditor.headers })).json();
      expect(older.items[0].seq).toBeLessThan(seqs[seqs.length - 1]);
    }
    // Workspace chains are not reachable from the system plane: invitations live there.
    const invites = await (await auditor.c.get('/sysapi/v1/system-audit/events?action=member.invited', { headers: auditor.headers })).json();
    expect(invites.items).toEqual([]);
    expect((await (await auditor.c.get('/sysapi/v1/system-audit/verify', { headers: auditor.headers })).json()).ok).toBe(true);
    expect((await auditor.c.get('/sysapi/v1/system-audit/events?category=nope', { headers: auditor.headers })).status()).toBe(422);
    expect((await auditor.c.get('/sysapi/v1/health', { headers: auditor.headers })).status()).toBe(200);
    expect((await auditor.c.post('/sysapi/v1/workspaces', { headers: auditor.headers, data: { name: 'No', slug: `no-${tag()}`, profile: 'starter', owner_email: 'n@acme.test' } })).status()).toBe(403);

    // Operators see health and workspaces but not the system audit log.
    const op = await systemUser(request, 'system.operator');
    expect((await op.c.get('/sysapi/v1/system-audit/events', { headers: op.headers })).status()).toBe(403);
  });
});
