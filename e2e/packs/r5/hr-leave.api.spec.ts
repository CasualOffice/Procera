import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase } from '../../fixtures/accounts';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R5-HR-02 — leave types, policies and balances (BPMS-223, docs/25 §2.2, §3): the owner sets up
// the common types and a Standard policy (once); a member who started on 1 January has this year's
// allowance (annual given monthly, sick/casual/optional up front, unpaid unlimited); an adjustment
// needs whole or half days and a reason, and shows in the member's ledger with who made it; members
// can't adjust or read others' balances; a policy for a location takes over by priority, and
// switching tops the allowance up to the new target instead of granting twice.

async function member(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = `${name.toLowerCase()}-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}@acme.test`;
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

type Bal = { leave_type: { key: string }; granted: number; available: number; taken: number; adjusted: number; entries: { kind: string; reason: string | null; by: string | null; days: number }[] };
const of = (items: Bal[], key: string) => items.find((b) => b.leave_type.key === key)!;

test.describe('leave policies and balances @r5 @hr', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R5-HR-02 types, a standard policy, balances from the ledger, adjustments and policy priority', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const now = new Date((await (await request.get('/version')).json()).server_time);
    const month = now.getUTCMonth() + 1;
    const year = now.getUTCFullYear();

    const set = await owner.c.post('/api/v1/leave-types/defaults', { headers: h });
    expect(set.status(), await set.text()).toBe(201);
    const { types, policy } = await set.json();
    expect(types.map((t: { key: string }) => t.key)).toEqual(['annual', 'sick', 'casual', 'optional_holiday', 'unpaid']);
    expect(policy).toMatchObject({ name: 'Standard', year_start_month: 1 });
    expect((await owner.c.post('/api/v1/leave-types/defaults', { headers: h })).status()).toBe(409);
    const annual = types.find((t: { key: string }) => t.key === 'annual');

    // Ann started on 1 January: 1.5 annual days a month so far, the rest in full.
    const ann = await member(request, owner, ws.id, 'Ann');
    expect((await owner.c.put(`/api/v1/members/${ann.id}/hr`, { headers: h, data: { start_date: `${year}-01-01` } })).ok()).toBe(true);
    const a = await enter(ann.email, ws.id);
    const mine = await (await a.c.get('/api/v1/me/time-off', { headers: a.headers })).json();
    expect(mine.policy).toMatchObject({ name: 'Standard', via: 'audience' });
    expect(of(mine.items, 'annual')).toMatchObject({ granted: Math.floor((36 * month) / 12) / 2, available: Math.floor((36 * month) / 12) / 2 });
    expect([of(mine.items, 'sick').available, of(mine.items, 'casual').available, of(mine.items, 'optional_holiday').available]).toEqual([12, 6, 2]);
    expect(of(mine.items, 'unpaid')).toMatchObject({ taken: 0 });
    const before = of(mine.items, 'annual').available;

    // Adjustments: whole or half days with a reason; the member sees it, with who made it.
    const adj = (data: unknown, s = owner) => s.c.post(`/api/v1/members/${ann.id}/time-off/adjustments`, { headers: s === owner ? h : s.headers, data });
    expect((await adj({ leave_type_id: annual.id, days: 0.25, reason: 'x' })).status()).toBe(422);
    expect((await adj({ leave_type_id: annual.id, days: 1, reason: ' ' })).status()).toBe(422);
    expect((await adj({ leave_type_id: annual.id, days: 2, reason: 'Worked on a holiday' }, a)).status()).toBe(403);
    const ok = await adj({ leave_type_id: annual.id, days: 2, reason: 'Worked on a holiday' });
    expect(ok.status(), await ok.text()).toBe(201);
    const after = of((await (await a.c.get('/api/v1/me/time-off', { headers: a.headers })).json()).items, 'annual');
    expect(after).toMatchObject({ available: before + 2, adjusted: 2 });
    expect(after.entries.find((e) => e.kind === 'adjustment')).toMatchObject({ days: 2, reason: 'Worked on a holiday', by: 'Olivia' });
    // Owner can read Ann's; Ann can't read the owner's.
    expect((await owner.c.get(`/api/v1/members/${ann.id}/time-off`, { headers: h })).status()).toBe(200);
    const me = (await (await owner.c.get('/api/v1/me/permissions', { headers: h })).json()).membership_id as string;
    expect((await a.c.get(`/api/v1/members/${me}/time-off`, { headers: a.headers })).status()).toBe(403);
    expect((await a.c.get('/api/v1/time-off/balances', { headers: a.headers })).status()).toBe(403);

    // A Bangalore policy (24 days up front) comes first for people there.
    const blr = await owner.c.post('/api/v1/leave-policies', { headers: h, data: { name: 'Bangalore', audience: { all: false, departments: [], groups: [], roles: [], locations: ['Bangalore'] }, priority: 5, rules: [{ leave_type_id: annual.id, yearly_days: 24, accrual: 'upfront' }] } });
    expect(blr.status(), await blr.text()).toBe(201);
    const bp = await blr.json();
    expect((await owner.c.post('/api/v1/leave-policies', { headers: h, data: { name: 'Empty', audience: { all: false, departments: [], groups: [], roles: [] } } })).status()).toBe(422);
    expect((await owner.c.patch(`/api/v1/members/${ann.id}`, { headers: h, data: { location: 'Bangalore' } })).ok()).toBe(true);
    expect(await (await owner.c.get(`/api/v1/leave-policies/${bp.id}/members`, { headers: h })).json()).toMatchObject({ count: 1, items: [{ id: ann.id, name: 'Ann' }] });
    const moved = await (await a.c.get('/api/v1/me/time-off', { headers: a.headers })).json();
    expect(moved.policy).toMatchObject({ name: 'Bangalore', via: 'audience' });
    // Topped up to 24 (not 24 on top of what was already given), plus the adjustment.
    expect(of(moved.items, 'annual')).toMatchObject({ granted: 24, available: 26 });

    // HR's grid has everyone, by type.
    const grid = await (await owner.c.get('/api/v1/time-off/balances', { headers: h })).json();
    expect(grid.items.find((r: { id: string }) => r.id === ann.id).balances[annual.id]).toMatchObject({ available: 26 });
  });
});
