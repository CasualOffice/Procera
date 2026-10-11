import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-POL-01..03 — policy portal (BPMS-045; FR-PL01/PL02, A-14): readers see published history only,
// restricted policies are hidden outside their applicability (lists, reads, search, portal), and the
// portal shows what changed since the previous version.

type Req = Parameters<typeof seedUser>[0];

/** A plain member (no authoring permissions) with a workspace session and their membership id. */
async function reader(request: Req, owner: Session, ws: string, tag: string) {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const bearer = { authorization: `Bearer ${pw.token}` };
  const { membership_id } = await (await pw.c.post('/api/v1/invitations/accept', { headers: bearer, data: { token } })).json();
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: bearer, data: { workspace_id: ws } })).json();
  return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` }, id: membership_id as string };
}

const doc = (...texts: string[]) => ({ blocks: texts.map((text) => ({ type: 'paragraph', text })) });

async function publish(owner: Session, versionId: string, reason?: string) {
  await owner.c.post(`/api/v1/versions/${versionId}/submit`, { headers: owner.headers });
  const r = await owner.c.post(`/api/v1/versions/${versionId}/publish`, { headers: owner.headers, data: reason ? { change_reason: reason } : {} });
  expect((await r.json()).lifecycle).toBe('published');
}

test.describe('policy portal @r1 @library', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-POL-01 readers never see drafts: not in lists, reads or search until published, and later drafts stay hidden', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const m = await reader(request, owner, ws.id, 'draftreader');
    const { artifact, version } = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'fin.travel', title: 'Travel zebra policy', content: doc('Economy class.') } })).json();
    const keys = async () => (await (await m.c.get('/api/v1/artifacts?kind=policy', { headers: m.headers })).json()).items.map((a: { key: string }) => a.key);
    expect(await keys()).not.toContain('fin.travel');
    expect((await m.c.get(`/api/v1/artifacts/${artifact.id}`, { headers: m.headers })).status()).toBe(404);
    expect((await m.c.get(`/api/v1/versions/${version.id}`, { headers: m.headers })).status()).toBe(404);
    const hits = async () => (await (await m.c.get('/api/v1/search?q=zebra', { headers: m.headers })).json()).items.map((x: { id: string }) => x.id);
    expect(await hits()).not.toContain(artifact.id);
    // The owner (an author) sees the draft everywhere.
    expect((await (await owner.c.get('/api/v1/search?q=zebra', { headers: h })).json()).items.map((x: { id: string }) => x.id)).toContain(artifact.id);

    await publish(owner, version.id);
    expect(await keys()).toContain('fin.travel');
    expect(await hits()).toContain(artifact.id);
    const v2 = await (await owner.c.post(`/api/v1/artifacts/${artifact.id}/versions`, { headers: h })).json();
    expect((await m.c.get(`/api/v1/versions/${v2.id}`, { headers: m.headers })).status()).toBe(404);
    expect((await m.c.get(`/api/v1/versions/${version.id}/diff/${v2.id}`, { headers: m.headers })).status()).toBe(404);
    const seen = await (await m.c.get(`/api/v1/artifacts/${artifact.id}`, { headers: m.headers })).json();
    expect(seen.versions.map((v: { version_no: number }) => v.version_no)).toEqual([1]);
  });

  test('E2E-R1-POL-02 applicability: a restricted policy is hidden from members outside its audience; an unrestricted one is visible but not "applicable"', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const inside = await reader(request, owner, ws.id, 'warden');
    const outside = await reader(request, owner, ws.id, 'visitor');
    const g = await (await owner.c.post('/api/v1/groups', { headers: h, data: { name: 'Fire wardens' } })).json();
    expect((await owner.c.post(`/api/v1/groups/${g.id}/members`, { headers: h, data: { membership_id: inside.id } })).status()).toBe(204);

    const secret = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'sec.evacuation', title: 'Evacuation quokka plan', content: doc('Meet at point B.') } })).json();
    await publish(owner, secret.version.id);
    expect((await owner.c.put(`/api/v1/artifacts/${secret.artifact.id}/applicability`, { headers: h, data: { applicability: {}, restricted: true } })).status()).toBe(422);
    expect((await owner.c.put(`/api/v1/artifacts/${secret.artifact.id}/applicability`, { headers: h, data: { restricted: true } })).status()).toBe(422);
    expect((await inside.c.put(`/api/v1/artifacts/${secret.artifact.id}/applicability`, { headers: inside.headers, data: { restricted: false } })).status()).toBe(403);
    const set = await owner.c.put(`/api/v1/artifacts/${secret.artifact.id}/applicability`, { headers: h, data: { category: 'Safety', applicability: { groups: [g.id] }, restricted: true } });
    expect(set.status()).toBe(200);
    expect(await set.json()).toMatchObject({ category: 'Safety', restricted: true, applicability: { groups: [g.id] } });

    const open = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'sec.drills', title: 'Drill schedule', content: doc('Quarterly.') } })).json();
    await publish(owner, open.version.id);
    await owner.c.put(`/api/v1/artifacts/${open.artifact.id}/applicability`, { headers: h, data: { category: 'Safety', applicability: { groups: [g.id] }, restricted: false } });

    for (const [who, sees] of [[inside, true], [outside, false]] as const) {
      const list = (await (await who.c.get('/api/v1/artifacts?kind=policy', { headers: who.headers })).json()).items.map((a: { key: string }) => a.key);
      expect(list.includes('sec.evacuation')).toBe(sees);
      expect(list).toContain('sec.drills');
      expect((await who.c.get(`/api/v1/artifacts/${secret.artifact.id}`, { headers: who.headers })).status()).toBe(sees ? 200 : 404);
      expect((await who.c.get(`/api/v1/versions/${secret.version.id}`, { headers: who.headers })).status()).toBe(sees ? 200 : 404);
      const hits = (await (await who.c.get('/api/v1/search?q=quokka', { headers: who.headers })).json()).items.map((x: { id: string }) => x.id);
      expect(hits.includes(secret.artifact.id)).toBe(sees);
      const portal = (await (await who.c.get('/api/v1/me/policies', { headers: who.headers })).json()).items.map((p: { key: string }) => p.key).sort();
      expect(portal).toEqual(sees ? ['sec.drills', 'sec.evacuation'] : []);
    }
    // Change requests on a hidden policy are hidden too.
    expect((await outside.c.post(`/api/v1/artifacts/${secret.artifact.id}/change-requests`, { headers: outside.headers, data: { title: 'x' } })).status()).toBe(404);
  });

  test('E2E-R1-POL-03 the portal lists applicable policies with what changed since the previous version, filterable by text and category', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const m = await reader(request, owner, ws.id, 'employee');
    const a = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'hr.leave', title: 'Annual leave', content: doc('20 days.', 'Ask your manager.') } })).json();
    await publish(owner, a.version.id);
    await owner.c.put(`/api/v1/artifacts/${a.artifact.id}/applicability`, { headers: h, data: { category: 'People' } });
    const v2 = await (await owner.c.post(`/api/v1/artifacts/${a.artifact.id}/versions`, { headers: h })).json();
    await owner.c.patch(`/api/v1/versions/${v2.id}`, { headers: h, data: { content: doc('25 days.', 'Ask your manager.', 'Five days carry over.') } });
    await publish(owner, v2.id, 'More leave');
    const b = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'it.devices', title: 'Device use', content: doc('Lock your screen.') } })).json();
    await publish(owner, b.version.id);
    await owner.c.put(`/api/v1/artifacts/${b.artifact.id}/applicability`, { headers: h, data: { category: 'IT' } });
    await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'it.draft', title: 'Draft only', content: doc('x') } });

    const all = await (await m.c.get('/api/v1/me/policies', { headers: m.headers })).json();
    expect(all.categories).toEqual(['IT', 'People']);
    expect(all.items.map((p: { key: string }) => p.key)).toEqual(['it.devices', 'hr.leave']);
    const leave = all.items[1];
    expect([leave.version_no, leave.version_id]).toEqual([2, v2.id]);
    expect(leave.what_changed).toEqual({ from_version_no: 1, added: 1, removed: 0, changed: 1, change_reason: 'More leave' });
    expect(all.items[0].what_changed).toBeNull();
    expect((await (await m.c.get('/api/v1/me/policies?category=people', { headers: m.headers })).json()).items.map((p: { key: string }) => p.key)).toEqual(['hr.leave']);
    expect((await (await m.c.get('/api/v1/me/policies?q=device', { headers: m.headers })).json()).items.map((p: { key: string }) => p.key)).toEqual(['it.devices']);
  });
});
