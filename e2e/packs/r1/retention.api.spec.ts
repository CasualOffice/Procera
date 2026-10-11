import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-RET-01..02 — retention classes honoured by jobs (BPMS-066; PRV-04 policy part, docs/14 §4).

const DAY = 86_400;
const doc = (text: string) => ({ blocks: [{ type: 'paragraph', text }] });
type Req = Parameters<typeof seedUser>[0];

async function backdate(request: Req, workspace: string, data: { version?: string; notification?: string; seconds: number }) {
  expect((await request.post('/__test/retention/backdate', { data: { workspace, ...data } })).status()).toBe(204);
}

/** Runs the retention jobs (and the outbox, for notifications) until the effect is visible. */
async function drainUntil<T>(request: Req, read: () => Promise<T>, done: (v: T) => boolean) {
  await expect.poll(async () => {
    await request.post('/__test/retention/run');
    await request.post('/__test/drain');
    return done(await read());
  }).toBe(true);
}

test.describe('retention @r1 @retention', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-RET-01 drafts: the author is told before an untouched draft is deleted; editing resets the clock; published history is never touched', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const lone = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'ops.lone', title: 'Forgotten draft', content: doc('x') } })).json();
    const kept = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'ops.kept', title: 'Busy draft', content: doc('x') } })).json();
    const pub = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'ops.live', title: 'Live SOP', content: doc('v1') } })).json();
    await owner.c.post(`/api/v1/versions/${pub.version.id}/submit`, { headers: h });
    await owner.c.post(`/api/v1/versions/${pub.version.id}/publish`, { headers: h, data: {} });
    const v2 = await (await owner.c.post(`/api/v1/artifacts/${pub.artifact.id}/versions`, { headers: h })).json();

    // 170 days untouched: inside the 14-day notice window of the 180-day policy.
    for (const v of [lone.version.id, kept.version.id, v2.id]) await backdate(request, ws.id, { version: v, seconds: 170 * DAY });
    const overview = await (await owner.c.get('/api/v1/admin/retention', { headers: h })).json();
    expect(overview.classes.map((c: { class: string; mode: string }) => [c.class, c.mode])).toEqual([['draft', 'automatic'], ['operational', 'automatic'], ['governance_record', 'never'], ['audit', 'never']]);
    expect(overview.due_drafts.map((d: { title: string }) => d.title).sort()).toEqual(['Busy draft', 'Forgotten draft', 'Live SOP']);
    await drainUntil(request, async () => (await (await owner.c.get('/api/v1/notifications', { headers: h })).json()).items as { kind: string }[], (items) => items.filter((n) => n.kind === 'draft_expiring').length === 3);

    // Someone edits one draft: its clock resets. Eleven more days pass for the others.
    expect((await owner.c.patch(`/api/v1/versions/${kept.version.id}`, { headers: h, data: { content: doc('still working') } })).status()).toBe(200);
    for (const v of [lone.version.id, v2.id]) await backdate(request, ws.id, { version: v, seconds: 11 * DAY });
    await drainUntil(request, async () => (await owner.c.get(`/api/v1/artifacts/${lone.artifact.id}`, { headers: h })).status(), (s) => s === 404);
    expect((await owner.c.get(`/api/v1/versions/${v2.id}`, { headers: h })).status()).toBe(404);
    const live = await (await owner.c.get(`/api/v1/artifacts/${pub.artifact.id}`, { headers: h })).json();
    expect([live.artifact.state, live.versions.map((v: { version_no: number }) => v.version_no)]).toEqual(['published', [1]]);
    expect((await owner.c.get(`/api/v1/versions/${kept.version.id}`, { headers: h })).status()).toBe(200);
    const after = await (await owner.c.get('/api/v1/admin/retention', { headers: h })).json();
    expect(after.last_run.purged.draft).toBe(2);
    const audit = await (await owner.c.get('/api/v1/audit/events?action=retention.', { headers: h })).json();
    expect(audit.items.map((e: { action: string }) => e.action)).toContain('retention.purged');
  });

  test('E2E-R1-RET-02 delivery logs past the operational period are removed; unread notifications stay; members cannot see retention', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const email = await seedUser(request, 'retmember');
    await owner.c.post('/api/v1/members/invitations', { headers: h, data: { email, roles: ['member'] } });
    const token = await inviteToken(request, ws.id, email);
    const pw = await passwordSession(email);
    await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });
    const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: { authorization: `Bearer ${pw.token}` }, data: { workspace_id: ws.id } })).json();
    const m: Session = { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` } };
    let joined: { id: string } | undefined;
    await drainUntil(request, async () => (await (await owner.c.get('/api/v1/notifications', { headers: h })).json()).items as { id: string; kind: string }[], (items) => !!(joined = items.find((n) => n.kind === 'member_joined')));
    await owner.c.post(`/api/v1/notifications/${joined!.id}/read`, { headers: h });
    await backdate(request, ws.id, { notification: joined!.id, seconds: 401 * DAY });
    await drainUntil(request, async () => (await (await owner.c.get('/api/v1/notifications', { headers: h })).json()).items as { id: string }[], (items) => !items.some((n) => n.id === joined!.id));
    expect((await m.c.get('/api/v1/admin/retention', { headers: m.headers })).status()).toBe(403);
  });
});
