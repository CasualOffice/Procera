import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-EA-01..02 — email-actionable approvals (BPMS-145, FR-N03): the manager's approval email for a
// leave request carries Approve/Reject links; the link's page reads without deciding, says who decides,
// and hides nothing it shouldn't show; rejecting needs a reason; approving decides once (the request
// is approved, the link is spent, a second use is 409); a step decided in My work closes its link;
// with the workspace setting off, emails carry no links.

async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const s = (await enter(email, ws)) as Session;
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, session: s, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

const key = () => `e2e-${Date.now()}-${Math.random().toString(36).slice(2)}`;

async function leave(ann: { session: Session }) {
  const r = await ann.session.c.post('/api/v1/requests', {
    headers: { ...ann.session.headers, 'idempotency-key': key() },
    data: { type: 'hr.leave-request', input: { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23' } },
  });
  expect(r.status(), await r.text()).toBe(201);
  return (await r.json()) as { id: string; display_id: string };
}

/** The token in the newest approval email to `to` about `ref`. */
async function emailToken(request: APIRequestContext, ws: string, to: string, ref: string): Promise<string | null> {
  let token: string | null = null;
  await expect.poll(async () => {
    await request.post('/__test/drain');
    const mail = await (await request.get(`/__test/mail?workspace=${ws}&to=${encodeURIComponent(to)}`)).json();
    const m = (mail.items as { kind: string; text: string }[]).find((x) => x.kind === 'work_assigned' && x.text.includes(ref));
    if (!m) return false;
    token = m.text.match(/\/approve\/([A-Za-z0-9_-]+)\?outcome=approved/)?.[1] ?? null;
    return true;
  }).toBe(true);
  return token;
}

test.describe('email approvals @r4 @email', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-EA-01 a manager approves from the email link once; the page reads without deciding', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const bo = await person(request, owner, ws.id, 'Bo Boss');
    const ann = await person(request, owner, ws.id, 'Ann Asker');
    await owner.c.patch(`/api/v1/members/${ann.id}`, { headers: owner.headers, data: { manager_membership_id: bo.id } });
    const req = await leave(ann);
    const token = await emailToken(request, ws.id, bo.email, req.display_id);
    expect(token).toMatch(/^[A-Za-z0-9_-]{43}$/);
    const anon = await client();
    // Reading decides nothing.
    const view = await (await anon.get(`/api/v1/email-actions/${token}`)).json();
    expect(view).toMatchObject({ state: 'open', deciding_as: 'Bo Boss', request: { display_id: req.display_id, initiator: 'Ann Asker' } });
    expect(view.answers.length).toBeGreaterThan(0);
    await anon.get(`/api/v1/email-actions/${token}`);
    expect((await (await ann.session.c.get(`/api/v1/requests/${req.id}`, { headers: ann.session.headers })).json()).status).toBe('in_progress');
    // Rejecting needs a reason; nonsense outcomes are refused.
    expect((await anon.post(`/api/v1/email-actions/${token}`, { data: { outcome: 'rejected' } })).status()).toBe(422);
    expect((await anon.post(`/api/v1/email-actions/${token}`, { data: { outcome: 'maybe' } })).status()).toBe(422);
    const ok = await anon.post(`/api/v1/email-actions/${token}`, { data: { outcome: 'approved' } });
    expect(ok.status(), await ok.text()).toBe(200);
    expect(await ok.json()).toMatchObject({ outcome: 'approved', display_id: req.display_id });
    await expect.poll(async () => (await (await ann.session.c.get(`/api/v1/requests/${req.id}`, { headers: ann.session.headers })).json()).status).toBe('approved');
    const detail = await (await ann.session.c.get(`/api/v1/requests/${req.id}`, { headers: ann.session.headers })).json();
    expect(detail.steps.find((s: { outcome: string | null }) => s.outcome === 'approved').decided_by).toBe('Bo Boss');
    // Spent: a second use fails, the page says so.
    expect((await anon.post(`/api/v1/email-actions/${token}`, { data: { outcome: 'approved' } })).status()).toBe(409);
    expect((await (await anon.get(`/api/v1/email-actions/${token}`)).json()).state).toBe('used');
    expect((await anon.get('/api/v1/email-actions/not-a-real-token')).status()).toBe(404);
  });

  test('E2E-R4-EA-02 a step decided in My work closes its link; with the setting off, emails carry no links', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const bo = await person(request, owner, ws.id, 'Bo Boss');
    const ann = await person(request, owner, ws.id, 'Ann Asker');
    await owner.c.patch(`/api/v1/members/${ann.id}`, { headers: owner.headers, data: { manager_membership_id: bo.id } });
    const req = await leave(ann);
    const token = await emailToken(request, ws.id, bo.email, req.display_id);
    const items = (await (await bo.session.c.get('/api/v1/work', { headers: bo.session.headers })).json()).items as { id: string; revision: number; request: { id: string } }[];
    const w = items.find((x) => x.request.id === req.id)!;
    expect((await bo.session.c.post(`/api/v1/work/${w.id}/decisions`, { headers: { ...bo.session.headers, 'if-match': `"${w.revision}"` }, data: { outcome: 'approved' } })).status()).toBe(200);
    const anon = await client();
    expect((await (await anon.get(`/api/v1/email-actions/${token}`)).json()).state).toBe('closed');
    expect((await anon.post(`/api/v1/email-actions/${token}`, { data: { outcome: 'approved' } })).status()).toBe(409);
    // Turn email approvals off: the next email has no links.
    const set = await owner.c.put('/api/v1/settings/notify.email_actions.enabled', { headers: owner.headers, data: { value: false } });
    expect(set.ok(), await set.text()).toBe(true);
    const req2 = await leave(ann);
    expect(await emailToken(request, ws.id, bo.email, req2.display_id)).toBeNull();
  });
});
