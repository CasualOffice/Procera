import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-LIB-06..10 — semantic diff (BPMS-043) and library governance (BPMS-044): review cadence,
// overdue flag + owner notification, recertification, verification with expiry, change requests.

type Req = Parameters<typeof seedUser>[0];
const DAY = 86_400_000;

async function joinAs(request: Req, owner: Session, ws: string, tag: string, role: string, mfa = true) {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: [role] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });
  if (mfa) return enter(email, ws);
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: { authorization: `Bearer ${pw.token}` }, data: { workspace_id: ws } })).json();
  return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` } };
}

const p = (text: string) => ({ type: 'paragraph', text });

/** Creates and publishes a standard SOP; returns the artifact and its first version. */
async function publishedSop(owner: Session, key: string, blocks: object[]) {
  const h = owner.headers;
  const { artifact, version } = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key, title: `SOP ${key}`, content: { blocks } } })).json();
  await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h });
  expect((await (await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: h, data: {} })).json()).lifecycle).toBe('published');
  return { artifact, version };
}

async function governance(owner: Session, id: string) {
  return (await (await owner.c.get(`/api/v1/artifacts/${id}`, { headers: owner.headers })).json()).artifact.governance;
}

test.describe('library governance @r1 @library', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-LIB-06 semantic diff between versions: block changes with word-level detail; versions must share an artifact', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const { artifact, version } = await publishedSop(owner, 'sec.visitors', [p('Please wear a badge.'), { type: 'heading', level: 2, text: 'Visitors' }, p('Sign in at reception.')]);
    const v2 = await (await owner.c.post(`/api/v1/artifacts/${artifact.id}/versions`, { headers: h })).json();
    await owner.c.patch(`/api/v1/versions/${v2.id}`, { headers: h, data: { content: { blocks: [p('Please always wear a visible badge.'), { type: 'heading', level: 2, text: 'Visitors' }, p('Sign in at reception.'), { type: 'list', items: ['Escort visitors', 'Return badges'] }] } } });
    const d = await (await owner.c.get(`/api/v1/versions/${version.id}/diff/${v2.id}`, { headers: h })).json();
    expect(d.summary).toEqual({ added: 1, removed: 0, changed: 1 });
    expect([d.from.version_no, d.to.version_no]).toEqual([1, 2]);
    expect(d.ops.map((o: { op: string }) => o.op)).toEqual(['changed', 'same', 'same', 'added']);
    expect(d.ops[0].words).toEqual([{ op: 'same', text: 'Please' }, { op: 'added', text: 'always' }, { op: 'same', text: 'wear a' }, { op: 'added', text: 'visible' }, { op: 'same', text: 'badge.' }]);
    // Reverse direction is the mirror image.
    expect((await (await owner.c.get(`/api/v1/versions/${v2.id}/diff/${version.id}`, { headers: h })).json()).summary).toEqual({ added: 0, removed: 1, changed: 1 });
    const other = await publishedSop(owner, 'sec.other', [p('x')]);
    expect((await owner.c.get(`/api/v1/versions/${version.id}/diff/${other.version.id}`, { headers: h })).status()).toBe(422);
    // Readers may compare published history only (A-14): the draft is hidden until it is published.
    const member = await joinAs(request, owner, ws.id, 'diffreader', 'member', false);
    expect((await member.c.get(`/api/v1/versions/${version.id}/diff/${v2.id}`, { headers: member.headers })).status()).toBe(404);
    await owner.c.post(`/api/v1/versions/${v2.id}/submit`, { headers: h });
    await owner.c.post(`/api/v1/versions/${v2.id}/publish`, { headers: h, data: { change_reason: 'Escorts' } });
    expect((await member.c.get(`/api/v1/versions/${version.id}/diff/${v2.id}`, { headers: member.headers })).status()).toBe(200);
    const stranger = await workspaceWithOwner(request);
    expect((await stranger.owner.c.get(`/api/v1/versions/${version.id}/diff/${v2.id}`, { headers: stranger.owner.headers })).status()).toBe(404);
  });

  test('E2E-R1-LIB-07 publishing sets a review date from the cadence (365 d standard, override in days); unpublished artifacts cannot be recertified', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const draft = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'ops.draft', title: 'Draft only' } })).json();
    expect((await governance(owner, draft.artifact.id)).review_status).toBe('none');
    expect((await owner.c.post(`/api/v1/artifacts/${draft.artifact.id}/recertify`, { headers: h, data: {} })).status()).toBe(409);
    const { artifact } = await publishedSop(owner, 'ops.cadence', [p('Do it.')]);
    const g = await governance(owner, artifact.id);
    expect(g.review_status).toBe('current');
    expect(Date.parse(g.next_review_at) - Date.parse(g.last_reviewed_at)).toBe(365 * DAY);
    const set = await owner.c.put(`/api/v1/artifacts/${artifact.id}/review-cadence`, { headers: h, data: { days: 20 } });
    expect(set.status()).toBe(200);
    const g2 = (await set.json()).governance;
    expect([g2.review_cadence_days, Date.parse(g2.next_review_at) - Date.parse(g2.last_reviewed_at), g2.review_status]).toEqual([20, 20 * DAY, 'due_soon']);
    expect((await owner.c.put(`/api/v1/artifacts/${artifact.id}/review-cadence`, { headers: h, data: { days: 0 } })).status()).toBe(422);
    const reset = (await (await owner.c.put(`/api/v1/artifacts/${artifact.id}/review-cadence`, { headers: h, data: { days: null } })).json()).governance;
    expect(Date.parse(reset.next_review_at) - Date.parse(reset.last_reviewed_at)).toBe(365 * DAY);
  });

  test('E2E-R1-LIB-08 a passed review date is flagged once and notifies the owner; recertifying starts a new period', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const { artifact } = await publishedSop(owner, 'ops.review', [p('Check the doors.')]);
    expect((await request.post('/__test/library/backdate', { data: { workspace: ws.id, artifact: artifact.id, seconds: 366 * 86_400 } })).status()).toBe(204);
    const before = await governance(owner, artifact.id);
    expect([before.review_status, before.review_overdue_at]).toEqual(['overdue', null]);
    await request.post('/__test/drain');
    await request.post('/__test/drain');
    expect((await governance(owner, artifact.id)).review_overdue_at).not.toBeNull();
    const inbox = await (await owner.c.get('/api/v1/notifications', { headers: h })).json();
    const due = inbox.items.filter((n: { kind: string }) => n.kind === 'review_overdue');
    expect(due).toHaveLength(1);
    expect(due[0]).toMatchObject({ title: 'SOP ops.review is due for review', link: `/library/${artifact.id}` });
    const re = await owner.c.post(`/api/v1/artifacts/${artifact.id}/recertify`, { headers: h, data: { comment: 'Still accurate.' } });
    expect(re.status()).toBe(200);
    const after = (await re.json()).governance;
    expect([after.review_status, after.review_overdue_at]).toEqual(['current', null]);
    const timeline = await (await owner.c.get(`/api/v1/audit/objects/artifact/${artifact.id}/timeline`, { headers: h })).json();
    expect(timeline.items.map((e: { action: string }) => e.action)).toEqual(expect.arrayContaining(['artifact.review_overdue', 'artifact.recertified']));
  });

  test('E2E-R1-LIB-09 verification lasts the configured validity and expires on its own; members cannot verify', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const { artifact } = await publishedSop(owner, 'it.wifi', [p('Use the staff network.')]);
    const member = await joinAs(request, owner, ws.id, 'verifier', 'member', false);
    expect((await member.c.post(`/api/v1/artifacts/${artifact.id}/verification`, { headers: member.headers })).status()).toBe(403);
    const v = (await (await owner.c.post(`/api/v1/artifacts/${artifact.id}/verification`, { headers: h })).json()).governance;
    expect(v.verified).toBe(true);
    expect(Date.parse(v.verification_expires_at) - Date.parse(v.verified_at)).toBe(90 * DAY);
    await request.post('/__test/library/backdate', { data: { workspace: ws.id, artifact: artifact.id, seconds: 91 * 86_400 } });
    expect((await governance(owner, artifact.id)).verified).toBe(false);
    expect((await owner.c.delete(`/api/v1/artifacts/${artifact.id}/verification`, { headers: h })).status()).toBe(200);
    expect((await governance(owner, artifact.id)).verified_at).toBeNull();
    expect((await owner.c.delete(`/api/v1/artifacts/${artifact.id}/verification`, { headers: h })).status()).toBe(409);
  });

  test('E2E-R1-LIB-10 change requests: readers propose, publishers accept/decline (with reason) and link the implementing version', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const { artifact } = await publishedSop(owner, 'hr.expenses', [p('Submit receipts monthly.')]);
    const other = await publishedSop(owner, 'hr.other', [p('x')]);
    const member = await joinAs(request, owner, ws.id, 'proposer', 'member', false);
    const mh = member.headers;
    const opened = await member.c.post(`/api/v1/artifacts/${artifact.id}/change-requests`, { headers: mh, data: { title: 'Allow weekly submissions', body: 'Monthly is too slow for travel.' } });
    expect(opened.status()).toBe(201);
    const cr = await opened.json();
    expect(cr.state).toBe('open');
    expect((await member.c.post(`/api/v1/artifacts/${artifact.id}/change-requests`, { headers: mh, data: { title: ' ' } })).status()).toBe(422);
    const second = await (await member.c.post(`/api/v1/artifacts/${artifact.id}/change-requests`, { headers: mh, data: { title: 'Use a new tool' } })).json();
    expect((await member.c.post(`/api/v1/change-requests/${cr.id}/resolve`, { headers: mh, data: { decision: 'accepted' } })).status()).toBe(403);
    expect((await owner.c.post(`/api/v1/change-requests/${second.id}/resolve`, { headers: h, data: { decision: 'declined' } })).status()).toBe(422);
    expect((await (await owner.c.post(`/api/v1/change-requests/${second.id}/resolve`, { headers: h, data: { decision: 'declined', resolution: 'Out of scope.' } })).json()).state).toBe('declined');
    expect((await (await owner.c.post(`/api/v1/change-requests/${cr.id}/resolve`, { headers: h, data: { decision: 'accepted' } })).json()).state).toBe('accepted');
    const v2 = await (await owner.c.post(`/api/v1/artifacts/${artifact.id}/versions`, { headers: h })).json();
    expect((await owner.c.post(`/api/v1/change-requests/${cr.id}/resolve`, { headers: h, data: { decision: 'implemented', version_id: other.version.id } })).status()).toBe(422);
    const done = await (await owner.c.post(`/api/v1/change-requests/${cr.id}/resolve`, { headers: h, data: { decision: 'implemented', version_id: v2.id } })).json();
    expect([done.state, done.version_id]).toEqual(['implemented', v2.id]);
    expect((await owner.c.post(`/api/v1/change-requests/${cr.id}/resolve`, { headers: h, data: { decision: 'declined', resolution: 'late' } })).status()).toBe(409);
    const list = await (await member.c.get(`/api/v1/artifacts/${artifact.id}/change-requests`, { headers: mh })).json();
    expect(list.items.map((c: { state: string }) => c.state).sort()).toEqual(['declined', 'implemented']);
    expect((await (await member.c.get(`/api/v1/artifacts/${artifact.id}/change-requests?state=declined`, { headers: mh })).json()).items).toHaveLength(1);
  });
});
