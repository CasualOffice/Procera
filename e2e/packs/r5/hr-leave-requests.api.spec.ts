import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase } from '../../fixtures/accounts';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R5-HR-03 — leave requests against balances (BPMS-224, docs/25 §4, §7): the leave form offers
// the workspace's leave types; a quote counts working days (a holiday mid-week isn't counted) and
// shows the balance after; starting the request holds the days; overlapping dates, too many days in
// a row and a missing note are refused with field errors; the manager approving takes the days,
// rejecting releases them, and the requester cancelling releases them.

type Bal = { leave_type: { key: string }; available: number; pending: number; taken: number };
const of = (items: Bal[], key: string) => items.find((b) => b.leave_type.key === key)!;
const iso = (d: Date) => d.toISOString().slice(0, 10);
const plus = (d: Date, n: number) => new Date(d.getTime() + n * 86_400_000);

async function member(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = `${name.toLowerCase()}-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}@acme.test`;
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

test.describe('leave requests @r5 @hr', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R5-HR-03 a leave request holds days, approval takes them, rejection and cancelling release them', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const me = (await (await owner.c.get('/api/v1/me/permissions', { headers: h })).json()).membership_id as string;
    const now = new Date((await (await request.get('/version')).json()).server_time);
    // The Monday at least a week ahead (annual leave needs 3 days' notice); a holiday on its Wednesday.
    const mon = plus(now, 7 + ((8 - now.getUTCDay()) % 7));
    const [tue, wed, fri] = [plus(mon, 1), plus(mon, 2), plus(mon, 4)];
    const { types } = await (await owner.c.post('/api/v1/leave-types/defaults', { headers: h })).json();
    const t = (k: string) => types.find((x: { key: string }) => x.key === k);
    const cal = await (await owner.c.post('/api/v1/holiday-calendars', { headers: h, data: { name: 'Office' } })).json();
    expect((await owner.c.put(`/api/v1/holiday-calendars/${cal.id}/holidays/${iso(wed)}`, { headers: h, data: { name: 'Founders Day' } })).status()).toBe(201);
    const loc = await (await owner.c.post('/api/v1/work-locations', { headers: h, data: { name: 'Office', timezone: 'UTC', holiday_calendar_id: cal.id } })).json();
    const ann = await member(request, owner, ws.id, 'Ann');
    expect((await owner.c.put(`/api/v1/members/${ann.id}/hr`, { headers: h, data: { work_location_id: loc.id, start_date: `${now.getUTCFullYear()}-01-01` } })).ok()).toBe(true);
    expect((await owner.c.patch(`/api/v1/members/${ann.id}`, { headers: h, data: { manager_membership_id: me } })).ok()).toBe(true);
    // Enough annual leave whatever the month.
    expect((await owner.c.post(`/api/v1/members/${ann.id}/time-off/adjustments`, { headers: h, data: { leave_type_id: t('annual').id, days: 10, reason: 'Opening balance' } })).status()).toBe(201);
    const a = await enter(ann.email, ws.id);
    const ah = a.headers;
    const balances = async () => (await (await a.c.get('/api/v1/me/time-off', { headers: ah })).json());
    const start0 = of((await balances()).items, 'annual').available;

    // The form offers the workspace's types.
    const types0 = (await (await a.c.get('/api/v1/request-types', { headers: ah })).json()).items;
    const field = types0.find((x: { key: string }) => x.key === 'hr.leave-request').form.fields.find((f: { name: string }) => f.name === 'leave_type');
    expect(field).toMatchObject({ type: 'enum', options: expect.arrayContaining([{ value: 'casual', label: 'Casual leave' }]) });

    // Quote: Monday to Friday is 4 days (the holiday isn't counted).
    const q = await (await a.c.post('/api/v1/time-off/quote', { headers: ah, data: { leave_type: 'annual', start_date: iso(mon), end_date: iso(fri) } })).json();
    expect(q).toMatchObject({ days: 4, available: start0, after: start0 - 4, problems: [] });
    expect(q.lines.find((l: { date: string }) => l.date === iso(wed))).toMatchObject({ halves: 0, why: { kind: 'holiday', name: 'Founders Day' } });

    const ask = async (input: Record<string, unknown>) => a.c.post('/api/v1/requests', { headers: { ...ah, 'idempotency-key': `lr-${Date.now()}-${Math.random()}` }, data: { type: 'hr.leave-request', input } });
    const r1 = await ask({ leave_type: 'annual', start_date: iso(mon), end_date: iso(fri), reason: 'Family trip' });
    expect(r1.status(), await r1.text()).toBe(201);
    const req1 = await r1.json();
    let b = await balances();
    expect(of(b.items, 'annual')).toMatchObject({ pending: 4, available: start0 - 4 });
    expect(b.requests[0]).toMatchObject({ request_id: req1.id, state: 'pending', days: 4 });

    // Refusals, each on its field.
    const codes = async (res: Awaited<ReturnType<typeof ask>>) => { expect(res.status()).toBe(422); return ((await res.json()).errors as { pointer: string; code: string }[]).map((e) => `${e.pointer}:${e.code}`); };
    expect(await codes(await ask({ leave_type: 'annual', start_date: iso(tue), end_date: iso(tue) }))).toContain('/input/start_date:overlap');
    expect(await codes(await ask({ leave_type: 'casual', start_date: iso(plus(mon, 7)), end_date: iso(plus(mon, 10)) }))).toContain('/input/end_date:too_long');
    expect(await codes(await ask({ leave_type: 'sick', start_date: iso(plus(mon, 7)), end_date: iso(plus(mon, 10)) }))).toContain('/input/supporting_document:note_required');
    expect(await codes(await ask({ leave_type: 'holiday', start_date: iso(plus(mon, 7)), end_date: iso(plus(mon, 7)) }))).toContain('/input/leave_type:unknown');

    // The manager approves: the days are taken.
    const decide = async (id: string, data: Record<string, unknown>) => {
      const items = (await (await owner.c.get('/api/v1/work?state=open', { headers: h })).json()).items as { id: string; revision: number; request: { id: string } }[];
      const w = items.find((x) => x.request.id === id)!;
      const res = await owner.c.post(`/api/v1/work/${w.id}/decisions`, { headers: { ...h, 'if-match': `"${w.revision}"` }, data });
      expect(res.status(), await res.text()).toBe(200);
    };
    await decide(req1.id, { outcome: 'approved' });
    b = await balances();
    expect(of(b.items, 'annual')).toMatchObject({ pending: 0, taken: 4, available: start0 - 4 });
    expect(b.requests.find((r: { request_id: string }) => r.request_id === req1.id).state).toBe('approved');

    // An afternoon-to-morning request is one day; rejecting releases it.
    const r2 = await ask({ leave_type: 'annual', start_date: iso(plus(mon, 7)), start_half: 'second_half', end_date: iso(plus(mon, 8)), end_half: 'first_half' });
    expect(r2.status(), await r2.text()).toBe(201);
    const req2 = await r2.json();
    expect(of((await balances()).items, 'annual')).toMatchObject({ pending: 1, available: start0 - 5 });
    await decide(req2.id, { outcome: 'rejected', comment: 'Release week' });
    b = await balances();
    expect(of(b.items, 'annual')).toMatchObject({ pending: 0, available: start0 - 4 });
    expect(b.requests.find((r: { request_id: string }) => r.request_id === req2.id).state).toBe('rejected');

    // Cancelling one's own pending request releases it too.
    const r3 = await ask({ leave_type: 'annual', start_date: iso(plus(mon, 14)), end_date: iso(plus(mon, 14)) });
    const req3 = await r3.json();
    expect(of((await balances()).items, 'annual').pending).toBe(1);
    expect((await a.c.post(`/api/v1/requests/${req3.id}/cancel`, { headers: ah, data: { reason: 'Plans changed' } })).status()).toBe(200);
    b = await balances();
    expect(of(b.items, 'annual')).toMatchObject({ pending: 0, available: start0 - 4 });
    expect(b.requests.find((r: { request_id: string }) => r.request_id === req3.id).state).toBe('cancelled');
  });
});
