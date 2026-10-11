import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-LIB-11..13 — collections, owners and RACI (BPMS-041; FR-G01 "search authorized-only", "RACI exportable").

type Req = Parameters<typeof seedUser>[0];
const doc = (text: string) => ({ blocks: [{ type: 'paragraph', text }] });

async function reader(request: Req, owner: Session, ws: string, tag: string) {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const bearer = { authorization: `Bearer ${pw.token}` };
  const { membership_id } = await (await pw.c.post('/api/v1/invitations/accept', { headers: bearer, data: { token } })).json();
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: bearer, data: { workspace_id: ws } })).json();
  return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` }, id: membership_id as string, email };
}

async function published(owner: Session, key: string, title: string) {
  const r = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key, title, content: doc('x') } })).json();
  await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: owner.headers });
  await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: owner.headers, data: {} });
  return r.artifact.id as string;
}

test.describe('collections @r1 @library', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-LIB-11 a collection tree: sibling names unique, moves rewrite paths, no move under itself, only empty collections delete; lists filter by subtree', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const ops = await (await owner.c.post('/api/v1/collections', { headers: h, data: { name: 'Operations' } })).json();
    const fac = await (await owner.c.post('/api/v1/collections', { headers: h, data: { name: 'Facilities', parent_id: ops.id } })).json();
    expect(fac.path).toEqual([ops.id, fac.id]);
    expect((await owner.c.post('/api/v1/collections', { headers: h, data: { name: 'facilities', parent_id: ops.id } })).status()).toBe(409);
    expect((await owner.c.patch(`/api/v1/collections/${ops.id}`, { headers: h, data: { parent_id: fac.id } })).status()).toBe(422);
    const people = await (await owner.c.post('/api/v1/collections', { headers: h, data: { name: 'People' } })).json();
    const moved = await (await owner.c.patch(`/api/v1/collections/${fac.id}`, { headers: h, data: { parent_id: people.id, name: 'Workplace' } })).json();
    expect([moved.name, moved.path]).toEqual(['Workplace', [people.id, fac.id]]);
    const doorId = await published(owner, 'ops.doors', 'Door rules');
    await published(owner, 'ops.other', 'Elsewhere');
    expect((await owner.c.put(`/api/v1/artifacts/${doorId}/collection`, { headers: h, data: { collection_id: fac.id } })).status()).toBe(200);
    // Filtering by the parent includes the sub-collection.
    const inPeople = (await (await owner.c.get(`/api/v1/artifacts?collection=${people.id}`, { headers: h })).json()).items.map((a: { key: string }) => a.key);
    expect(inPeople).toEqual(['ops.doors']);
    expect((await owner.c.delete(`/api/v1/collections/${people.id}`, { headers: h })).status()).toBe(409);
    expect((await owner.c.delete(`/api/v1/collections/${ops.id}`, { headers: h })).status()).toBe(204);
  });

  test('E2E-R1-LIB-12 a restricted collection hides its documents from readers outside it: lists, reads, search and the tree', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const inside = await reader(request, owner, ws.id, 'colin');
    const outside = await reader(request, owner, ws.id, 'colout');
    const g = await (await owner.c.post('/api/v1/groups', { headers: h, data: { name: 'Board' } })).json();
    await owner.c.post(`/api/v1/groups/${g.id}/members`, { headers: h, data: { membership_id: inside.id } });
    const board = await (await owner.c.post('/api/v1/collections', { headers: h, data: { name: 'Board papers' } })).json();
    expect((await owner.c.put(`/api/v1/collections/${board.id}/visibility`, { headers: h, data: { visibility: {} } })).status()).toBe(422);
    expect((await owner.c.put(`/api/v1/collections/${board.id}/visibility`, { headers: h, data: { visibility: { groups: [g.id] } } })).status()).toBe(200);
    const sub = await (await owner.c.post('/api/v1/collections', { headers: h, data: { name: 'Minutes', parent_id: board.id } })).json();
    const id = await published(owner, 'board.pelican', 'Pelican minutes');
    await owner.c.put(`/api/v1/artifacts/${id}/collection`, { headers: h, data: { collection_id: sub.id } });
    for (const [who, sees] of [[inside, true], [outside, false]] as const) {
      const list = (await (await who.c.get('/api/v1/artifacts', { headers: who.headers })).json()).items.map((a: { key: string }) => a.key);
      expect(list.includes('board.pelican')).toBe(sees);
      expect((await who.c.get(`/api/v1/artifacts/${id}`, { headers: who.headers })).status()).toBe(sees ? 200 : 404);
      const hits = (await (await who.c.get('/api/v1/search?q=pelican', { headers: who.headers })).json()).items.map((x: { id: string }) => x.id);
      expect(hits.includes(id)).toBe(sees);
      const tree = (await (await who.c.get('/api/v1/collections', { headers: who.headers })).json()).items.map((c: { name: string }) => c.name);
      expect(tree.includes('Minutes')).toBe(sees);
    }
    // Members cannot manage collections.
    expect((await inside.c.post('/api/v1/collections', { headers: inside.headers, data: { name: 'Mine' } })).status()).toBe(403);
  });

  test('E2E-R1-LIB-13 owner hand-over and RACI (one accountable), readable by readers, exported as CSV and audited', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const m = await reader(request, owner, ws.id, 'raciuser');
    const g = await (await owner.c.post('/api/v1/groups', { headers: h, data: { name: 'Safety team' } })).json();
    const id = await published(owner, 'ops.fire', 'Fire drill');
    expect((await owner.c.put(`/api/v1/artifacts/${id}/owner`, { headers: h, data: { membership_id: m.id } })).status()).toBe(200);
    expect((await m.c.put(`/api/v1/artifacts/${id}/owner`, { headers: m.headers, data: { membership_id: m.id } })).status()).toBe(403);
    expect((await owner.c.put(`/api/v1/artifacts/${id}/raci`, { headers: h, data: { accountable: [{ membership_id: m.id }, { group_id: g.id }] } })).status()).toBe(422);
    const set = await owner.c.put(`/api/v1/artifacts/${id}/raci`, { headers: h, data: { responsible: [{ group_id: g.id }], accountable: [{ membership_id: m.id }], informed: [{ group_id: g.id }] } });
    expect(set.status()).toBe(200);
    const raci = await (await m.c.get(`/api/v1/artifacts/${id}/raci`, { headers: m.headers })).json();
    expect([raci.responsible[0].name, raci.accountable[0].name, raci.consulted, raci.informed[0].name]).toEqual(['Safety team', 'raciuser', [], 'Safety team']);
    const csv = await owner.c.get('/api/v1/raci/export', { headers: h });
    expect(csv.headers()['content-type']).toContain('text/csv');
    const lines = (await csv.text()).trim().split('\n');
    expect(lines[0]).toBe('collection,document,key,kind,owner,responsible,accountable,consulted,informed');
    expect(lines.find((l) => l.includes('"ops.fire"'))).toBe('"","Fire drill","ops.fire","sop","raciuser","Safety team","raciuser","","Safety team"');
    const audit = await (await owner.c.get('/api/v1/audit/events?action=raci.', { headers: h })).json();
    expect(audit.items.map((e: { action: string }) => e.action)).toContain('raci.exported');
  });
});
