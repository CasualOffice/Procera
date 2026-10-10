import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-DIR-01..04 — directory: departments, managers, groups, positions, CSV import (BPMS-023; FR-W02).

async function join(request: Parameters<typeof seedUser>[0], owner: Session, ws: string, tag: string) {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const { membership_id } = await (await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } })).json();
  return { email, id: membership_id as string };
}

test.describe('directory @r1 @directory', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-DIR-01 department tree: sibling names unique, no moving under a descendant, non-empty delete refused', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const ops = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'Operations' } })).json();
    const sup = await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'Support', parent_id: ops.id } });
    expect(sup.status()).toBe(201);
    const support = await sup.json();
    expect(support.path).toEqual([ops.id, support.id]);
    expect((await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'support', parent_id: ops.id } })).status()).toBe(409);
    const cycle = await owner.c.patch(`/api/v1/departments/${ops.id}`, { headers: h, data: { parent_id: support.id } });
    expect(cycle.status()).toBe(422);
    expect((await cycle.json()).code).toBe('cycle');
    expect((await owner.c.delete(`/api/v1/departments/${ops.id}`, { headers: h })).status()).toBe(409);
    expect((await owner.c.delete(`/api/v1/departments/${support.id}`, { headers: h })).status()).toBe(204);
  });

  test('E2E-R1-DIR-02 managers: cycles rejected; resolution is explicit, then nearest department head', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const ann = await join(request, owner, ws.id, 'ann');
    const ben = await join(request, owner, ws.id, 'ben');
    const dev = await join(request, owner, ws.id, 'dev');
    expect((await owner.c.patch(`/api/v1/members/${ben.id}`, { headers: h, data: { manager_membership_id: ann.id } })).status()).toBe(200);
    const cycle = await owner.c.patch(`/api/v1/members/${ann.id}`, { headers: h, data: { manager_membership_id: ben.id } });
    expect([cycle.status(), (await cycle.json()).code]).toEqual([422, 'cycle']);
    const ops = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'Ops' } })).json();
    const sup = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'Support', parent_id: ops.id } })).json();
    await owner.c.patch(`/api/v1/members/${dev.id}`, { headers: h, data: { department_id: sup.id, title: 'Agent' } });
    await owner.c.patch(`/api/v1/departments/${sup.id}`, { headers: h, data: { head_membership_id: dev.id } });
    await owner.c.patch(`/api/v1/departments/${ops.id}`, { headers: h, data: { head_membership_id: ann.id } });
    const dir = await (await owner.c.get(`/api/v1/members/${dev.id}/directory`, { headers: h })).json();
    expect(dir.resolved_manager).toEqual({ membership_id: ann.id, via: 'department_head', department_id: ops.id });
    expect(dir.field_sources).toEqual({ department: 'manual', title: 'manual' });
    const explicit = await (await owner.c.get(`/api/v1/members/${ben.id}/directory`, { headers: h })).json();
    expect(explicit.resolved_manager.via).toBe('explicit');
  });

  test('E2E-R1-DIR-03 groups and positions; positions follow the workspace setting; members can read but not write', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const cat = await join(request, owner, ws.id, 'cat');
    const g = await (await owner.c.post('/api/v1/groups', { headers: h, data: { name: 'Fire wardens' } })).json();
    expect((await owner.c.post(`/api/v1/groups/${g.id}/members`, { headers: h, data: { membership_id: cat.id } })).status()).toBe(204);
    expect((await owner.c.post(`/api/v1/groups/${g.id}/members`, { headers: h, data: { membership_id: cat.id } })).status()).toBe(409);
    expect((await (await owner.c.get(`/api/v1/groups/${g.id}/members`, { headers: h })).json()).items).toEqual([cat.id]);
    const off = await owner.c.post('/api/v1/positions', { headers: h, data: { title: 'Payroll lead' } });
    expect([off.status(), (await off.json()).code]).toEqual([409, 'positions_disabled']);
    await owner.c.put('/api/v1/settings/directory.positions.enabled', { headers: h, data: { value: true } });
    expect((await owner.c.post('/api/v1/positions', { headers: h, data: { title: 'Payroll lead', holder_membership_id: cat.id } })).status()).toBe(201);
    const member = await enter(cat.email, ws.id);
    expect((await member.c.get('/api/v1/departments', { headers: member.headers })).status()).toBe(200);
    expect((await member.c.post('/api/v1/groups', { headers: member.headers, data: { name: 'Mine' } })).status()).toBe(403);
  });

  test('E2E-R1-DIR-04 CSV import: plan changes nothing, apply creates departments and links managers; bad rows reported', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const a = await join(request, owner, ws.id, 'imp-a');
    const b = await join(request, owner, ws.id, 'imp-b');
    const csv = `email,department,title,manager_email\n${a.email},Finance/Payables,Clerk,${b.email}\n${b.email},Finance,Controller,\nghost@acme.test,Finance,Ghost,\n`;
    const post = (mode: string) => owner.c.post(`/api/v1/members/import?mode=${mode}`, { headers: { ...h, 'content-type': 'text/csv' }, data: csv });
    const planned = await (await post('plan')).json();
    expect(planned.report.created_departments).toEqual(['Finance', 'Finance/Payables']);
    expect((await (await owner.c.get('/api/v1/departments', { headers: h })).json()).items).toEqual([]);
    const applied = await (await post('apply')).json();
    expect([applied.report.updated, applied.report.errors]).toEqual([2, 1]);
    expect(applied.report.rows.find((r: { email: string }) => r.email === 'ghost@acme.test').error).toMatch(/not an active member/);
    const dir = await (await owner.c.get(`/api/v1/members/${a.id}/directory`, { headers: h })).json();
    expect([dir.title, dir.manager_membership_id, dir.field_sources.manager]).toEqual(['Clerk', b.id, 'csv']);
    expect((await owner.c.post('/api/v1/members/import?mode=apply', { headers: { ...h, 'content-type': 'text/csv' }, data: 'name\nx\n' })).status()).toBe(422);
  });
});
