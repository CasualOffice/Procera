import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser, serverNow } from '../../fixtures/accounts';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-ANN-01..06 — announcements: targeting snapshot, approval with SoD, scheduling, expiry,
// required acknowledgement, analytics and permissions (BPMS-047; FR-AN01–AN06, FX-C01).

type Req = Parameters<typeof seedUser>[0];
type Member = Session & { membership: string; email: string };

/** Joins `ws` as a Member (password session, no MFA needed), optionally granted a custom role. */
async function join(request: Req, owner: Session, ws: string, tag: string, role?: string): Promise<Member> {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const auth = { authorization: `Bearer ${pw.token}` };
  const { membership_id } = await (await pw.c.post('/api/v1/invitations/accept', { headers: auth, data: { token } })).json();
  if (role) {
    const g = await owner.c.post('/api/v1/role-grants', { headers: owner.headers, data: { membership_id, role_key: role } });
    expect(g.status(), await g.text()).toBe(201);
  }
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: auth, data: { workspace_id: ws } })).json();
  return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` }, membership: membership_id, email };
}

async function roles(owner: Session) {
  for (const [key, permissions] of [
    ['ann.author', ['announcement:view', 'announcement:create']],
    ['ann.publisher', ['announcement:view', 'announcement:publish']],
  ] as const) {
    const r = await owner.c.post('/api/v1/roles', { headers: owner.headers, data: { key, name: key, permissions } });
    expect(r.status(), await r.text()).toBe(201);
  }
}

async function department(owner: Session, name: string, members: string[]) {
  const d = await (await owner.c.post('/api/v1/departments', { headers: owner.headers, data: { name } })).json();
  for (const m of members) await owner.c.patch(`/api/v1/members/${m}`, { headers: owner.headers, data: { department_id: d.id } });
  return d.id as string;
}

const body = (text: string) => ({ blocks: [{ type: 'paragraph', text }] });
const ids = async (s: Session, q = '') =>
  ((await (await s.c.get(`/api/v1/announcements/feed${q}`, { headers: s.headers })).json()).items as { id: string }[]).map((i) => i.id);

test.describe('announcements @r1 @announcements', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-ANN-01 compose → publish to a department: only its members see it, are notified in-app and by email, and it is frozen', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const alice = await join(request, owner, ws.id, 'alice');
    const bob = await join(request, owner, ws.id, 'bob');
    const ops = await department(owner, 'Operations', [alice.membership]);
    const created = await owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'Office move', content: body('We move on Monday.'), audience: { departments: [ops] } } });
    expect(created.status(), await created.text()).toBe(201);
    const a = await created.json();
    expect([a.state, a.require_ack, a.priority]).toEqual(['draft', false, 'normal']);
    // Default expiry: 30 days after publication.
    expect(Date.parse(a.expires_at) - (await serverNow(owner.c))).toBeGreaterThan(29 * 86_400_000);
    expect((await owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'Nobody', audience: {} } })).status()).toBe(422);
    const edited = await owner.c.patch(`/api/v1/announcements/${a.id}`, { headers: { ...h, 'if-match': `"${a.revision}"` }, data: { title: 'Office move on Monday', content: body('We move on Monday.'), audience: { departments: [ops] }, pinned: true } });
    expect(edited.status(), await edited.text()).toBe(200);
    expect((await owner.c.patch(`/api/v1/announcements/${a.id}`, { headers: { ...h, 'if-match': `"${a.revision}"` }, data: { title: 'x', audience: { all: true } } })).status()).toBe(412);
    // Targeted (not workspace-wide): no approval under the default policy.
    const pub = await (await owner.c.post(`/api/v1/announcements/${a.id}/publish`, { headers: h })).json();
    expect([pub.state, pub.recipient_count, pub.content_hash]).toEqual(['published', 1, expect.stringMatching(/^[0-9a-f]{64}$/)]);
    expect((await owner.c.patch(`/api/v1/announcements/${a.id}`, { headers: h, data: { title: 'Changed', audience: { all: true } } })).status()).toBe(409);
    expect(await ids(alice)).toEqual([a.id]);
    expect(await ids(bob)).toEqual([]);
    expect((await bob.c.get(`/api/v1/announcements/${a.id}`, { headers: bob.headers })).status()).toBe(404);
    expect((await bob.c.post(`/api/v1/announcements/${a.id}/reads`, { headers: bob.headers })).status()).toBe(404);
    expect((await alice.c.get(`/api/v1/announcements/${a.id}`, { headers: alice.headers })).status()).toBe(200);
    const read = await (await alice.c.post(`/api/v1/announcements/${a.id}/reads`, { headers: alice.headers })).json();
    expect(read.read_at).toBeTruthy();
    expect(await ids(alice, '?unread=true')).toEqual([]);
    // Delivery after commit: in-app and email (announcements.channels default), recipients only.
    await request.post('/__test/drain');
    const inbox = await (await alice.c.get('/api/v1/notifications', { headers: alice.headers })).json();
    expect(inbox.items.map((n: { kind: string; ref_id: string }) => [n.kind, n.ref_id])).toContainEqual(['announcement', a.id]);
    const mail = await (await request.get(`/__test/mail?workspace=${ws.id}&to=${encodeURIComponent(alice.email)}`)).json();
    expect(mail.items.map((m: { kind: string; subject: string }) => [m.kind, m.subject])).toContainEqual(['announcement', 'Office move on Monday']);
    const bobMail = await (await request.get(`/__test/mail?workspace=${ws.id}&to=${encodeURIComponent(bob.email)}`)).json();
    expect(bobMail.items.filter((m: { kind: string }) => m.kind === 'announcement')).toEqual([]);
  });

  test('E2E-R1-ANN-02 workspace-wide posts need approval; authors never approve their own; rejection returns the draft', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    await roles(owner);
    const author = await join(request, owner, ws.id, 'author', 'ann.author');
    const approver = await join(request, owner, ws.id, 'approver', 'ann.publisher');
    // The owner (announcement:manage) cannot bypass approval or approve their own.
    const own = await (await owner.c.post('/api/v1/announcements', { headers: owner.headers, data: { title: 'All hands', audience: { all: true } } })).json();
    const direct = await owner.c.post(`/api/v1/announcements/${own.id}/publish`, { headers: owner.headers });
    expect([direct.status(), (await direct.json()).code]).toEqual([409, 'state_conflict']);
    const sub = await (await owner.c.post(`/api/v1/announcements/${own.id}/submit`, { headers: owner.headers })).json();
    expect([sub.state, sub.approval_required]).toEqual(['pending_approval', true]);
    const self = await owner.c.post(`/api/v1/announcements/${own.id}/approve`, { headers: owner.headers });
    expect([self.status(), (await self.json()).code]).toEqual([403, 'sod_violation']);
    expect((await (await approver.c.post(`/api/v1/announcements/${own.id}/approve`, { headers: approver.headers })).json()).state).toBe('published');

    // An author without publish rights submits; the approver rejects with a comment, then approves.
    const a = await (await author.c.post('/api/v1/announcements', { headers: author.headers, data: { title: 'Holiday calendar', content: body('Dates TBC.'), audience: { all: true } } })).json();
    expect((await author.c.post(`/api/v1/announcements/${a.id}/publish`, { headers: author.headers })).status()).toBe(403);
    expect((await approver.c.get(`/api/v1/announcements/${a.id}`, { headers: approver.headers })).status()).toBe(404); // drafts stay private
    await author.c.post(`/api/v1/announcements/${a.id}/submit`, { headers: author.headers });
    expect((await author.c.post(`/api/v1/announcements/${a.id}/approve`, { headers: author.headers })).status()).toBe(403);
    const queue = await (await approver.c.get('/api/v1/announcements?state=pending_approval', { headers: approver.headers })).json();
    expect(queue.items.map((x: { id: string }) => x.id)).toEqual([a.id]);
    expect((await approver.c.post(`/api/v1/announcements/${a.id}/reject`, { headers: approver.headers, data: { comment: ' ' } })).status()).toBe(422);
    const back = await (await approver.c.post(`/api/v1/announcements/${a.id}/reject`, { headers: approver.headers, data: { comment: 'Add the dates.' } })).json();
    expect([back.state, back.decision_comment]).toEqual(['draft', 'Add the dates.']);
    const fixed = await author.c.patch(`/api/v1/announcements/${a.id}`, { headers: { ...author.headers, 'if-match': `"${back.revision}"` }, data: { title: 'Holiday calendar', content: body('25 Dec, 1 Jan.'), audience: { all: true } } });
    expect(fixed.status(), await fixed.text()).toBe(200);
    await author.c.post(`/api/v1/announcements/${a.id}/submit`, { headers: author.headers });
    const ok = await (await approver.c.post(`/api/v1/announcements/${a.id}/approve`, { headers: approver.headers })).json();
    expect([ok.state, ok.decided_by, ok.recipient_count]).toEqual(['published', approver.membership, 3]);
    expect(await ids(author)).toEqual(expect.arrayContaining([a.id, own.id]));
  });

  test('E2E-R1-ANN-03 scheduled announcements go live when due; expired ones leave the feed but are kept', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const alice = await join(request, owner, ws.id, 'alice3');
    const grp = await (await owner.c.post('/api/v1/groups', { headers: h, data: { name: 'Night shift' } })).json();
    await owner.c.post(`/api/v1/groups/${grp.id}/members`, { headers: h, data: { membership_id: alice.membership } });
    const now = await serverNow(owner.c);
    const later = await (await owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'Fire drill', audience: { groups: [grp.id] }, publish_at: new Date(now + 30_000).toISOString() } })).json();
    const sch = await (await owner.c.post(`/api/v1/announcements/${later.id}/publish`, { headers: h })).json();
    expect(sch.state).toBe('scheduled');
    const brief = await (await owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'Lunch today', audience: { groups: [grp.id] }, expires_at: new Date(now + 45_000).toISOString() } })).json();
    await owner.c.post(`/api/v1/announcements/${brief.id}/publish`, { headers: h });
    expect(await ids(alice)).toEqual([brief.id]);
    await request.post('/__test/clock', { data: { advance: 'PT1M' } });
    await request.post('/__test/drain');
    expect(await ids(alice)).toEqual([later.id]);
    const live = await (await owner.c.get(`/api/v1/announcements/${later.id}`, { headers: h })).json();
    expect([live.state, live.published_by, live.recipient_count]).toEqual(['published', null, 1]);
    const gone = await (await owner.c.get(`/api/v1/announcements/${brief.id}`, { headers: h })).json();
    expect([gone.state, gone.expired]).toEqual(['published', true]);
    expect((await alice.c.post(`/api/v1/announcements/${brief.id}/reads`, { headers: alice.headers })).status()).toBe(404);
  });

  test('E2E-R1-ANN-04 required acknowledgement records the receipt; analytics reconcile; archive withdraws with a reason', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    await roles(owner);
    const alice = await join(request, owner, ws.id, 'alice4');
    const carol = await join(request, owner, ws.id, 'carol4');
    const publisher = await join(request, owner, ws.id, 'pub4', 'ann.publisher');
    const hq = await department(owner, 'HQ', [alice.membership, carol.membership]);
    const a = await (await owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'Badge policy change', audience: { departments: [hq] }, require_ack: true, priority: 'critical' } })).json();
    expect((await (await owner.c.post(`/api/v1/announcements/${a.id}/publish`, { headers: h })).json()).channels).toEqual(['email', 'in_app']);
    const ack = await alice.c.post(`/api/v1/announcements/${a.id}/acks`, { headers: { ...alice.headers, 'user-agent': 'ann-e2e-client' } });
    expect(ack.status(), await ack.text()).toBe(200);
    const r = await ack.json();
    expect(r.acked_at).toBeTruthy();
    expect(r.read_at).toBe(r.acked_at);
    expect((await (await alice.c.post(`/api/v1/announcements/${a.id}/acks`, { headers: alice.headers })).json()).acked_at).toBe(r.acked_at);
    await carol.c.post(`/api/v1/announcements/${a.id}/reads`, { headers: carol.headers });
    const stats = await (await owner.c.get(`/api/v1/announcements/${a.id}/analytics`, { headers: h })).json();
    expect([stats.recipients, stats.read, stats.acknowledged, stats.pending_ack]).toEqual([2, 2, 1, 1]);
    expect(stats.by_department).toEqual([{ department_id: hq, department: 'HQ', recipients: 2, read: 2, acknowledged: 1 }]);
    expect((await publisher.c.get(`/api/v1/announcements/${a.id}/analytics`, { headers: publisher.headers })).status()).toBe(403);
    // Acknowledging an announcement that does not ask for it is refused.
    const info = await (await owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'FYI', audience: { departments: [hq] } } })).json();
    await owner.c.post(`/api/v1/announcements/${info.id}/publish`, { headers: h });
    expect((await carol.c.post(`/api/v1/announcements/${info.id}/acks`, { headers: carol.headers })).status()).toBe(409);
    // Withdraw: a reason is required; it leaves the feed; analytics stay.
    expect((await publisher.c.post(`/api/v1/announcements/${a.id}/archive`, { headers: publisher.headers, data: { reason: ' ' } })).status()).toBe(422);
    const arch = await (await publisher.c.post(`/api/v1/announcements/${a.id}/archive`, { headers: publisher.headers, data: { reason: 'Superseded by v2' } })).json();
    expect([arch.state, arch.archive_reason]).toEqual(['archived', 'Superseded by v2']);
    expect(await ids(carol)).toEqual([info.id]);
    expect((await (await owner.c.get(`/api/v1/announcements/${a.id}/analytics`, { headers: h })).json()).acknowledged).toBe(1);
    expect((await alice.c.post(`/api/v1/announcements/${a.id}/acks`, { headers: alice.headers })).status()).toBe(404);
  });

  test('E2E-R1-ANN-05 permissions and isolation: members only read their feed; authors see only their own drafts; other workspaces get 404', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    await roles(owner);
    const member = await join(request, owner, ws.id, 'plain');
    const author = await join(request, owner, ws.id, 'author5', 'ann.author');
    expect((await member.c.post('/api/v1/announcements', { headers: member.headers, data: { title: 'Mine', audience: { all: true } } })).status()).toBe(403);
    expect((await member.c.get('/api/v1/announcements', { headers: member.headers })).status()).toBe(403);
    expect((await member.c.get('/api/v1/announcements/feed', { headers: member.headers })).status()).toBe(200);
    const draft = await (await owner.c.post('/api/v1/announcements', { headers: owner.headers, data: { title: 'Secret draft', audience: { all: true } } })).json();
    expect((await author.c.get(`/api/v1/announcements/${draft.id}`, { headers: author.headers })).status()).toBe(404);
    expect((await author.c.patch(`/api/v1/announcements/${draft.id}`, { headers: author.headers, data: { title: 'Mine now', audience: { all: true } } })).status()).toBe(404);
    expect((await member.c.get(`/api/v1/announcements/${draft.id}`, { headers: member.headers })).status()).toBe(404);
    const mine = await (await author.c.post('/api/v1/announcements', { headers: author.headers, data: { title: 'Team lunch', audience: { roles: ['member'] } } })).json();
    const list = await (await author.c.get('/api/v1/announcements', { headers: author.headers })).json();
    expect(list.items.map((x: { id: string }) => x.id)).toEqual([mine.id]);
    expect((await author.c.get(`/api/v1/announcements/${mine.id}/analytics`, { headers: author.headers })).status()).toBe(200);
    const other = await workspaceWithOwner(request);
    expect((await other.owner.c.get(`/api/v1/announcements/${draft.id}`, { headers: other.owner.headers })).status()).toBe(404);
    expect((await other.owner.c.post(`/api/v1/announcements/${draft.id}/submit`, { headers: other.owner.headers })).status()).toBe(404);
  });

  test('E2E-R1-ANN-06 workspace settings drive approval, acknowledgement default and expiry default', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const alice = await join(request, owner, ws.id, 'alice6');
    const put = async (key: string, value: unknown) => {
      const r = await owner.c.put(`/api/v1/settings/${key}`, { headers: h, data: { value } });
      expect(r.status(), await r.text()).toBe(200);
    };
    await put('announcements.approval_required', 'off');
    await put('announcements.require_ack.default', true);
    await put('announcements.expiry.default', 7 * 86_400);
    const wide = await (await owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'Everyone, no approval', audience: { all: true } } })).json();
    expect(wide.require_ack).toBe(true);
    expect(Date.parse(wide.expires_at) - (await serverNow(owner.c))).toBeLessThan(8 * 86_400_000);
    expect((await (await owner.c.post(`/api/v1/announcements/${wide.id}/publish`, { headers: h })).json()).state).toBe('published');
    expect(await ids(alice)).toContain(wide.id);
    await put('announcements.approval_required', 'always');
    const team = await (await owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'Targeted, approval always', audience: { roles: ['member'] } } })).json();
    expect((await owner.c.post(`/api/v1/announcements/${team.id}/publish`, { headers: h })).status()).toBe(409);
    await put('announcements.enabled', false);
    expect((await owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'Off', audience: { all: true } } })).status()).toBe(409);
  });
});
