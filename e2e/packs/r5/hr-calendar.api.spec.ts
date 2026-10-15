import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase } from '../../fixtures/accounts';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R5-HR-04 — team calendar privacy and the org chart (BPMS-225, docs/25 §6): Ann and Bo report
// to the owner, Cy reports to Ann. A colleague sees Ann's approved sick leave only as "Out" (no type,
// no link, no days) and never sees pending requests; Ann sees her report Cy's pending request with
// its type; HR sees everything; "everyone" is HR only; the org chart nests Cy under Ann under the owner.

const iso = (d: Date) => d.toISOString().slice(0, 10);
const plus = (d: Date, n: number) => new Date(d.getTime() + n * 86_400_000);
type Cal = { people: { id: string; me: boolean }[]; absences: { membership_id: string; label: string; colour: string; state: string; request_id: string | null; days: number | null }[] };

async function member(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = `${name.toLowerCase()}-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}@acme.test`;
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

test.describe('team calendar @r5 @hr', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R5-HR-04 colleagues see "Out", managers and HR see the type, and the org chart nests reports', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const me = (await (await owner.c.get('/api/v1/me/permissions', { headers: h })).json()).membership_id as string;
    const now = new Date((await (await request.get('/version')).json()).server_time);
    const mon = plus(now, 7 + ((8 - now.getUTCDay()) % 7));
    const { types } = await (await owner.c.post('/api/v1/leave-types/defaults', { headers: h })).json();
    const annual = types.find((t: { key: string }) => t.key === 'annual');
    const [ann, bo, cy] = [await member(request, owner, ws.id, 'Ann'), await member(request, owner, ws.id, 'Bo'), await member(request, owner, ws.id, 'Cy')];
    for (const [who, boss] of [[ann.id, me], [bo.id, me], [cy.id, ann.id]]) {
      expect((await owner.c.patch(`/api/v1/members/${who}`, { headers: h, data: { manager_membership_id: boss } })).ok()).toBe(true);
    }
    expect((await owner.c.post(`/api/v1/members/${bo.id}/time-off/adjustments`, { headers: h, data: { leave_type_id: annual.id, days: 5, reason: 'Opening balance' } })).status()).toBe(201);
    const [a, b, c] = [await enter(ann.email, ws.id), await enter(bo.email, ws.id), await enter(cy.email, ws.id)];
    const ask = async (s: Session, input: Record<string, unknown>) => {
      const r = await s.c.post('/api/v1/requests', { headers: { ...s.headers, 'idempotency-key': `cal-${Date.now()}-${Math.random()}` }, data: { type: 'hr.leave-request', input } });
      expect(r.status(), await r.text()).toBe(201);
      return (await r.json()).id as string;
    };
    // Ann: sick leave Monday–Tuesday, approved by the owner. Bo: annual on Wednesday, pending. Cy: casual on Thursday, pending (with Ann).
    const annReq = await ask(a, { leave_type: 'sick', start_date: iso(mon), end_date: iso(plus(mon, 1)) });
    const items = (await (await owner.c.get('/api/v1/work?state=open', { headers: h })).json()).items as { id: string; revision: number; request: { id: string } }[];
    const w = items.find((x) => x.request.id === annReq)!;
    expect((await owner.c.post(`/api/v1/work/${w.id}/decisions`, { headers: { ...h, 'if-match': `"${w.revision}"` }, data: { outcome: 'approved' } })).status()).toBe(200);
    const boReq = await ask(b, { leave_type: 'annual', start_date: iso(plus(mon, 2)), end_date: iso(plus(mon, 2)) });
    const cyReq = await ask(c, { leave_type: 'casual', start_date: iso(plus(mon, 3)), end_date: iso(plus(mon, 3)) });

    const cal = async (s: Session, scope = 'team') => {
      const r = await s.c.get(`/api/v1/team-calendar?from=${iso(mon)}&to=${iso(plus(mon, 6))}&scope=${scope}`, { headers: s.headers });
      expect(r.status(), await r.text()).toBe(200);
      return (await r.json()) as Cal;
    };
    const absOf = (k: Cal, who: string) => k.absences.filter((x) => x.membership_id === who);

    // Bo (a colleague of Ann): Ann's sick leave is just "Out"; Bo's own pending request shows in full.
    const bv = await cal(b);
    expect(bv.people.map((p) => p.id).sort()).toEqual([me, ann.id, bo.id].sort());
    expect(absOf(bv, ann.id)).toEqual([expect.objectContaining({ label: 'Out', colour: 'gray', request_id: null, days: null, state: 'approved' })]);
    expect(absOf(bv, bo.id)).toEqual([expect.objectContaining({ label: 'Annual leave', state: 'pending', request_id: boReq, days: 1 })]);
    // Ann: Bo's pending request is hidden from a colleague; her report Cy's pending request shows with its type.
    const av = await cal(a);
    expect(av.people.map((p) => p.id)).toEqual(expect.arrayContaining([ann.id, me, bo.id, cy.id]));
    expect(absOf(av, bo.id)).toEqual([]);
    expect(absOf(av, cy.id)).toEqual([expect.objectContaining({ label: 'Casual leave', state: 'pending', request_id: cyReq })]);
    expect(absOf(av, ann.id)[0]).toMatchObject({ label: 'Sick leave', request_id: annReq, days: 2 });
    // HR sees everyone and everything; members can't ask for everyone.
    const hv = await cal(owner, 'everyone');
    expect(absOf(hv, ann.id)[0]).toMatchObject({ label: 'Sick leave' });
    expect(absOf(hv, bo.id)[0]).toMatchObject({ label: 'Annual leave', state: 'pending' });
    expect((await b.c.get(`/api/v1/team-calendar?from=${iso(mon)}&to=${iso(mon)}&scope=everyone`, { headers: b.headers })).status()).toBe(403);
    expect((await b.c.get(`/api/v1/team-calendar?from=${iso(mon)}&to=${iso(plus(mon, 120))}`, { headers: b.headers })).status()).toBe(422);

    // The org chart nests Cy under Ann under the owner.
    const org = (await (await c.c.get('/api/v1/org-chart', { headers: c.headers })).json()).items as { id: string; manager_id: string | null }[];
    const mgr = (id: string) => org.find((n) => n.id === id)!.manager_id;
    expect([mgr(cy.id), mgr(ann.id), mgr(bo.id), mgr(me)]).toEqual([ann.id, me, me, null]);
  });
});
