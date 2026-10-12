import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// Requests and approvals over the API (Procera 0.2; BPMS-083/085/101/103): built-in request types,
// start with Idempotency-Key, manager and finance approvals, separation of duties, the send-back
// loop, cancel, the double-approval race, unassigned steps and workspace isolation.
// E2E-R2-02 (subset: CEL thresholds, no rule table), -03, -04, -09 (no diff yet), -17..-20, -ISO.

type Req = Parameters<typeof seedUser>[0];
type Member = Session & { email: string; membership: string };

async function joinAs(request: Req, owner: Session, ws: string, tag: string, role = 'member'): Promise<Member> {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: [role] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const bearer = { authorization: `Bearer ${pw.token}` };
  const joined = await (await pw.c.post('/api/v1/invitations/accept', { headers: bearer, data: { token } })).json();
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: bearer, data: { workspace_id: ws } })).json();
  return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` }, email, membership: joined.membership_id };
}

async function reportsTo(owner: Session, who: Member, manager: Member) {
  const r = await owner.c.patch(`/api/v1/members/${who.membership}`, { headers: owner.headers, data: { manager_membership_id: manager.membership } });
  expect(r.status(), await r.text()).toBe(200);
}

let seq = 0;
const key = () => `e2e-${Date.now()}-${seq++}-${Math.random().toString(36).slice(2)}`;

async function start(m: Session, type: string, input: Record<string, unknown>, idem = key()) {
  return m.c.post('/api/v1/requests', { headers: { ...m.headers, 'idempotency-key': idem }, data: { type, input } });
}

async function started(m: Session, type: string, input: Record<string, unknown>) {
  const r = await start(m, type, input);
  expect(r.status(), await r.text()).toBe(201);
  return (await r.json()) as { id: string; display_id: string; status: string };
}

type Work = { id: string; revision: number; step: string; outcomes: string[]; request: { id: string }; values: Record<string, unknown> };

async function inbox(m: Session, state = 'open'): Promise<Work[]> {
  const r = await m.c.get(`/api/v1/work?state=${state}`, { headers: m.headers });
  expect(r.status(), await r.text()).toBe(200);
  return (await r.json()).items;
}

async function itemFor(m: Session, requestId: string) {
  const item = (await inbox(m)).find((w) => w.request.id === requestId);
  expect(item, 'an open step for this request').toBeTruthy();
  return item!;
}

async function decide(m: Session, item: Work, data: Record<string, unknown>) {
  return m.c.post(`/api/v1/work/${item.id}/decisions`, { headers: { ...m.headers, 'if-match': `"${item.revision}"` }, data });
}

async function detail(m: Session, id: string) {
  return (await m.c.get(`/api/v1/requests/${id}`, { headers: m.headers })).json();
}

async function kinds(m: Session) {
  return ((await (await m.c.get('/api/v1/notifications?limit=200', { headers: m.headers })).json()).items as { kind: string }[]).map((n) => n.kind);
}

const leave = { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23', reason: 'Holidays' };
const expense = (amount: string) => ({ amount: { amount, currency: 'EUR' }, category: 'travel', description: 'Client visit', expense_date: '2026-10-01' });

test.describe('requests @r2 @requests', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-17 a leave request goes to the manager, is approved and the requester is told', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [boss, ann] = [await joinAs(request, owner, ws.id, 'boss'), await joinAs(request, owner, ws.id, 'ann')];
    await reportsTo(owner, ann, boss);
    const types = (await (await ann.c.get('/api/v1/request-types', { headers: ann.headers })).json()).items;
    expect(types.map((t: { key: string }) => t.key)).toEqual(expect.arrayContaining(['hr.leave-request', 'hr.expense-claim', 'fin.purchase-requisition', 'it.access-request']));
    const lr = types.find((t: { key: string }) => t.key === 'hr.leave-request');
    expect(lr.form.fields.map((f: { name: string; type: string }) => `${f.name}:${f.type}`)).toEqual(['leave_type:enum', 'start_date:date', 'end_date:date', 'reason:text']);

    const r = await started(ann, 'hr.leave-request', leave);
    expect(r.display_id).toMatch(/^LR-\d{4}-\d{6}$/);
    expect(r.status).toBe('in_progress');
    const mine = (await (await ann.c.get('/api/v1/requests?scope=mine', { headers: ann.headers })).json()).items;
    expect(mine[0]).toMatchObject({ id: r.id, type: 'hr.leave-request', type_name: 'Leave request', status: 'in_progress', current_step: 'Manager approval', assignees: ['boss'] });
    expect(await inbox(ann)).toEqual([]);
    expect(await kinds(boss)).toContain('work_assigned');

    const item = await itemFor(boss, r.id);
    expect(item.outcomes).toEqual(expect.arrayContaining(['approved', 'rejected', 'changes_requested']));
    const ok = await decide(boss, item, { outcome: 'approved', comment: 'Enjoy' });
    expect(ok.status(), await ok.text()).toBe(200);
    expect((await ok.json()).request.status).toBe('approved');
    const d = await detail(ann, r.id);
    expect(d.status).toBe('approved');
    expect(d.steps[0]).toMatchObject({ name: 'Manager approval', state: 'completed', outcome: 'approved', decided_by: 'boss', comment: 'Enjoy' });
    expect(d.timeline.map((t: { kind: string }) => t.kind)).toEqual(['submitted', 'decided', 'completed']);
    expect(await kinds(ann)).toContain('request_completed');
    expect((await inbox(boss, 'done')).map((w) => w.request.id)).toContain(r.id);
  });

  test('E2E-R2-02 an expense over the threshold needs a second approval by finance', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [boss, ann, fin] = [await joinAs(request, owner, ws.id, 'boss'), await joinAs(request, owner, ws.id, 'ann'), await joinAs(request, owner, ws.id, 'fin')];
    await reportsTo(owner, ann, boss);
    const g = await (await owner.c.post('/api/v1/groups', { headers: owner.headers, data: { name: 'Finance' } })).json();
    for (const m of [fin, boss]) {
      await owner.c.post(`/api/v1/groups/${g.id}/members`, { headers: owner.headers, data: { membership_id: m.membership } });
    }
    const small = await started(ann, 'hr.expense-claim', expense('120'));
    await decide(boss, await itemFor(boss, small.id), { outcome: 'approved' });
    expect((await detail(ann, small.id)).status).toBe('approved');

    const big = await started(ann, 'hr.expense-claim', expense('900'));
    expect((await (await decide(boss, await itemFor(boss, big.id), { outcome: 'approved' })).json()).request.status).toBe('in_progress');
    const d = await detail(ann, big.id);
    expect(d.current_step).toBe('Finance approval');
    expect(d.assignees).toEqual(['fin']); // boss decided the manager step: excluded from finance
    expect(await kinds(ann)).toContain('request_decided');
    await decide(fin, await itemFor(fin, big.id), { outcome: 'approved' });
    expect((await detail(ann, big.id)).status).toBe('approved');
  });

  test('E2E-R2-03 a repeated start with the same Idempotency-Key returns the same request; another body is 409', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const ann = await joinAs(request, owner, ws.id, 'ann');
    const k = key();
    const a = await start(ann, 'hr.leave-request', leave, k);
    const b = await start(ann, 'hr.leave-request', leave, k);
    expect([a.status(), b.status()]).toEqual([201, 201]);
    expect((await b.json()).id).toBe((await a.json()).id);
    expect((await start(ann, 'hr.leave-request', { ...leave, reason: 'Other' }, k)).status()).toBe(409);
    expect((await ann.c.post('/api/v1/requests', { headers: ann.headers, data: { type: 'hr.leave-request', input: leave } })).status()).toBe(400);
    const bad = await start(ann, 'hr.leave-request', { leave_type: 'holiday', colour: 'red' });
    expect(bad.status()).toBe(422);
    const problem = await bad.json();
    expect(problem.code).toBe('validation_failed');
    expect(problem.errors.map((e: { pointer: string }) => e.pointer)).toEqual(['/input/colour', '/input/leave_type', '/input/start_date', '/input/end_date']);
  });

  test('E2E-R2-04 two approvers deciding at once: exactly one wins, the other gets already_decided', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [boss, ann, f1, f2] = [
      await joinAs(request, owner, ws.id, 'boss'), await joinAs(request, owner, ws.id, 'ann'),
      await joinAs(request, owner, ws.id, 'f1'), await joinAs(request, owner, ws.id, 'f2'),
    ];
    await reportsTo(owner, ann, boss);
    const g = await (await owner.c.post('/api/v1/groups', { headers: owner.headers, data: { name: 'Finance' } })).json();
    for (const m of [f1, f2]) {
      await owner.c.post(`/api/v1/groups/${g.id}/members`, { headers: owner.headers, data: { membership_id: m.membership } });
    }
    const r = await started(ann, 'hr.expense-claim', expense('700'));
    await decide(boss, await itemFor(boss, r.id), { outcome: 'approved' });
    const item = await itemFor(f1, r.id);
    const [a, b] = await Promise.all([decide(f1, item, { outcome: 'approved' }), decide(f2, item, { outcome: 'rejected', comment: 'No' })]);
    const statuses = [a.status(), b.status()].sort();
    expect(statuses).toEqual([200, 409]);
    const loser = a.status() === 409 ? a : b;
    expect((await loser.json()).code).toBe('already_decided');
    // A stale revision is refused too.
    expect((await decide(f1, item, { outcome: 'approved' })).status()).toBe(409);
  });

  test('E2E-R2-09 send back with a comment, the requester edits and resubmits, the manager decides again', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [boss, ann] = [await joinAs(request, owner, ws.id, 'boss'), await joinAs(request, owner, ws.id, 'ann')];
    await reportsTo(owner, ann, boss);
    const r = await started(ann, 'hr.leave-request', leave);
    const first = await itemFor(boss, r.id);
    const noComment = await decide(boss, first, { outcome: 'changes_requested' });
    expect(noComment.status()).toBe(422);
    expect((await noComment.json()).errors[0].pointer).toBe('/comment');
    expect((await decide(boss, first, { outcome: 'changes_requested', comment: 'Pick other dates' })).status()).toBe(200);
    expect((await detail(ann, r.id)).status).toBe('changes_requested');
    expect(await kinds(ann)).toContain('request_decided');
    const revise = await itemFor(ann, r.id);
    expect(revise.step).toBe('Revise request');
    expect(revise.values.end_date).toBe('2026-12-23');
    expect((await decide(ann, revise, { outcome: 'submitted', fields: { end_date: '2026-12-24' } })).status()).toBe(200);
    const d = await detail(ann, r.id);
    expect([d.status, d.current_step, d.input.end_date]).toEqual(['in_progress', 'Manager approval', '2026-12-24']);
    await decide(boss, await itemFor(boss, r.id), { outcome: 'approved' });
    expect((await detail(ann, r.id)).status).toBe('approved');
  });

  test('E2E-R2-18 separation of duties: the requester is never offered her own request and cannot decide it', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [boss, ann, out] = [await joinAs(request, owner, ws.id, 'boss'), await joinAs(request, owner, ws.id, 'ann'), await joinAs(request, owner, ws.id, 'out')];
    await reportsTo(owner, ann, boss);
    const g = await (await owner.c.post('/api/v1/groups', { headers: owner.headers, data: { name: 'Finance' } })).json();
    for (const m of [ann, boss]) {
      await owner.c.post(`/api/v1/groups/${g.id}/members`, { headers: owner.headers, data: { membership_id: m.membership } });
    }
    const r = await started(ann, 'hr.expense-claim', expense('800'));
    const item = await itemFor(boss, r.id);
    expect((await decide(ann, item, { outcome: 'approved' })).status()).toBe(403);
    expect((await decide(out, item, { outcome: 'approved' })).status()).toBe(404);
    expect((await out.c.get(`/api/v1/requests/${r.id}`, { headers: out.headers })).status()).toBe(404);
    expect((await out.c.get('/api/v1/requests?scope=all', { headers: out.headers })).status()).toBe(403);
    await decide(boss, item, { outcome: 'approved' });
    // Finance: Ann (initiator) and Boss (decided the manager step) are both excluded, nobody is left,
    // the fallback role is unheld, so the step is unassigned and visible to admins.
    const d = await detail(ann, r.id);
    expect(d.steps.find((s: { name: string }) => s.name === 'Finance approval')).toMatchObject({ name: 'Finance approval', unassigned: true, assignees: [] });
    const unassigned = await (await owner.c.get('/api/v1/work?state=unassigned', { headers: owner.headers })).json();
    expect(unassigned.items.map((w: Work) => w.request.id)).toContain(r.id);
    expect((await ann.c.get('/api/v1/work?state=unassigned', { headers: ann.headers })).status()).toBe(403);
    const all = (await (await owner.c.get('/api/v1/requests?scope=all', { headers: owner.headers })).json()).items;
    expect(all.map((x: { id: string }) => x.id)).toContain(r.id);
  });

  test('E2E-R2-20 the requester cancels; the open step closes and cancelling again is a conflict', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [boss, ann] = [await joinAs(request, owner, ws.id, 'boss'), await joinAs(request, owner, ws.id, 'ann')];
    await reportsTo(owner, ann, boss);
    const r = await started(ann, 'hr.leave-request', leave);
    expect((await detail(ann, r.id)).can_cancel).toBe(true);
    expect((await detail(boss, r.id)).can_cancel).toBe(false);
    expect((await boss.c.post(`/api/v1/requests/${r.id}/cancel`, { headers: boss.headers, data: { reason: 'no' } })).status()).toBe(403);
    const c = await ann.c.post(`/api/v1/requests/${r.id}/cancel`, { headers: ann.headers, data: { reason: 'Plans changed' } });
    expect(c.status(), await c.text()).toBe(200);
    expect((await c.json()).status).toBe('cancelled');
    expect((await inbox(boss)).find((w) => w.request.id === r.id)).toBeUndefined();
    expect((await ann.c.post(`/api/v1/requests/${r.id}/cancel`, { headers: ann.headers, data: { reason: 'again' } })).status()).toBe(409);
  });

  test('E2E-R2-ISO requests and work items of another workspace are not found', async ({ request }) => {
    const a = await workspaceWithOwner(request);
    const b = await workspaceWithOwner(request);
    const [boss, ann] = [await joinAs(request, a.owner, a.ws.id, 'boss'), await joinAs(request, a.owner, a.ws.id, 'ann')];
    await reportsTo(a.owner, ann, boss);
    const r = await started(ann, 'hr.leave-request', leave);
    const item = await itemFor(boss, r.id);
    expect((await b.owner.c.get(`/api/v1/requests/${r.id}`, { headers: b.owner.headers })).status()).toBe(404);
    expect((await decide(b.owner, item, { outcome: 'approved' })).status()).toBe(404);
    expect((await b.owner.c.post(`/api/v1/requests/${r.id}/cancel`, { headers: b.owner.headers, data: { reason: 'x' } })).status()).toBe(404);
    const theirs = (await (await b.owner.c.get('/api/v1/requests?scope=all', { headers: b.owner.headers })).json()).items;
    expect(theirs.map((x: { id: string }) => x.id)).not.toContain(r.id);
  });
});
