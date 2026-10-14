import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { PNG_1PX, upload } from '../../fixtures/files';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-ISO (SEC-01/02): the owner of workspace B, with every permission there, probes the ids of
// workspace A's objects through the API. Every read and every write answers 404, exactly as for an
// id that does not exist; each probe is recorded in B's audit log as `access.denied`; A's objects
// are unchanged afterwards.

test.describe('R1 isolation sweep @r1 @security', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-ISO workspace B cannot read or change any object of workspace A', async ({ request }) => {
    const a = await workspaceWithOwner(request);
    const b = await workspaceWithOwner(request);
    const h = a.owner.headers;
    const doc = await (await a.owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'secret', title: 'A secret', content: { blocks: [{ type: 'paragraph', text: 'For A only.' }] } } })).json();
    await a.owner.c.post(`/api/v1/versions/${doc.version.id}/submit`, { headers: h });
    await a.owner.c.post(`/api/v1/versions/${doc.version.id}/publish`, { headers: h, data: {} });
    const camp = await (await a.owner.c.post('/api/v1/campaigns', { headers: h, data: { version_id: doc.version.id, title: 'A campaign', audience: { all: true } } })).json();
    const ann = await (await a.owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'A news', content: { blocks: [{ type: 'paragraph', text: 'x' }] }, audience: { all: true } } })).json();
    const col = await (await a.owner.c.post('/api/v1/collections', { headers: h, data: { name: 'A only' } })).json();
    const dept = await (await a.owner.c.post('/api/v1/departments', { headers: h, data: { name: 'A dept' } })).json();
    const view = await (await a.owner.c.post('/api/v1/views', { headers: h, data: { list: 'library.documents', name: 'A view', state: { search: '' } } })).json();
    const file = await upload(a.owner, 'a.png', PNG_1PX);
    const me = await (await a.owner.c.get('/api/v1/me/permissions', { headers: h })).json();
    const owner = me.membership_id as string;

    const hcal = await (await a.owner.c.post('/api/v1/holiday-calendars', { headers: h, data: { name: 'A holidays' } })).json();
    const wloc = await (await a.owner.c.post('/api/v1/work-locations', { headers: h, data: { name: 'A office', timezone: 'UTC' } })).json();
    const ltype = await (await a.owner.c.post('/api/v1/leave-types', { headers: h, data: { name: 'A leave' } })).json();
    const lpol = await (await a.owner.c.post('/api/v1/leave-policies', { headers: h, data: { name: 'A policy', audience: { all: true, departments: [], groups: [], roles: [] } } })).json();
    const probes: [string, string, unknown?][] = [
      ['PATCH', `/api/v1/leave-types/${ltype.id}`, { name: 'taken' }],
      ['GET', `/api/v1/leave-policies/${lpol.id}`],
      ['GET', `/api/v1/leave-policies/${lpol.id}/members`],
      ['GET', `/api/v1/members/${owner}/time-off`],
      ['POST', `/api/v1/members/${owner}/time-off/adjustments`, { leave_type_id: ltype.id, days: 1, reason: 'x' }],
      ['GET', `/api/v1/work-locations/${wloc.id}`],
      ['PATCH', `/api/v1/work-locations/${wloc.id}`, { name: 'taken', timezone: 'UTC' }],
      ['GET', `/api/v1/holiday-calendars/${hcal.id}`],
      ['GET', `/api/v1/holiday-calendars/${hcal.id}/holidays?year=2030`],
      ['PUT', `/api/v1/holiday-calendars/${hcal.id}/holidays/2030-01-01`, { name: 'taken' }],
      ['GET', `/api/v1/members/${owner}/hr`],
      ['GET', `/api/v1/artifacts/${doc.artifact.id}`],
      ['GET', `/api/v1/versions/${doc.version.id}`],
      ['GET', `/api/v1/versions/${doc.version.id}/export?format=html`],
      ['GET', `/api/v1/versions/${doc.version.id}/attachments`],
      ['GET', `/api/v1/artifacts/${doc.artifact.id}/raci`],
      ['POST', `/api/v1/artifacts/${doc.artifact.id}/versions`],
      ['GET', `/api/v1/campaigns/${camp.id}`],
      ['GET', `/api/v1/campaigns/${camp.id}/recipients`],
      ['POST', `/api/v1/campaigns/${camp.id}/activate`],
      ['GET', `/api/v1/announcements/${ann.id}`],
      ['POST', `/api/v1/announcements/${ann.id}/publish`],
      ['PATCH', `/api/v1/collections/${col.id}`, { name: 'taken' }],
      ['DELETE', `/api/v1/collections/${col.id}`],
      ['PATCH', `/api/v1/departments/${dept.id}`, { name: 'taken' }],
      ['DELETE', `/api/v1/views/${view.id}`],
      ['GET', `/api/v1/files/${file.id}`],
      ['GET', `/api/v1/files/${file.id}/download-url`],
      ['GET', `/api/v1/files/${file.id}/preview-url`],
      ['DELETE', `/api/v1/files/${file.id}`],
      ['GET', `/api/v1/members/${owner}/directory`],
      ['POST', `/api/v1/members/${owner}/deactivate`],
    ];
    const problems: string[] = [];
    for (const [method, path, data] of probes) {
      const res = await b.owner.c.fetch(path, { method, headers: b.owner.headers, data });
      if (res.status() !== 404) problems.push(`${method} ${path} → ${res.status()}`);
    }
    expect(problems).toEqual([]);

    // Each probe is on record in B's audit log as a denied access to that object id.
    const denied = await (await b.owner.c.get('/api/v1/audit/events?action=access.denied&page[size]=100', { headers: b.owner.headers })).json();
    expect(denied.items.length).toBe(probes.length);
    expect(denied.items.every((e: { result: string }) => e.result === 'denied')).toBe(true);
    expect(denied.items.map((e: { object_id: string }) => e.object_id)).toEqual(expect.arrayContaining([doc.artifact.id, doc.version.id, file.id, camp.id, owner]));
    const chain = await (await request.get(`/__test/audit/verify?workspace=${b.ws.id}`)).json();
    expect(chain.ok).toBe(true);

    // A is untouched.
    expect((await (await a.owner.c.get(`/api/v1/artifacts/${doc.artifact.id}`, { headers: h })).json()).artifact.title).toBe('A secret');
    expect((await a.owner.c.get(`/api/v1/files/${file.id}`, { headers: h })).status()).toBe(200);
    expect((await (await a.owner.c.get(`/api/v1/campaigns/${camp.id}`, { headers: h })).json()).state).toBe('draft');
    const cols = await (await a.owner.c.get('/api/v1/collections', { headers: h })).json();
    expect(cols.items.map((c: { name: string }) => c.name)).toContain('A only');
  });
});
