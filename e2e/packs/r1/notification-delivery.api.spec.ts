import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser, serverNow } from '../../fixtures/accounts';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-NOT-03..05 — delivery preferences (BPMS-048; FR-N04): per-member overrides of the workspace
// defaults, daily digests that replace individual emails, quiet hours that hold email unless critical.

type Req = Parameters<typeof seedUser>[0];

async function member(request: Req, owner: Session, ws: string, tag: string) {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const auth = { authorization: `Bearer ${pw.token}` };
  const { membership_id } = await (await pw.c.post('/api/v1/invitations/accept', { headers: auth, data: { token } })).json();
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: auth, data: { workspace_id: ws } })).json();
  return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` }, email, id: membership_id as string };
}

/** Publishes an announcement to members (targeted: no approval needed). */
async function announce(owner: Session, title: string, extra: Record<string, unknown> = {}) {
  const a = await (await owner.c.post('/api/v1/announcements', { headers: owner.headers, data: { title, audience: { roles: ['member'] }, ...extra } })).json();
  const pub = await owner.c.post(`/api/v1/announcements/${a.id}/publish`, { headers: owner.headers });
  expect(pub.status(), await pub.text()).toBe(200);
  return a.id as string;
}

const mail = async (request: Req, ws: string, to: string) =>
  (await (await request.get(`/__test/mail?workspace=${ws}&to=${encodeURIComponent(to)}`)).json()).items as { kind: string; subject: string; text: string; not_before: string | null }[];

test.describe('notification delivery @r1 @notify', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-NOT-03 preferences: workspace defaults, member overrides, validation, reset', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const m = await member(request, owner, ws.id, 'prefs');
    const url = '/api/v1/me/notification-preferences';
    const before = await (await m.c.get(url, { headers: m.headers })).json();
    expect([before.digest, before.quiet_enabled, before.overrides.digest]).toEqual(['off', false, null]); // test-workspace defaults
    const put = await m.c.put(url, { headers: m.headers, data: { digest: 'weekly', digest_hour: 7, quiet_enabled: true, quiet_start: 22, quiet_end: 6 } });
    expect(put.status()).toBe(200);
    expect(await put.json()).toMatchObject({ digest: 'weekly', digest_hour: 7, quiet_enabled: true, quiet_start: 22, quiet_end: 6 });
    expect((await m.c.put(url, { headers: m.headers, data: { digest: 'hourly' } })).status()).toBe(422);
    expect((await m.c.put(url, { headers: m.headers, data: { digest_hour: 24 } })).status()).toBe(422);
    // Overrides are the member's own: the owner still has the workspace defaults.
    expect((await (await owner.c.get(url, { headers: owner.headers })).json()).digest).toBe('off');
    const reset = await (await m.c.put(url, { headers: m.headers, data: { digest: null, digest_hour: null, quiet_enabled: null, quiet_start: null, quiet_end: null } })).json();
    expect([reset.digest, reset.quiet_enabled]).toEqual(['off', false]);
  });

  test('E2E-R1-NOT-04 a daily digest replaces individual emails and goes out once at the digest hour', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const m = await member(request, owner, ws.id, 'digest');
    const hour = new Date(await serverNow(request)).getUTCHours(); // members default to UTC
    await m.c.put('/api/v1/me/notification-preferences', { headers: m.headers, data: { digest: 'daily', digest_hour: hour } });
    await announce(owner, 'Kitchen is closed on Friday');
    await announce(owner, 'New parking rules');
    await expect.poll(async () => {
      await request.post('/__test/drain');
      return (await mail(request, ws.id, m.email)).filter((e) => e.kind === 'digest').length;
    }).toBe(1);
    await request.post('/__test/drain');
    const sent = await mail(request, ws.id, m.email);
    expect(sent.filter((e) => e.kind === 'announcement')).toEqual([]);
    const digests = sent.filter((e) => e.kind === 'digest');
    expect(digests).toHaveLength(1);
    expect(digests[0]!.subject).toBe('Your Acme digest: 2 updates');
    expect(digests[0]!.text).toContain('Kitchen is closed on Friday');
    expect(digests[0]!.text).toContain('New parking rules');
    // In-app notifications are never held.
    const inbox = await (await m.c.get('/api/v1/notifications', { headers: m.headers })).json();
    expect(inbox.items.filter((n: { kind: string }) => n.kind === 'announcement')).toHaveLength(2);
  });

  test('E2E-R1-NOT-05 quiet hours hold email until they end; critical announcements still go out', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const m = await member(request, owner, ws.id, 'quiet');
    const now = await serverNow(request);
    const hour = new Date(now).getUTCHours();
    await m.c.put('/api/v1/me/notification-preferences', { headers: m.headers, data: { digest: 'off', quiet_enabled: true, quiet_start: hour, quiet_end: (hour + 2) % 24 } });
    await announce(owner, 'Quarterly results');
    await announce(owner, 'Fire alarm test now', { priority: 'critical' });
    await expect.poll(async () => {
      await request.post('/__test/drain');
      return (await mail(request, ws.id, m.email)).filter((e) => e.kind === 'announcement').length;
    }).toBe(2);
    const sent = await mail(request, ws.id, m.email);
    const held = sent.find((e) => e.subject === 'Quarterly results')!;
    const critical = sent.find((e) => e.subject.includes('Fire alarm test now'))!;
    expect(Date.parse(held.not_before!)).toBeGreaterThan(now);
    expect(new Date(held.not_before!).getUTCHours()).toBe((hour + 2) % 24);
    expect(critical.not_before).toBeNull();
  });
});
