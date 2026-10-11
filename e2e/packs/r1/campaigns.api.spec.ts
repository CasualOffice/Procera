import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-CMP-01..07 — acknowledgement campaigns: launch + notify, acknowledgement bound to the exact
// version hash, own-only / once, scheduled reminders and manager escalation, dynamic new
// joiners, evidence CSV, permissions and workspace isolation (BPMS-046; FR-AK01–04, FR-PL03).
//
// The test clock is shared by parallel tests and only moves forward (a jump also expires their step-up
// window), so time-based scenarios backdate their own campaign and only assert what did happen.

type Req = Parameters<typeof seedUser>[0];
type Member = Session & { email: string; membership: string };

/** Invites `role`, accepts with a password session and selects the workspace (no MFA needed for these roles). */
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

const doc = (text: string) => ({ blocks: [{ type: 'paragraph', text }] });

/** Creates and publishes a standard policy (no review needed in the Standard profile). */
async function publishedPolicy(owner: Session, key: string) {
  const h = owner.headers;
  const { version } = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key, title: `Policy ${key}`, content: doc('Lock your screen.') } })).json();
  await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h });
  const pub = await (await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: h, data: {} })).json();
  expect(pub.lifecycle).toBe('published');
  return { versionId: pub.id as string, hash: pub.content_hash as string };
}

async function department(owner: Session, name: string, members: Member[]) {
  const d = await (await owner.c.post('/api/v1/departments', { headers: owner.headers, data: { name } })).json();
  for (const m of members) {
    expect((await owner.c.patch(`/api/v1/members/${m.membership}`, { headers: owner.headers, data: { department_id: d.id } })).status()).toBe(200);
  }
  return d.id as string;
}

async function launch(owner: Session, data: Record<string, unknown>) {
  const created = await owner.c.post('/api/v1/campaigns', { headers: owner.headers, data });
  expect(created.status(), await created.text()).toBe(201);
  const draft = await created.json();
  const act = await owner.c.post(`/api/v1/campaigns/${draft.id}/activate`, { headers: owner.headers });
  expect(act.status(), await act.text()).toBe(200);
  return act.json();
}

async function pending(m: Session) {
  return (await (await m.c.get('/api/v1/me/acknowledgements?state=pending', { headers: m.headers })).json()).items as Array<{ id: string; campaign_id: string; subject: { content_hash: string; version_id: string }; due_at: string; assigned_at: string }>;
}

async function notificationKinds(m: Session) {
  return ((await (await m.c.get('/api/v1/notifications?limit=200', { headers: m.headers })).json()).items as Array<{ kind: string }>).map((n) => n.kind);
}

async function mailKinds(request: Req, ws: string, to: string) {
  return ((await (await request.get(`/__test/mail?workspace=${ws}&to=${encodeURIComponent(to)}`)).json()).items as Array<{ kind: string; text: string }>);
}

test.describe('campaigns @r1 @campaigns', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-CMP-01 launching a campaign snapshots the audience and notifies each assignee in-app and by email', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [ann, ben, out] = [await joinAs(request, owner, ws.id, 'ann'), await joinAs(request, owner, ws.id, 'ben'), await joinAs(request, owner, ws.id, 'out')];
    const ops = await department(owner, 'Ops', [ann, ben]);
    const { versionId, hash } = await publishedPolicy(owner, 'sec.screens');
    const draft = await (await owner.c.post('/api/v1/campaigns', { headers: owner.headers, data: { version_id: versionId, audience: { departments: [ops] } } })).json();
    expect([draft.state, draft.title, draft.audience_snapshot, draft.subject.content_hash]).toEqual(['draft', 'Policy sec.screens', 'dynamic', hash]);
    expect(draft.reminder_schedule).toEqual([3 * 86400, 7 * 86400]);
    expect((await (await owner.c.get(`/api/v1/campaigns/${draft.id}`, { headers: owner.headers })).json()).audience_preview).toBe(2);
    const act = await (await owner.c.post(`/api/v1/campaigns/${draft.id}/activate`, { headers: owner.headers })).json();
    expect([act.state, act.progress.assigned, act.progress.acknowledged]).toEqual(['active', 2, 0]);
    expect(Date.parse(act.due_at) - Date.parse(act.launched_at)).toBe(14 * 86400 * 1000);
    expect((await owner.c.post(`/api/v1/campaigns/${draft.id}/activate`, { headers: owner.headers })).status()).toBe(409);

    const mine = await pending(ann);
    expect(mine).toHaveLength(1);
    expect([mine[0].campaign_id, mine[0].subject.version_id, mine[0].subject.content_hash]).toEqual([draft.id, versionId, hash]);
    expect(await notificationKinds(ann)).toContain('campaign_assigned');
    const mail = (await mailKinds(request, ws.id, ann.email)).find((m) => m.kind === 'campaign_assigned');
    expect(mail?.text).toContain(`/acknowledgements/${mine[0].id}`);
    expect(await pending(out)).toHaveLength(0);
    expect(await notificationKinds(out)).not.toContain('campaign_assigned');
    const list = await (await owner.c.get('/api/v1/campaigns?state=active', { headers: owner.headers })).json();
    expect(list.items.map((c: { id: string }) => c.id)).toContain(draft.id);
  });

  test('E2E-R1-CMP-02 acknowledging records the exact version id and content hash; a stale hash is refused', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [ann, ben] = [await joinAs(request, owner, ws.id, 'ann2'), await joinAs(request, owner, ws.id, 'ben2')];
    const ops = await department(owner, 'Ops', [ann, ben]);
    const { versionId, hash } = await publishedPolicy(owner, 'sec.mfa');
    const c = await launch(owner, { version_id: versionId, audience: { departments: [ops] }, ack_text: 'I will use MFA.' });
    const [a] = await pending(ann);
    const ack = await ann.c.post(`/api/v1/acknowledgements/${a.id}/complete`, { headers: { ...ann.headers, 'x-forwarded-for': '203.0.113.9' }, data: { content_hash: hash } });
    expect(ack.status(), await ack.text()).toBe(200);
    const done = await ack.json();
    expect([done.state, done.acknowledged_version_id, done.acknowledged_content_hash, done.ack_text]).toEqual(['completed', versionId, hash, 'I will use MFA.']);
    expect(await pending(ann)).toHaveLength(0);
    const completed = await (await ann.c.get('/api/v1/me/acknowledgements?state=completed', { headers: ann.headers })).json();
    expect(completed.items[0].acknowledged_at).toBeTruthy();

    const [b] = await pending(ben);
    const stale = await ben.c.post(`/api/v1/acknowledgements/${b.id}/complete`, { headers: ben.headers, data: { content_hash: '0'.repeat(64) } });
    expect([stale.status(), (await stale.json()).code]).toEqual([409, 'content_changed']);
    const progress = (await (await owner.c.get(`/api/v1/campaigns/${c.id}`, { headers: owner.headers })).json()).progress;
    expect([progress.assigned, progress.acknowledged, progress.pending]).toEqual([2, 1, 1]);
  });

  test('E2E-R1-CMP-03 only the assignee can acknowledge, only once, and only while the campaign is active', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [ann, ben] = [await joinAs(request, owner, ws.id, 'ann3'), await joinAs(request, owner, ws.id, 'ben3')];
    const ops = await department(owner, 'Ops', [ann, ben]);
    const { versionId } = await publishedPolicy(owner, 'hr.conduct');
    const c = await launch(owner, { version_id: versionId, audience: { departments: [ops] } });
    const [a] = await pending(ann);
    const [b] = await pending(ben);
    expect((await ben.c.post(`/api/v1/acknowledgements/${a.id}/complete`, { headers: ben.headers, data: {} })).status()).toBe(404);
    expect((await owner.c.post(`/api/v1/acknowledgements/${a.id}/complete`, { headers: owner.headers, data: {} })).status()).toBe(404);
    expect((await ann.c.post(`/api/v1/acknowledgements/${a.id}/complete`, { headers: ann.headers, data: {} })).status()).toBe(200);
    const again = await ann.c.post(`/api/v1/acknowledgements/${a.id}/complete`, { headers: ann.headers, data: {} });
    expect([again.status(), (await again.json()).code]).toEqual([409, 'already_acknowledged']);

    expect((await owner.c.post(`/api/v1/campaigns/${c.id}/cancel`, { headers: owner.headers, data: {} })).status()).toBe(422);
    const closed = await (await owner.c.post(`/api/v1/campaigns/${c.id}/close`, { headers: owner.headers, data: { reason: 'Audit period ended' } })).json();
    expect([closed.state, closed.end_reason, closed.progress.pending]).toEqual(['closed', 'Audit period ended', 1]);
    expect((await owner.c.post(`/api/v1/campaigns/${c.id}/close`, { headers: owner.headers, data: {} })).status()).toBe(409);
    expect((await owner.c.post(`/api/v1/campaigns/${c.id}/remind`, { headers: owner.headers })).status()).toBe(409);
    const late = await ben.c.post(`/api/v1/acknowledgements/${b.id}/complete`, { headers: ben.headers, data: {} });
    expect([late.status(), (await late.json()).code]).toEqual([409, 'state_conflict']);
    expect(await pending(ben)).toHaveLength(0);
  });

  test('E2E-R1-CMP-04 reminders follow the ladder and an open assignment escalates to the manager (scheduler via drain)', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [ben, boss] = [await joinAs(request, owner, ws.id, 'ben4'), await joinAs(request, owner, ws.id, 'boss4')];
    const ops = await department(owner, 'Ops', [ben]);
    await owner.c.patch(`/api/v1/members/${ben.membership}`, { headers: owner.headers, data: { manager_membership_id: boss.membership } });
    const { versionId } = await publishedPolicy(owner, 'ops.safety');
    const c = await launch(owner, { version_id: versionId, audience: { departments: [ops] } });
    const reminded = await (await owner.c.post(`/api/v1/campaigns/${c.id}/remind`, { headers: owner.headers })).json();
    expect(reminded.reminded).toBe(1);

    // 15 days later: past both ladder steps (+3 d, +7 d) and the escalation (+14 d). The campaign is
    // moved into the past instead of moving the shared clock, which would expire the step-up window of
    // every test running in parallel. A missed ladder sends a single reminder.
    expect((await request.post('/__test/campaigns/backdate', { data: { workspace: ws.id, campaign: c.id, seconds: 15 * 86400 } })).status()).toBe(204);
    // Drain is global: a drain started by a parallel test may pick up these reminders and still be
    // delivering them when ours returns, so drain until delivery is visible.
    await expect.poll(async () => {
      await request.post('/__test/drain');
      return (await notificationKinds(ben)).filter((k) => k === 'campaign_reminder').length;
    }).toBeGreaterThanOrEqual(2);
    await expect.poll(async () => {
      await request.post('/__test/drain');
      return notificationKinds(boss);
    }).toContain('campaign_escalated');
    expect((await mailKinds(request, ws.id, ben.email)).filter((m) => m.kind === 'campaign_reminder').length).toBeGreaterThanOrEqual(2);
    expect((await mailKinds(request, ws.id, boss.email)).map((m) => m.kind)).toContain('campaign_escalated');
    const overdue = (await (await owner.c.get(`/api/v1/campaigns/${c.id}/recipients?state=overdue`, { headers: owner.headers })).json()).items;
    expect(overdue).toHaveLength(1);
    expect([overdue[0].membership_id, overdue[0].escalated_to, overdue[0].reminders_sent, overdue[0].overdue]).toEqual([ben.membership, boss.membership, 2, true]);
    expect((await (await owner.c.get(`/api/v1/campaigns/${c.id}`, { headers: owner.headers })).json()).progress.overdue).toBe(1);
    // Overdue assignments can still be acknowledged while the campaign is active.
    const [b] = await pending(ben);
    expect((await ben.c.post(`/api/v1/acknowledgements/${b.id}/complete`, { headers: ben.headers, data: {} })).status()).toBe(200);
  });

  test('E2E-R1-CMP-05 a new member matching a dynamic audience is enrolled; at-launch campaigns stay fixed', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const ann = await joinAs(request, owner, ws.id, 'ann5');
    const { versionId } = await publishedPolicy(owner, 'it.acceptable-use');
    const dynamic = await launch(owner, { version_id: versionId, audience: { roles: ['member'] }, audience_snapshot: 'dynamic', title: 'AUP (dynamic)' });
    const fixed = await launch(owner, { version_id: versionId, audience: { roles: ['member'] }, audience_snapshot: 'at_launch', title: 'AUP (fixed)' });
    expect([dynamic.progress.assigned, fixed.progress.assigned]).toEqual([1, 1]);
    expect((await pending(ann)).length).toBe(2);

    const cy = await joinAs(request, owner, ws.id, 'cy5');
    await request.post('/__test/drain');
    const cyOpen = await pending(cy);
    expect(cyOpen.map((a) => a.campaign_id)).toEqual([dynamic.id]);
    expect(Date.parse(cyOpen[0].due_at) - Date.parse(cyOpen[0].assigned_at)).toBe(14 * 86400 * 1000);
    expect(await notificationKinds(cy)).toContain('campaign_assigned');
    const recips = (await (await owner.c.get(`/api/v1/campaigns/${dynamic.id}/recipients`, { headers: owner.headers })).json()).items;
    expect(recips.find((r: { membership_id: string }) => r.membership_id === cy.membership).source).toBe('joiner');
    expect((await (await owner.c.get(`/api/v1/campaigns/${fixed.id}`, { headers: owner.headers })).json()).progress.assigned).toBe(1);
  });

  test('E2E-R1-CMP-06 evidence export lists every assignee with department, timestamps, version and content hash', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [ann, ben] = [await joinAs(request, owner, ws.id, 'ann6'), await joinAs(request, owner, ws.id, 'ben6')];
    const ops = await department(owner, 'Operations', [ann, ben]);
    const { hash, versionId } = await publishedPolicy(owner, 'fin.expenses');
    const c = await launch(owner, { version_id: versionId, audience: { departments: [ops] } });
    const [a] = await pending(ann);
    await ann.c.post(`/api/v1/acknowledgements/${a.id}/complete`, { headers: ann.headers, data: { content_hash: hash } });
    const res = await owner.c.get(`/api/v1/campaigns/${c.id}/evidence`, { headers: owner.headers });
    expect(res.status()).toBe(200);
    expect(res.headers()['content-type']).toContain('text/csv');
    expect(res.headers()['content-disposition']).toContain(`campaign-${c.id}-evidence.csv`);
    const lines = (await res.text()).trim().split('\n');
    expect(lines[0]).toBe('member,email,department,state,assigned_at,due_at,acknowledged_at,version,version_id,content_hash,auth_method');
    expect(lines).toHaveLength(3);
    const annRow = lines.find((l) => l.includes(ann.email))!.split(',');
    expect([annRow[2], annRow[3], annRow[7], annRow[8], annRow[9], annRow[10]]).toEqual(['Operations', 'completed', '1', versionId, hash, 'pwd']);
    expect(Date.parse(annRow[6])).toBeGreaterThan(0);
    const benRow = lines.find((l) => l.includes(ben.email))!.split(',');
    expect([benRow[3], benRow[6], benRow[9]]).toEqual(['pending', '', '']);
    expect((await ann.c.get(`/api/v1/campaigns/${c.id}/evidence`, { headers: ann.headers })).status()).toBe(403);
  });

  test('E2E-R1-CMP-07 permissions: members cannot manage or track campaigns; compliance officers can; other workspaces see nothing', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const ann = await joinAs(request, owner, ws.id, 'ann7');
    const co = await joinAs(request, owner, ws.id, 'co7', 'compliance.officer');
    const { versionId } = await publishedPolicy(owner, 'gov.ethics');
    const data = { version_id: versionId, audience: { all: true } };
    expect((await ann.c.post('/api/v1/campaigns', { headers: ann.headers, data })).status()).toBe(403);
    expect((await ann.c.get('/api/v1/campaigns', { headers: ann.headers })).status()).toBe(403);
    const c = await launch(co, data);
    expect(c.progress.assigned).toBeGreaterThanOrEqual(3);
    expect((await ann.c.get(`/api/v1/campaigns/${c.id}`, { headers: ann.headers })).status()).toBe(403);
    expect((await ann.c.post(`/api/v1/campaigns/${c.id}/close`, { headers: ann.headers, data: {} })).status()).toBe(403);
    expect((await ann.c.get(`/api/v1/campaigns/${c.id}/recipients`, { headers: ann.headers })).status()).toBe(403);
    // Validation: a draft version cannot be acknowledged; the audience cannot be empty.
    const { version: draftVersion } = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'ops.draft', title: 'Draft SOP', content: doc('wip') } })).json();
    expect((await co.c.post('/api/v1/campaigns', { headers: co.headers, data: { version_id: draftVersion.id, audience: { all: true } } })).status()).toBe(409);
    expect((await co.c.post('/api/v1/campaigns', { headers: co.headers, data: { version_id: versionId, audience: {} } })).status()).toBe(422);
    // Draft campaigns can be cancelled with a reason.
    const d2 = await (await co.c.post('/api/v1/campaigns', { headers: co.headers, data })).json();
    expect((await (await co.c.post(`/api/v1/campaigns/${d2.id}/cancel`, { headers: co.headers, data: { reason: 'duplicate' } })).json()).state).toBe('cancelled');

    // Isolation: another workspace's owner gets 404 for this campaign and its assignments.
    const other = await workspaceWithOwner(request);
    const [a] = await pending(ann);
    expect((await other.owner.c.get(`/api/v1/campaigns/${c.id}`, { headers: other.owner.headers })).status()).toBe(404);
    expect((await other.owner.c.post(`/api/v1/campaigns/${c.id}/activate`, { headers: other.owner.headers })).status()).toBe(404);
    expect((await other.owner.c.post(`/api/v1/acknowledgements/${a.id}/complete`, { headers: other.owner.headers, data: {} })).status()).toBe(404);
    expect((await other.owner.c.get(`/api/v1/campaigns/${c.id}/evidence`, { headers: other.owner.headers })).status()).toBe(404);
  });
});
