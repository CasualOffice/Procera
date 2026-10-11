import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-VIEW-01 — saved DataGrid views and audited exports (BPMS-028; FR-V05/V06).

test.describe('saved views @r1 @grid', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-VIEW-01 personal and shared views, one default, only the owner edits; exports are audited', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const state = { q: '', sort: [{ id: 'title', desc: true }], filters: [{ id: 'state', op: 'any_of', value: ['Published'] }], columns: ['title', 'state'], density: 'compact' };
    const mine = await (await owner.c.post('/api/v1/views', { headers: h, data: { list: 'library.documents', name: 'My drafts', state, is_default: true } })).json();
    const shared = await owner.c.post('/api/v1/views', { headers: h, data: { list: 'library.documents', name: 'Published', state, shared: true, is_default: true } });
    expect(shared.status()).toBe(201);
    const sv = await shared.json();
    let list = (await (await owner.c.get('/api/v1/views?list=library.documents', { headers: h })).json()).items;
    expect(list.filter((v: { is_default: boolean }) => v.is_default).map((v: { id: string }) => v.id)).toEqual([sv.id]); // one default per member
    expect(list.find((v: { id: string }) => v.id === mine.id).state).toEqual(state);
    expect((await owner.c.post('/api/v1/views', { headers: h, data: { list: 'Bad List!', name: 'x', state } })).status()).toBe(422);
    expect((await owner.c.post('/api/v1/views', { headers: h, data: { list: 'members', name: ' ', state } })).status()).toBe(422);

    // A member sees the shared view but not the personal one, and cannot change it.
    const email = await seedUser(request, 'viewer');
    await owner.c.post('/api/v1/members/invitations', { headers: h, data: { email, roles: ['member'] } });
    const token = await inviteToken(request, ws.id, email);
    const pw = await passwordSession(email);
    const bearer = { authorization: `Bearer ${pw.token}` };
    await pw.c.post('/api/v1/invitations/accept', { headers: bearer, data: { token } });
    const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: bearer, data: { workspace_id: ws.id } })).json();
    const mh = { authorization: `Bearer ${sw.access_token}` };
    list = (await (await pw.c.get('/api/v1/views?list=library.documents', { headers: mh })).json()).items;
    expect(list.map((v: { name: string; mine: boolean; is_default: boolean }) => [v.name, v.mine, v.is_default])).toEqual([['Published', false, false]]);
    expect((await pw.c.patch(`/api/v1/views/${sv.id}`, { headers: mh, data: { name: 'Mine now' } })).status()).toBe(403);
    expect((await pw.c.delete(`/api/v1/views/${sv.id}`, { headers: mh })).status()).toBe(403);
    expect((await pw.c.patch(`/api/v1/views/${mine.id}`, { headers: mh, data: { name: 'x' } })).status()).toBe(404);

    expect((await owner.c.patch(`/api/v1/views/${sv.id}`, { headers: h, data: { name: 'Published only', shared: false } })).status()).toBe(200);
    expect((await (await pw.c.get('/api/v1/views?list=library.documents', { headers: mh })).json()).items).toEqual([]);
    expect((await owner.c.delete(`/api/v1/views/${mine.id}`, { headers: h })).status()).toBe(204);

    expect((await pw.c.post('/api/v1/exports', { headers: mh, data: { list: 'library.documents', format: 'csv', rows: 3, columns: ['title', 'state'], filters: [] } })).status()).toBe(204);
    expect((await pw.c.post('/api/v1/exports', { headers: mh, data: { list: 'library.documents', format: 'xlsx', rows: 3, columns: [] } })).status()).toBe(422);
    const audit = await (await owner.c.get('/api/v1/audit/events?action=data.exported', { headers: h })).json();
    expect(audit.items.length).toBeGreaterThanOrEqual(1);
  });
});
