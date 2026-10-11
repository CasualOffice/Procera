import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-AUDX-07..08 — audit query language and background exports (BPMS-064; docs/08 §7.5).

test.describe('audit query and background exports @r1 @audit', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-AUDX-07 POST /audit/query: AND/OR groups over allow-listed fields, sort, keyset pages, limits', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    for (const key of ['q.one', 'q.two', 'q.three']) {
      await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key, title: key, content: { blocks: [] } } });
    }
    const q = (body: unknown) => owner.c.post('/api/v1/audit/query', { headers: h, data: body });
    const created = await (await q({ filter: { and: [{ field: 'category', op: 'eq', value: 'content' }, { field: 'action', op: 'prefix', value: 'artifact.' }, { field: 'occurred_at', op: 'within', value: 'P30D' }] }, sort: [{ field: 'occurred_at', dir: 'asc' }], page: { size: 2 } })).json();
    expect(created.items).toHaveLength(2);
    expect(created.items.every((e: { action: string }) => e.action.startsWith('artifact.'))).toBe(true);
    expect(created.items[0].seq).toBeLessThan(created.items[1].seq); // ascending
    const next = await (await q({ filter: { and: [{ field: 'category', op: 'eq', value: 'content' }, { field: 'action', op: 'prefix', value: 'artifact.' }] }, sort: [{ field: 'occurred_at', dir: 'asc' }], page: { size: 2, cursor: created.next_cursor } })).json();
    expect(next.items[0].seq).toBeGreaterThan(created.items[1].seq);
    const either = await (await q({ filter: { or: [{ field: 'action', op: 'in', value: ['artifact.created'] }, { field: 'category', op: 'eq', value: 'auth' }] } })).json();
    expect(either.items.every((e: { action: string; category: string }) => e.action === 'artifact.created' || e.category === 'auth')).toBe(true);
    for (const bad of [
      { filter: { field: 'password', op: 'eq', value: 'x' } },
      { filter: { field: 'category', op: 'eq', value: 'nope' } },
      { filter: { field: 'occurred_at', op: 'within', value: '30 days' } },
      { filter: { and: [{ or: [{ and: [{ field: 'category', op: 'eq', value: 'auth' }] }] }] } },
      { sort: [{ field: 'action', dir: 'asc' }] },
    ]) {
      expect((await q(bad)).status(), JSON.stringify(bad)).toBe(422);
    }
  });

  test('E2E-R1-AUDX-08 a background export runs past the synchronous cap path, stores an encrypted file the requester downloads, notifies and is audited', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'x.export', title: 'Exported', content: { blocks: [] } } });
    const queued = await owner.c.post('/api/v1/audit/exports', { headers: h, data: { format: 'csv', filters: { category: 'content' }, background: true } });
    expect(queued.status()).toBe(202);
    const { id } = await queued.json();
    await expect.poll(async () => {
      await request.post('/__test/audit/exports/run');
      return (await (await owner.c.get(`/api/v1/audit/exports/${id}`, { headers: h })).json()).state;
    }).toBe('done');
    const job = await (await owner.c.get(`/api/v1/audit/exports/${id}`, { headers: h })).json();
    expect(job.rows).toBeGreaterThanOrEqual(1);
    const { url } = await (await owner.c.get(`/api/v1/files/${job.file_id}/download-url`, { headers: h })).json();
    const csv = await (await request.get(url)).text();
    expect(csv.split('\r\n')[0]).toMatch(/^seq,occurred_at,actor_subject_ref/);
    expect(csv).toContain(',content,');
    // The job holds its file: weeks later it is still not an orphan, so it is never purged.
    await request.post('/__test/files/backdate', { data: { workspace: ws.id, file: job.file_id, seconds: 60 * 86_400 } });
    const orphans = await (await owner.c.get('/api/v1/files/orphans', { headers: h })).json();
    expect(orphans.items.map((f: { id: string }) => f.id)).not.toContain(job.file_id);
    const list = await (await owner.c.get('/api/v1/audit/exports', { headers: h })).json();
    expect(list.items[0]).toMatchObject({ id, state: 'done' });
    const inbox = await (await owner.c.get('/api/v1/notifications', { headers: h })).json();
    expect(inbox.items.map((n: { kind: string }) => n.kind)).toContain('audit_export_ready');
    const audit = await (await owner.c.get('/api/v1/audit/events?action=audit.export', { headers: h })).json();
    expect(audit.items.map((e: { action: string }) => e.action)).toEqual(expect.arrayContaining(['audit.export_requested', 'audit.exported']));
  });
});
