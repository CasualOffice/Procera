import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R3-PROC-01..03 — processes published from the Guided Studio's API (BPMS-100, BPMS-084): a process
// made from a template is validated, published (a release bundle is written) and offered under New
// request, where an employee starts it and the manager approves it; a step given to a group that
// doesn't exist is a problem on that step and publishing is refused; drafts are saved with If-Match
// and a published process opens a new draft.

const key = () => `e2e-${Date.now()}-${Math.random().toString(36).slice(2)}`;

async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const s = (await enter(email, ws)) as Session;
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, session: s, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

test.describe('processes @r3 @processes', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R3-PROC-01 a process from a template is published and offered under New request; an employee asks and the manager approves', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const templates = (await (await owner.c.get('/api/v1/process-templates', { headers: h })).json()).items as { key: string }[];
    expect(templates.map((t) => t.key)).toEqual(expect.arrayContaining(['blank', 'hr.leave-request']));
    const created = await owner.c.post('/api/v1/processes', { headers: h, data: { key: 'ops.time-off', title: 'Time off', template: 'hr.leave-request' } });
    expect(created.status(), await created.text()).toBe(201);
    const { id } = await created.json();
    const p = await (await owner.c.get(`/api/v1/processes/${id}`, { headers: h })).json();
    expect(p.draft.version_no).toBe(1);
    expect(p.draft.content.ir.process.key).toBe('ops.time-off');
    expect(p.draft.content.studio.fields.start_date.label).toBeTruthy();
    // Rename the request and save (If-Match), then validate: nothing blocks publishing.
    const content = p.draft.content;
    content.studio.request.name = 'Time off';
    content.studio.request.prefix = 'TO';
    const saved = await owner.c.put(`/api/v1/processes/${id}/draft`, { headers: { ...h, 'if-match': `"${p.draft.revision}"` }, data: { content } });
    expect(saved.status(), await saved.text()).toBe(200);
    const v = await (await owner.c.post('/api/v1/processes/validate', { headers: h, data: { content } })).json();
    expect(v.problems.filter((x: { severity: string }) => x.severity === 'error')).toEqual([]);
    const pub = await owner.c.post(`/api/v1/processes/${id}/publish`, { headers: h });
    expect(pub.status(), await pub.text()).toBe(200);
    expect(await pub.json()).toMatchObject({ state: 'published', version_no: 1 });
    const listed = (await (await owner.c.get('/api/v1/processes', { headers: h })).json()).items;
    expect(listed.find((x: { id: string }) => x.id === id).published.version_no).toBe(1);

    // An employee sees it under New request, asks, and the manager approves.
    const bo = await person(request, owner, ws.id, 'Bo Boss');
    const ann = await person(request, owner, ws.id, 'Ann Asker');
    expect((await owner.c.patch(`/api/v1/members/${ann.id}`, { headers: h, data: { manager_membership_id: bo.id } })).ok()).toBe(true);
    const types = (await (await ann.session.c.get('/api/v1/request-types', { headers: ann.session.headers })).json()).items as { key: string; name: string }[];
    expect(types.find((t) => t.key === 'ops.time-off')?.name).toBe('Time off');
    const started = await ann.session.c.post('/api/v1/requests', {
      headers: { ...ann.session.headers, 'idempotency-key': key() },
      data: { type: 'ops.time-off', input: { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23' } },
    });
    expect(started.status(), await started.text()).toBe(201);
    const { id: rid, display_id: ref } = await started.json();
    expect(ref).toMatch(/^TO-\d{4}-\d{6}$/);
    const items = (await (await bo.session.c.get('/api/v1/work', { headers: bo.session.headers })).json()).items as { id: string; revision: number; request: { id: string; type_name: string } }[];
    const item = items.find((w) => w.request.id === rid)!;
    expect(item.request.type_name).toBe('Time off');
    expect((await bo.session.c.post(`/api/v1/work/${item.id}/decisions`, { headers: { ...bo.session.headers, 'if-match': `"${item.revision}"` }, data: { outcome: 'approved' } })).status()).toBe(200);
    await expect.poll(async () => (await (await ann.session.c.get(`/api/v1/requests/${rid}`, { headers: ann.session.headers })).json()).status).toBe('approved');
  });

  test('E2E-R3-PROC-02 a step given to a group that does not exist is a problem on that step, and publishing is refused', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const { id } = await (await owner.c.post('/api/v1/processes', { headers: h, data: { key: 'ops.badge', title: 'Badge request', template: 'blank' } })).json();
    const p = await (await owner.c.get(`/api/v1/processes/${id}`, { headers: h })).json();
    const content = p.draft.content;
    content.ir.nodes.splice(1, 0, { id: 'security', kind: 'human.approval', name: { en: 'Security approval' }, assignment: { candidates: [{ type: 'group', ref: 'Nobody Here' }] } });
    content.ir.edges = [
      { id: 'e1', from: 'start', to: 'security' },
      { id: 'e2', from: 'security', to: 'end_approved', on: 'approved' },
      { id: 'e3', from: 'security', to: 'end_approved', on: 'rejected' },
      { id: 'e4', from: 'security', to: 'end_approved', on: 'changes_requested' },
    ];
    expect((await owner.c.put(`/api/v1/processes/${id}/draft`, { headers: { ...h, 'if-match': `"${p.draft.revision}"` }, data: { content } })).status()).toBe(200);
    const v = await (await owner.c.post('/api/v1/processes/validate', { headers: h, data: { content } })).json();
    expect(v.ok).toBe(false);
    const problem = (v.problems as { node: string | null; message: string; severity: string }[]).find((x) => x.severity === 'error' && x.node === 'security');
    expect(problem?.message).toBe("There's no group called “Nobody Here” in this workspace.");
    const pub = await owner.c.post(`/api/v1/processes/${id}/publish`, { headers: h });
    expect(pub.status()).toBe(422);
    expect(JSON.stringify(await pub.json())).toContain('Nobody Here');
  });

  test('E2E-R3-PROC-03 drafts are saved with If-Match; a published process opens a new draft', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const { id } = await (await owner.c.post('/api/v1/processes', { headers: h, data: { key: 'ops.simple', title: 'Simple', template: 'hr.leave-request' } })).json();
    const p = await (await owner.c.get(`/api/v1/processes/${id}`, { headers: h })).json();
    expect((await owner.c.put(`/api/v1/processes/${id}/draft`, { headers: h, data: { content: p.draft.content } })).status()).toBe(428);
    expect((await owner.c.put(`/api/v1/processes/${id}/draft`, { headers: { ...h, 'if-match': `"${p.draft.revision}"` }, data: { content: p.draft.content } })).status()).toBe(200);
    expect((await owner.c.put(`/api/v1/processes/${id}/draft`, { headers: { ...h, 'if-match': `"${p.draft.revision}"` }, data: { content: p.draft.content } })).status()).toBe(412);
    expect((await owner.c.post(`/api/v1/processes/${id}/publish`, { headers: h })).status()).toBe(200);
    const after = await (await owner.c.get(`/api/v1/processes/${id}`, { headers: h })).json();
    expect(after.draft).toBeNull();
    const reopened = await owner.c.put(`/api/v1/processes/${id}/draft`, { headers: h, data: { content: after.published.content } });
    expect(reopened.status(), await reopened.text()).toBe(200);
    const v2 = await (await owner.c.get(`/api/v1/processes/${id}`, { headers: h })).json();
    expect(v2.draft.version_no).toBe(2);
    expect(v2.draft.content.ir.process.version).toBe(2);
  });

  test('E2E-R3-PROC-04 a test run shows the path for these answers and choices, and stores nothing', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const { id } = await (await owner.c.post('/api/v1/processes', { headers: h, data: { key: 'ops.leave-test', title: 'Leave test', template: 'hr.leave-request' } })).json();
    const content = (await (await owner.c.get(`/api/v1/processes/${id}`, { headers: h })).json()).draft.content;
    const input = { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23' };
    const run = async (choices: Record<string, string[]>) => (await owner.c.post('/api/v1/processes/test-run', { headers: h, data: { content, input, choices } })).json();
    const plain = await run({});
    expect(plain).toMatchObject({ result: 'approved', ended_at: 'Approved' });
    expect(plain.steps).toEqual([expect.objectContaining({ node: 'manager_approval', who: "the requester's manager", outcome: 'approved' })]);
    const back = await run({ manager_approval: ['changes_requested', 'rejected'] });
    expect(back.steps.map((s: { outcome: string }) => s.outcome)).toEqual(['changes_requested', 'submitted', 'rejected']);
    expect(back.result).toBe('rejected');
    const bad = await owner.c.post('/api/v1/processes/test-run', { headers: h, data: { content, input, choices: { manager_approval: ['maybe'] } } });
    expect(bad.status()).toBe(422);
    // Nothing was stored: no requests exist.
    expect((await (await owner.c.get('/api/v1/requests?scope=all', { headers: h })).json()).items).toEqual([]);
  });
});
