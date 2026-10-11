import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser, serverNow } from '../../fixtures/accounts';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-LIB-01..05 — governed library: lifecycle, separation of duties, immutability (T07), scheduling (BPMS-040; FR-G04/G05).

type Req = Parameters<typeof seedUser>[0];

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

const doc = (text: string) => ({ blocks: [{ type: 'paragraph', text }] });

test.describe('library @r1 @library', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-LIB-01 SOP: draft → edit (If-Match) → submit → approved with content hash → published; published versions are immutable (T07)', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const created = await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'it.onboarding', title: 'IT onboarding', content: doc('Create accounts.') } });
    expect(created.status()).toBe(201);
    const { artifact, version } = await created.json();
    expect([artifact.state, version.lifecycle, version.version_no]).toEqual(['draft', 'draft', 1]);
    expect((await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'it.onboarding', title: 'Again' } })).status()).toBe(409);
    const edit = await owner.c.patch(`/api/v1/versions/${version.id}`, { headers: { ...h, 'if-match': `"${version.revision}"` }, data: { content: doc('Create accounts and laptops.') } });
    expect(edit.status()).toBe(200);
    expect((await owner.c.patch(`/api/v1/versions/${version.id}`, { headers: { ...h, 'if-match': `"${version.revision}"` }, data: { content: doc('x') } })).status()).toBe(412);
    // Standard SOP in a Standard workspace needs no review: submission approves and freezes the hash.
    const sub = await (await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h })).json();
    expect(sub.lifecycle).toBe('approved');
    expect(sub.content_hash).toMatch(/^[0-9a-f]{64}$/);
    expect((await owner.c.patch(`/api/v1/versions/${version.id}`, { headers: h, data: { content: doc('sneaky') } })).status()).toBe(409);
    const pub = await (await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: h, data: {} })).json();
    expect(pub.lifecycle).toBe('published');
    const again = await (await owner.c.get(`/api/v1/versions/${version.id}`, { headers: h })).json();
    expect([again.content, again.content_hash]).toEqual([doc('Create accounts and laptops.'), sub.content_hash]);
    const art = await (await owner.c.get(`/api/v1/artifacts/${artifact.id}`, { headers: h })).json();
    expect([art.artifact.state, art.artifact.current_published_version_id]).toEqual(['published', version.id]);
    expect(art.versions[0].published_at).toMatch(/^\d{4}-\d{2}-\d{2}T/); // RFC 3339, not a tuple
    const v2 = await owner.c.post(`/api/v1/artifacts/${artifact.id}/versions`, { headers: h });
    expect([v2.status(), (await v2.json()).version_no]).toEqual([201, 2]);
    expect((await owner.c.post(`/api/v1/artifacts/${artifact.id}/versions`, { headers: h })).status()).toBe(409);
  });

  test('E2E-R1-LIB-02 controlled policy: authors cannot approve their own; a reviewer approves; owner publishes', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const reviewer = await joinAs(request, owner, ws.id, 'rev', 'reviewer');
    const { version } = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'sec.badges', title: 'Badge policy', criticality: 'controlled', content: doc('Wear a badge.') } })).json();
    const sub = await (await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h })).json();
    expect([sub.lifecycle, sub.approvals_required]).toEqual(['in_review', 1]);
    const self = await owner.c.post(`/api/v1/versions/${version.id}/reviews`, { headers: h, data: { decision: 'approve' } });
    expect([self.status(), (await self.json()).code]).toEqual([403, 'sod_violation']);
    expect((await reviewer.c.post(`/api/v1/versions/${version.id}/publish`, { headers: reviewer.headers, data: {} })).status()).toBe(403);
    const ok = await (await reviewer.c.post(`/api/v1/versions/${version.id}/reviews`, { headers: reviewer.headers, data: { decision: 'approve', comment: 'Clear.' } })).json();
    expect(ok.lifecycle).toBe('approved');
    expect(ok.reviews).toHaveLength(1);
    expect((await (await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: h, data: {} })).json()).lifecycle).toBe('published');
  });

  test('E2E-R1-LIB-03 changes requested needs a comment and returns the draft; withdraw; a reason for change is required for later versions', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const reviewer = await joinAs(request, owner, ws.id, 'rev3', 'reviewer');
    const { artifact, version } = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'hr.leave', title: 'Leave', criticality: 'controlled', content: doc('20 days.') } })).json();
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h });
    expect((await (await owner.c.post(`/api/v1/versions/${version.id}/withdraw`, { headers: h })).json()).lifecycle).toBe('draft');
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h });
    expect((await reviewer.c.post(`/api/v1/versions/${version.id}/reviews`, { headers: reviewer.headers, data: { decision: 'changes_requested' } })).status()).toBe(422);
    expect((await (await reviewer.c.post(`/api/v1/versions/${version.id}/reviews`, { headers: reviewer.headers, data: { decision: 'changes_requested', comment: 'Add carry-over.' } })).json()).lifecycle).toBe('draft');
    await owner.c.patch(`/api/v1/versions/${version.id}`, { headers: h, data: { content: doc('20 days; 5 carry over.') } });
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h });
    await reviewer.c.post(`/api/v1/versions/${version.id}/reviews`, { headers: reviewer.headers, data: { decision: 'approve' } });
    const p1 = await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: h, data: {} });
    expect(p1.status(), await p1.text()).toBe(200);
    const v2 = await (await owner.c.post(`/api/v1/artifacts/${artifact.id}/versions`, { headers: h })).json();
    await owner.c.patch(`/api/v1/versions/${v2.id}`, { headers: h, data: { content: doc('25 days.') } });
    await owner.c.post(`/api/v1/versions/${v2.id}/submit`, { headers: h });
    await reviewer.c.post(`/api/v1/versions/${v2.id}/reviews`, { headers: reviewer.headers, data: { decision: 'approve' } });
    const noReason = await owner.c.post(`/api/v1/versions/${v2.id}/publish`, { headers: h, data: {} });
    expect(noReason.status(), await noReason.text()).toBe(422);
    expect((await (await owner.c.post(`/api/v1/versions/${v2.id}/publish`, { headers: h, data: { change_reason: 'New allowance' } })).json()).lifecycle).toBe('published');
    expect((await (await owner.c.get(`/api/v1/versions/${version.id}`, { headers: h })).json()).lifecycle).toBe('deprecated');
  });

  test('E2E-R1-LIB-04 scheduled publication goes live when its effective time arrives and supersedes the previous version', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const { artifact, version } = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'ops.backup', title: 'Backups', content: doc('Nightly.') } })).json();
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h });
    await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: h, data: {} });
    const v2 = await (await owner.c.post(`/api/v1/artifacts/${artifact.id}/versions`, { headers: h })).json();
    await owner.c.patch(`/api/v1/versions/${v2.id}`, { headers: h, data: { content: doc('Hourly.') } });
    await owner.c.post(`/api/v1/versions/${v2.id}/submit`, { headers: h });
    const at = new Date((await serverNow(owner.c)) + 30_000).toISOString();
    const sch = await (await owner.c.post(`/api/v1/versions/${v2.id}/publish`, { headers: h, data: { effective_at: at, change_reason: 'More often' } })).json();
    expect(sch.lifecycle).toBe('scheduled');
    expect((await (await owner.c.get(`/api/v1/versions/${version.id}`, { headers: h })).json()).lifecycle).toBe('published');
    await request.post('/__test/clock', { data: { advance: 'PT1M' } });
    await request.post('/__test/drain');
    expect((await (await owner.c.get(`/api/v1/versions/${v2.id}`, { headers: h })).json()).lifecycle).toBe('published');
    expect((await (await owner.c.get(`/api/v1/versions/${version.id}`, { headers: h })).json()).lifecycle).toBe('deprecated');
    expect((await owner.c.post(`/api/v1/versions/${version.id}/retire`, { headers: h, data: { reason: 'Superseded' } })).status()).toBe(200);
  });

  test('E2E-R1-LIB-05 members read published policies but cannot author; lists show only kinds you may view', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const member = await joinAs(request, owner, ws.id, 'reader', 'member', false);
    const { version } = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'policy', key: 'code.conduct', title: 'Code of conduct', content: doc('Be kind.') } })).json();
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: owner.headers });
    await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: owner.headers, data: {} });
    const list = await (await member.c.get('/api/v1/artifacts?kind=policy', { headers: member.headers })).json();
    expect(list.items.map((a: { key: string }) => a.key)).toContain('code.conduct');
    expect((await member.c.get(`/api/v1/versions/${version.id}`, { headers: member.headers })).status()).toBe(200);
    expect((await member.c.post('/api/v1/artifacts', { headers: member.headers, data: { kind: 'policy', key: 'mine', title: 'Mine' } })).status()).toBe(403);
    expect((await member.c.get('/api/v1/artifacts?kind=process', { headers: member.headers })).status()).toBe(422);
  });
});
