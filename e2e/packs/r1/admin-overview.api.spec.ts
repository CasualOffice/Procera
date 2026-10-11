import { expect, test } from '@playwright/test';
import { client, hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { overdueCampaign } from '../../fixtures/library';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-ADM-01..02 — admin overview and people & access monitor (BPMS-063; FR-A01, FR-A06).

type Req = Parameters<typeof seedUser>[0];

async function member(request: Req, owner: Session, ws: string, tag: string) {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const bearer = { authorization: `Bearer ${pw.token}` };
  const { membership_id } = await (await pw.c.post('/api/v1/invitations/accept', { headers: bearer, data: { token } })).json();
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: bearer, data: { workspace_id: ws } })).json();
  return { email, id: membership_id as string, c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` } };
}

test.describe('admin overview @r1 @admin', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-ADM-01 overview: overdue acknowledgements rank in "needs attention", security counts members and failed sign-ins; members are refused', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const m = await member(request, owner, ws.id, 'ovmember');
    const { cmp } = await overdueCampaign(request, owner, ws.id);
    // Make the member an admin: a new admin grant this week, and an admin without two-step verification.
    expect((await owner.c.post('/api/v1/role-grants', { headers: h, data: { membership_id: m.id, role_key: 'workspace.admin' } })).status()).toBe(201);
    // A failed sign-in for the member (wrong password) shows up in the workspace's security strip.
    const anon = await client();
    expect((await anon.post('/api/v1/auth/login', { data: { email: m.email, password: 'definitely wrong' } })).status()).toBe(401);

    const o = await (await owner.c.get('/api/v1/admin/overview', { headers: h })).json();
    expect(o.summary.overdue_acknowledgements).toBe(2); // owner + member, both assigned and overdue (the later joiner is not overdue)
    const first = o.attention[0];
    expect(first).toMatchObject({ kind: 'overdue_acknowledgements', severity: 'high', count: 2, link: `/admin/campaigns/${cmp.id}` });
    expect(o.metrics.acknowledgements.pending).toBe(2);
    expect(o.security.failed_logins).toBeGreaterThanOrEqual(1);
    expect(o.security.members_without_mfa).toBe(1); // the owner enrolled TOTP; the member did not
    expect(o.security.new_admins).toBe(1);
    expect(o.attention.map((a: { kind: string }) => a.kind)).toEqual(['overdue_acknowledgements', 'privileged_without_mfa']);
    expect([o.requests, o.storage]).toEqual([null, null]);
    expect(o.metrics.published_versions.series).toHaveLength(7);

    const plain = await member(request, owner, ws.id, 'ovplain');
    expect((await plain.c.get('/api/v1/admin/overview', { headers: plain.headers })).status()).toBe(403);
    expect((await owner.c.get('/api/v1/admin/overview?from=2026-01-02T00:00:00Z&to=2026-01-01T00:00:00Z', { headers: h })).status()).toBe(422);
  });

  test('E2E-R1-ADM-02 people & access: MFA gaps, admins, pending invites and filters', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const m = await member(request, owner, ws.id, 'pplmember');
    await owner.c.post('/api/v1/members/invitations', { headers: h, data: { email: `pending-${Date.now()}@acme.test`, roles: ['member'] } });

    const all = await (await owner.c.get('/api/v1/admin/people', { headers: h })).json();
    expect(all.counts).toMatchObject({ members: 2, no_mfa: 1, privileged: 1, pending_invites: 1 });
    const row = all.items.find((p: { membership_id: string }) => p.membership_id === m.id);
    expect(row).toMatchObject({ mfa: false, privileged: false, dormant: false, roles: ['member'] });
    expect(row.last_active_at).not.toBeNull();
    const noMfa = await (await owner.c.get('/api/v1/admin/people?filter=no_mfa', { headers: h })).json();
    expect(noMfa.items.map((p: { membership_id: string }) => p.membership_id)).toEqual([m.id]);
    const admins = await (await owner.c.get('/api/v1/admin/people?filter=privileged', { headers: h })).json();
    expect(admins.items.map((p: { roles: string[] }) => p.roles)).toEqual([['workspace.owner']]);
    expect((await owner.c.get('/api/v1/admin/people?filter=nope', { headers: h })).status()).toBe(422);
    expect((await m.c.get('/api/v1/admin/people', { headers: m.headers })).status()).toBe(403);
  });
});
