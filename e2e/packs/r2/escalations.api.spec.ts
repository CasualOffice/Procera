import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser, serverNow } from '../../fixtures/accounts';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// Reminders, escalations and admin oversight of work items over the API (Procera 0.2; BPMS-085/088).
// Timers are moved with /__test/work-items/backdate and fired with /__test/drain, so the shared clock
// stays put for the tests running beside these. Business-time precision is covered by the DB tests
// (crates/requests/tests/escalations.rs). E2E-R2-21, -22 (and -11 subset: reassign after due), -23.

type Req = Parameters<typeof seedUser>[0];
type Member = Session & { membership: string };
type Work = { id: string; revision: number; due_at: string; overdue: boolean; assignees: string[]; request: { id: string } };

async function joinAs(request: Req, owner: Session, ws: string, tag: string): Promise<Member> {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const bearer = { authorization: `Bearer ${pw.token}` };
  const joined = await (await pw.c.post('/api/v1/invitations/accept', { headers: bearer, data: { token } })).json();
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: bearer, data: { workspace_id: ws } })).json();
  return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` }, membership: joined.membership_id };
}

async function reportsTo(owner: Session, who: Member, manager: Member) {
  const r = await owner.c.patch(`/api/v1/members/${who.membership}`, { headers: owner.headers, data: { manager_membership_id: manager.membership } });
  expect(r.status(), await r.text()).toBe(200);
}

async function leave(m: Session) {
  const data = { type: 'hr.leave-request', input: { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23' } };
  const r = await m.c.post('/api/v1/requests', { headers: { ...m.headers, 'idempotency-key': `esc-${Date.now()}-${Math.random()}` }, data });
  expect(r.status(), await r.text()).toBe(201);
  return (await r.json()) as { id: string };
}

async function work(m: Session, state = 'open'): Promise<Work[]> {
  const r = await m.c.get(`/api/v1/work?state=${state}`, { headers: m.headers });
  expect(r.status(), await r.text()).toBe(200);
  return (await r.json()).items;
}

async function count(m: Session, kind: string) {
  const items = (await (await m.c.get('/api/v1/notifications?limit=200', { headers: m.headers })).json()).items as { kind: string }[];
  return items.filter((n) => n.kind === kind).length;
}

/** Moves the item's due date and timers `seconds` into the past, then runs due work. */
async function backdate(request: Req, ws: string, item: string, seconds: number) {
  const r = await request.post('/__test/work-items/backdate', { data: { workspace: ws, work_item: item, seconds: Math.ceil(seconds) } });
  expect(r.status(), await r.text()).toBe(204);
  await request.post('/__test/drain');
}

test.describe('work-item reminders and escalations @r2 @requests', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-21 an overdue step reminds its approver once per reminder and is listed as overdue', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [boss, ann] = [await joinAs(request, owner, ws.id, 'boss'), await joinAs(request, owner, ws.id, 'ann')];
    await reportsTo(owner, ann, boss);
    const r = await leave(ann);
    const [item] = await work(boss);
    expect(item.overdue).toBe(false);
    expect(await count(boss, 'work_reminder')).toBe(0);
    // The due date passes (a minute ago): both reminders, the step is overdue, the escalation (a
    // business day later) is still to come.
    await backdate(request, ws.id, item.id, (Date.parse(item.due_at) - (await serverNow(boss.c))) / 1000 + 60);
    expect(await count(boss, 'work_reminder')).toBe(2);
    expect((await work(boss))[0].assignees).toEqual(['boss']);
    await request.post('/__test/drain');
    expect(await count(boss, 'work_reminder')).toBe(2);
    expect((await work(boss))[0].overdue).toBe(true);
    const overdue = await work(owner, 'overdue');
    expect(overdue.find((w) => w.request.id === r.id)).toMatchObject({ overdue: true, assignees: ['boss'] });
    expect((await boss.c.get('/api/v1/work?state=overdue', { headers: boss.headers })).status()).toBe(403);
    const d = await (await ann.c.get(`/api/v1/requests/${r.id}`, { headers: ann.headers })).json();
    expect(d.timeline.filter((t: { kind: string }) => t.kind === 'reminded')).toHaveLength(2);
    expect(await count(ann, 'work_reminder')).toBe(0);
  });

  test('E2E-R2-22 a manager approval still open a business day after due goes to the manager’s manager', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [dana, boss, ann] = [await joinAs(request, owner, ws.id, 'dana'), await joinAs(request, owner, ws.id, 'boss'), await joinAs(request, owner, ws.id, 'ann')];
    await reportsTo(owner, boss, dana);
    await reportsTo(owner, ann, boss);
    const r = await leave(ann);
    const [item] = await work(boss);
    await backdate(request, ws.id, item.id, 10 * 86_400);
    expect(await work(boss)).toEqual([]);
    const moved = (await work(dana)).find((w) => w.request.id === r.id)!;
    expect(moved.assignees).toEqual(['dana']);
    expect(await count(dana, 'work_assigned')).toBe(1);
    expect(await count(boss, 'work_reassigned')).toBe(1);
    const d = await (await ann.c.get(`/api/v1/requests/${r.id}`, { headers: ann.headers })).json();
    expect(d.timeline.map((t: { text: string }) => t.text)).toContain('Escalated: Manager approval reassigned to dana (escalated after due)');
    const ok = await dana.c.post(`/api/v1/work/${moved.id}/decisions`, { headers: { ...dana.headers, 'if-match': `"${moved.revision}"` }, data: { outcome: 'approved' } });
    expect(ok.status(), await ok.text()).toBe(200);
  });

  test('E2E-R2-23 an admin reassigns an unassigned or stuck step; SoD and a reason are enforced', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [carl, cleo] = [await joinAs(request, owner, ws.id, 'carl'), await joinAs(request, owner, ws.id, 'cleo')];
    const r = await leave(carl); // Carl has no manager and nobody holds process.owner: unassigned
    const item = (await work(owner, 'unassigned')).find((w) => w.request.id === r.id)!;
    expect(item.assignees).toEqual([]);
    const post = (who: Session, data: Record<string, unknown>) => who.c.post(`/api/v1/work/${item.id}/reassign`, { headers: who.headers, data });
    expect((await post(cleo, { membership_id: cleo.membership, reason: 'self' })).status()).toBe(403);
    const sod = await post(owner, { membership_id: carl.membership, reason: 'he knows best' });
    expect(sod.status()).toBe(422);
    expect((await sod.json()).errors[0]).toMatchObject({ pointer: '/membership_id', code: 'sod_excluded' });
    expect((await post(owner, { membership_id: cleo.membership, reason: ' ' })).status()).toBe(422);
    const ok = await post(owner, { membership_id: cleo.membership, reason: 'Carl has no manager yet' });
    expect(ok.status(), await ok.text()).toBe(200);
    expect(await ok.json()).toMatchObject({ id: item.id, assignees: ['cleo'] });
    expect((await work(owner, 'unassigned')).map((w) => w.id)).not.toContain(item.id);
    expect((await work(cleo)).map((w) => w.id)).toContain(item.id);
    expect(await count(cleo, 'work_assigned')).toBe(1);
    const d = await (await carl.c.get(`/api/v1/requests/${r.id}`, { headers: carl.headers })).json();
    expect(d.timeline.map((t: { text: string }) => t.text)).toContain('Reassigned: Manager approval to cleo by Olivia (Carl has no manager yet)');
  });
});
