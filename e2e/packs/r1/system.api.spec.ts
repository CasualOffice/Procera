import { expect, test } from '@playwright/test';
import { PW, client, hasDatabase, mfaSession, passwordSession, seedUser, uniqueEmail } from '../../fixtures/accounts';

// E2E-R1-SYS-01..05 — system plane (BPMS-020; docs/23 E2E-R1-01, -02, -17).

test.describe('system plane @r1 @system', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-SYS-01 bootstrap token is single-use and creates the system owner', async ({ request }) => {
    const issued = await request.post('/__test/bootstrap-token');
    test.skip(issued.status() === 409, 'platform already bootstrapped in this database');
    const { token } = await issued.json();
    const c = await client();
    const email = uniqueEmail('root');
    const bad = await c.post('/sysapi/v1/bootstrap', { data: { token: 'not-the-token', email, display_name: 'Root', password: PW } });
    expect(bad.status()).toBe(401);
    const ok = await c.post('/sysapi/v1/bootstrap', { data: { token, email, display_name: 'Root', password: PW } });
    expect(ok.status()).toBe(201);
    expect((await ok.json()).roles).toEqual(['system.owner']);
    const again = await c.post('/sysapi/v1/bootstrap', { data: { token, email: uniqueEmail('x'), display_name: 'X', password: PW } });
    expect(again.status()).toBe(409);
    expect((await request.post('/__test/bootstrap-token')).status()).toBe(409);
  });

  test('E2E-R1-SYS-02 system API is invisible to non-system users and requires MFA for system users', async ({ request }) => {
    const plain = await passwordSession(await seedUser(request, 'member'));
    expect((await plain.c.get('/sysapi/v1/workspaces', { headers: { authorization: `Bearer ${plain.token}` } })).status()).toBe(404);
    const adminEmail = await seedUser(request, 'sysadmin-nomfa');
    expect((await request.post('/__test/system-roles', { data: { email: adminEmail, role: 'system.admin' } })).status()).toBe(204);
    const noMfa = await passwordSession(adminEmail);
    const res = await noMfa.c.get('/sysapi/v1/workspaces', { headers: { authorization: `Bearer ${noMfa.token}` } });
    expect(res.status()).toBe(403);
    expect((await res.json()).code).toBe('mfa_required');
  });

  test('E2E-R1-SYS-03 create workspaces A (starter) and B (enterprise) with profile quotas', async ({ request }) => {
    const email = await seedUser(request, 'sysadmin');
    await request.post('/__test/system-roles', { data: { email, role: 'system.admin' } });
    const { c, headers } = await mfaSession(email);
    const tag = Date.now().toString(36);
    const a = await c.post('/sysapi/v1/workspaces', { headers, data: { name: 'Acme HR', slug: `acme-hr-${tag}`, profile: 'starter', owner_email: 'owner@acme.test' } });
    expect(a.status()).toBe(201);
    expect(a.headers()['location']).toMatch(/\/sysapi\/v1\/workspaces\/[0-9a-f-]{36}$/);
    expect(a.headers()['etag']).toBe('"1"');
    const wa = await a.json();
    expect(wa.state).toBe('provisioning');
    expect(wa.quotas.max_members).toBe(100);
    const b = await (await c.post('/sysapi/v1/workspaces', { headers, data: { name: 'Acme Finance', slug: `acme-fin-${tag}`, profile: 'enterprise', owner_email: 'cfo@acme.test' } })).json();
    expect(b.quotas.max_members).toBeNull();
    expect((await c.post('/sysapi/v1/workspaces', { headers, data: { name: 'Dup', slug: `acme-hr-${tag}`, profile: 'starter', owner_email: 'o@acme.test' } })).status()).toBe(409);
    const invalid = await c.post('/sysapi/v1/workspaces', { headers, data: { name: 'Bad', slug: 'Bad Slug!', profile: 'starter', owner_email: 'o@acme.test' } });
    expect(invalid.status()).toBe(422);
    const list = await (await c.get('/sysapi/v1/workspaces', { headers })).json();
    const slugs = list.items.map((w: { slug: string }) => w.slug);
    expect(slugs).toEqual(expect.arrayContaining([`acme-hr-${tag}`, `acme-fin-${tag}`]));
  });

  test('E2E-R1-SYS-04 suspend → restore with If-Match, schedule deletion; transitions validated', async ({ request }) => {
    const email = await seedUser(request, 'sysadmin2');
    await request.post('/__test/system-roles', { data: { email, role: 'system.admin' } });
    const { c, headers } = await mfaSession(email);
    const ws = await (await c.post('/sysapi/v1/workspaces', { headers, data: { name: 'Acme Ops', slug: `acme-ops-${Date.now().toString(36)}`, profile: 'standard', owner_email: 'ops@acme.test' } })).json();
    const sus = await c.post(`/sysapi/v1/workspaces/${ws.id}/suspend`, { headers: { ...headers, 'if-match': `"${ws.revision}"` }, data: { mode: 'read_only', reason: 'unpaid invoice' } });
    expect(sus.status()).toBe(200);
    const s = await sus.json();
    expect([s.state, s.suspended_reason]).toEqual(['suspended_ro', 'unpaid invoice']);
    const stale = await c.post(`/sysapi/v1/workspaces/${ws.id}/restore`, { headers: { ...headers, 'if-match': `"${ws.revision}"` } });
    expect(stale.status()).toBe(412);
    const restored = await c.post(`/sysapi/v1/workspaces/${ws.id}/restore`, { headers: { ...headers, 'if-match': sus.headers()['etag'] } });
    expect((await restored.json()).state).toBe('active');
    expect((await c.post(`/sysapi/v1/workspaces/${ws.id}/restore`, { headers })).status()).toBe(409);
    expect((await c.post(`/sysapi/v1/workspaces/${ws.id}/schedule-deletion`, { headers, data: { grace_days: 1, reason: 'customer request' } })).status()).toBe(422);
    const del = await (await c.post(`/sysapi/v1/workspaces/${ws.id}/schedule-deletion`, { headers, data: { grace_days: 30, reason: 'customer request' } })).json();
    expect(del.state).toBe('pending_deletion');
    expect(del.deletion_due_at).toBeTruthy();
  });

  test('E2E-R1-SYS-05 operators can read but not change workspaces; system audit chain verifies', async ({ request }) => {
    const email = await seedUser(request, 'operator');
    await request.post('/__test/system-roles', { data: { email, role: 'system.operator' } });
    const { c, headers } = await mfaSession(email);
    expect((await c.get('/sysapi/v1/workspaces', { headers })).status()).toBe(200);
    const write = await c.post('/sysapi/v1/workspaces', { headers, data: { name: 'Nope', slug: `nope-${Date.now().toString(36)}`, profile: 'starter', owner_email: 'n@acme.test' } });
    expect(write.status()).toBe(403);
    const audit = await (await request.get('/__test/audit/verify')).json();
    expect(audit.ok).toBe(true);
  });
});
