import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser, serverNow } from '../../fixtures/accounts';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// Workspace request settings and out of office over the API (Procera 0.2; BPMS-103, BPMS-086):
// E2E-R2-30 a disabled type is neither listed nor startable; -31 a type's audience limits who may
// start it; -32 money uses the workspace currency and thresholds read in it; -33 new work goes to
// the delegate of an away manager, who decides on behalf (E2E-R2-08 subset); -34 the delegate
// decides existing items, SoD holds for the delegate, admin and self-service rules.

type Req = Parameters<typeof seedUser>[0];
type Member = Session & { email: string; membership: string };
type Work = { id: string; revision: number; request: { id: string }; on_behalf_of: { id: string; name: string; until: string | null } | null };

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

async function setting(owner: Session, key: string, value: unknown) {
  const r = await owner.c.put(`/api/v1/settings/${key}`, { headers: owner.headers, data: { value } });
  return r;
}

const key = () => `e2e-${Date.now()}-${Math.random().toString(36).slice(2)}`;
const start = (m: Session, type: string, input: Record<string, unknown>) =>
  m.c.post('/api/v1/requests', { headers: { ...m.headers, 'idempotency-key': key() }, data: { type, input } });

async function started(m: Session, type: string, input: Record<string, unknown>): Promise<string> {
  const r = await start(m, type, input);
  expect(r.status(), await r.text()).toBe(201);
  return (await r.json()).id;
}

async function inbox(m: Session): Promise<Work[]> {
  const r = await m.c.get('/api/v1/work?state=open', { headers: m.headers });
  expect(r.status(), await r.text()).toBe(200);
  return (await r.json()).items;
}

const decide = (m: Session, w: Work) =>
  m.c.post(`/api/v1/work/${w.id}/decisions`, { headers: { ...m.headers, 'if-match': `"${w.revision}"` }, data: { outcome: 'approved' } });
const detail = async (m: Session, id: string) => (await m.c.get(`/api/v1/requests/${id}`, { headers: m.headers })).json();
const typeKeys = async (m: Session) => ((await (await m.c.get('/api/v1/request-types', { headers: m.headers })).json()).items as { key: string }[]).map((t) => t.key);

const leave = { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23' };
const expense = (amount: string, currency: string) => ({ amount: { amount, currency }, category: 'travel', description: 'Client visit', expense_date: '2026-10-01' });
const purchase = { amount: { amount: '100' }, vendor: 'Acme', cost_center: 'CC-1', justification: 'Laptops' };

/** An out-of-office from the server's today for 30 days (the shared e2e clock may run ahead). */
async function away(c: Session, delegate: Member) {
  const now = await serverNow(c.c);
  const day = (ms: number) => new Date(ms).toISOString().slice(0, 10);
  return { starts_on: day(now), ends_on: day(now + 30 * 86_400_000), delegate: delegate.membership, note: 'Back next month' };
}

async function names(owner: Session): Promise<Map<string, string>> {
  const items = (await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json()).items as { id: string; display_name: string }[];
  return new Map(items.map((m) => [m.id, m.display_name]));
}

test.describe('workspace requests and out of office @r2 @requests', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-30 a disabled request type is not listed and cannot be started', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const ann = await joinAs(request, owner, ws.id, 'ann');
    expect(await typeKeys(ann)).toContain('hr.leave-request');
    const off = await setting(owner, 'requests.types.enabled', ['hr.expense-claim', 'it.access-request', 'fin.purchase-requisition']);
    expect(off.status(), await off.text()).toBe(200);
    expect(await typeKeys(ann)).toEqual(['hr.expense-claim', 'fin.purchase-requisition', 'it.access-request']);
    const refused = await start(ann, 'hr.leave-request', leave);
    expect(refused.status()).toBe(403);
    expect((await refused.json()).detail).toContain('not offered');
    // Settings › Requests lists every type with its state; members may not read it.
    const all = (await (await owner.c.get('/api/v1/request-types?scope=all', { headers: owner.headers })).json()).items as { key: string; enabled: boolean }[];
    expect(all.find((t) => t.key === 'hr.leave-request')?.enabled).toBe(false);
    expect((await ann.c.get('/api/v1/request-types?scope=all', { headers: ann.headers })).status()).toBe(403);
    expect((await setting(owner, 'requests.types.enabled', ['*'])).status()).toBe(200);
    expect((await start(ann, 'hr.leave-request', leave)).status()).toBe(201);
  });

  test('E2E-R2-31 a type limited to a group can only be started by its members', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [ann, fin] = [await joinAs(request, owner, ws.id, 'ann'), await joinAs(request, owner, ws.id, 'fin')];
    const group = await (await owner.c.post('/api/v1/groups', { headers: owner.headers, data: { name: 'Buyers' } })).json();
    await owner.c.post(`/api/v1/groups/${group.id}/members`, { headers: owner.headers, data: { membership_id: fin.membership } });
    const bad = await setting(owner, 'requests.types.audience', { 'fin.purchase-requisition': { groups: ['0190f000-0000-7000-8000-000000000001'] } });
    expect(bad.status(), 'an unknown group is refused').toBe(422);
    expect((await setting(owner, 'requests.types.audience', { 'fin.purchase-requisition': { groups: [group.id] } })).status()).toBe(200);
    expect(await typeKeys(ann)).not.toContain('fin.purchase-requisition');
    expect(await typeKeys(fin)).toContain('fin.purchase-requisition');
    expect((await start(ann, 'fin.purchase-requisition', purchase)).status()).toBe(403);
    expect((await start(fin, 'fin.purchase-requisition', purchase)).status()).toBe(201);
  });

  test('E2E-R2-32 amounts use the workspace currency and approval limits read in it', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [boss, ann] = [await joinAs(request, owner, ws.id, 'boss'), await joinAs(request, owner, ws.id, 'ann')];
    await reportsTo(owner, ann, boss);
    expect((await setting(owner, 'requests.currency', 'USD')).status()).toBe(200);
    const types = (await (await ann.c.get('/api/v1/request-types', { headers: ann.headers })).json());
    expect(types.currency).toBe('USD');
    const exp = (types.items as { key: string; description: string; form: { fields: { type: string; currency?: string }[] } }[]).find((t) => t.key === 'hr.expense-claim')!;
    expect(exp.form.fields.find((f) => f.type === 'money')?.currency).toBe('USD');
    expect(exp.description).toContain('500 USD');
    const eur = await start(ann, 'hr.expense-claim', expense('900', 'EUR'));
    expect(eur.status()).toBe(422);
    expect((await eur.json()).errors[0].pointer).toBe('/input/amount');
    const id = await started(ann, 'hr.expense-claim', expense('900', 'USD'));
    expect((await detail(ann, id)).input.amount).toEqual({ amount: '900', currency: 'USD' });
    const item = (await inbox(boss)).find((w) => w.request.id === id)!;
    expect((await decide(boss, item)).status()).toBe(200);
    expect((await detail(ann, id)).current_step).toBe('Finance approval');
  });

  test('E2E-R2-33 new work goes to the delegate of an away manager, who approves on behalf', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [boss, ann, dana] = [await joinAs(request, owner, ws.id, 'boss'), await joinAs(request, owner, ws.id, 'ann'), await joinAs(request, owner, ws.id, 'dana')];
    await reportsTo(owner, ann, boss);
    const set = await boss.c.put('/api/v1/me/out-of-office', { headers: boss.headers, data: await away(boss, dana) });
    expect(set.status(), await set.text()).toBe(200);
    const ooo = await set.json();
    const mine = await (await boss.c.get('/api/v1/me/out-of-office', { headers: boss.headers })).json();
    expect(mine.out_of_office).toMatchObject({ active: true, delegate: { id: dana.membership }, note: 'Back next month' });
    expect(mine.can_set).toBe(true);

    const id = await started(ann, 'hr.leave-request', leave);
    expect((await inbox(boss)).find((w) => w.request.id === id)).toBeUndefined();
    const item = (await inbox(dana)).find((w) => w.request.id === id)!;
    expect(item.on_behalf_of?.id).toBe(boss.membership);
    expect(item.on_behalf_of?.until).toBe(ooo.ends_on);
    expect((await decide(dana, item)).status()).toBe(200);
    const n = await names(owner);
    const d = await detail(ann, id);
    expect(d.status).toBe('approved');
    expect(d.steps[0]).toMatchObject({ decided_by: n.get(dana.membership), on_behalf_of: n.get(boss.membership) });
    expect((d.timeline as { kind: string; text: string }[]).some((t) => t.kind === 'delegated')).toBe(true);
    expect((d.timeline as { text: string }[]).some((t) => t.text.endsWith(`on behalf of ${n.get(boss.membership)}`))).toBe(true);
  });

  test('E2E-R2-34 the delegate decides existing work; SoD holds for the delegate; who may set out of office', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [boss, ann, dana] = [await joinAs(request, owner, ws.id, 'boss'), await joinAs(request, owner, ws.id, 'ann'), await joinAs(request, owner, ws.id, 'dana')];
    await reportsTo(owner, ann, boss);
    const id = await started(ann, 'hr.leave-request', leave);
    const bossItem = (await inbox(boss)).find((w) => w.request.id === id)!;

    // An admin makes the requester the delegate: she still cannot approve her own request.
    const toAnn = await owner.c.put(`/api/v1/members/${boss.membership}/out-of-office`, { headers: owner.headers, data: await away(owner, ann) });
    expect(toAnn.status(), await toAnn.text()).toBe(200);
    expect((await toAnn.json()).set_by.id).not.toBe(boss.membership);
    expect((await inbox(ann)).find((w) => w.request.id === id)).toBeUndefined();
    const sod = await decide(ann, bossItem);
    expect(sod.status()).toBe(403);
    expect((await sod.json()).code).toBe('sod_violation');

    // With Dana as delegate, she sees the waiting item, opens it and decides it on Boss's behalf.
    expect((await owner.c.put(`/api/v1/members/${boss.membership}/out-of-office`, { headers: owner.headers, data: await away(owner, dana) })).status()).toBe(200);
    const steps = (await detail(ann, id)).steps;
    expect(steps[0].away[0]).toMatchObject({ id: boss.membership, delegate: { id: dana.membership } });
    const item = (await inbox(dana)).find((w) => w.request.id === id)!;
    expect(item.on_behalf_of?.id).toBe(boss.membership);
    expect((await dana.c.get(`/api/v1/requests/${id}`, { headers: dana.headers })).status()).toBe(200);
    expect((await decide(dana, item)).status()).toBe(200);

    // Members cannot set someone else's; with self-service off they cannot set their own either.
    expect((await dana.c.put(`/api/v1/members/${boss.membership}/out-of-office`, { headers: dana.headers, data: await away(dana, ann) })).status()).toBe(403);
    expect((await setting(owner, 'access.delegation.self_service_ooo', false)).status()).toBe(200);
    expect((await dana.c.put('/api/v1/me/out-of-office', { headers: dana.headers, data: await away(dana, ann) })).status()).toBe(403);
    expect((await (await dana.c.get('/api/v1/me/out-of-office', { headers: dana.headers })).json()).can_set).toBe(false);
    const self = await boss.c.put('/api/v1/me/out-of-office', { headers: boss.headers, data: { ...(await away(boss, boss)) } });
    expect(self.status(), 'self-service is off').toBe(403);
    const invalid = await owner.c.put(`/api/v1/members/${boss.membership}/out-of-office`, { headers: owner.headers, data: await away(owner, boss) });
    expect(invalid.status()).toBe(422);
    expect((await invalid.json()).errors[0].pointer).toBe('/delegate');
    expect((await owner.c.delete(`/api/v1/members/${boss.membership}/out-of-office`, { headers: owner.headers })).status()).toBe(204);
    expect((await owner.c.delete(`/api/v1/members/${boss.membership}/out-of-office`, { headers: owner.headers })).status()).toBe(404);
  });
});
