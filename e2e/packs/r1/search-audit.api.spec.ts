import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-SRCH-01..03 — permission-filtered search with full-text + trigram matching (BPMS-049; docs/04 §6, docs/05 §2.4).
// E2E-R1-AUDX-01..06 — audit explorer: filters, keyset pages, view-own, timelines, verification, saved queries,
// audited exports with step-up (BPMS-064; FR-T01/T03/T04).

type Req = Parameters<typeof seedUser>[0];
type Hit = { type: string; id: string; title: string; subtitle: string | null; snippet: string | null };
type Ev = { seq: number; action: string; category: string; actor_subject_ref: string | null; actor_name: string | null; object_id: string | null; diff: Record<string, unknown> | null };

async function joinAs(request: Req, owner: Session, ws: string, tag: string, role: string, mfa = false) {
  const email = await seedUser(request, tag);
  const inv = await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: [role] } });
  expect(inv.status(), await inv.text()).toBe(201);
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });
  if (mfa) return enter(email, ws);
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: { authorization: `Bearer ${pw.token}` }, data: { workspace_id: ws } })).json();
  return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` } };
}

const doc = (text: string) => ({ blocks: [{ type: 'heading', text: 'Purpose' }, { type: 'paragraph', text }] });

async function publish(s: Session, kind: string, key: string, title: string, text: string) {
  const res = await s.c.post('/api/v1/artifacts', { headers: s.headers, data: { kind, key, title, content: doc(text) } });
  expect(res.status(), await res.text()).toBe(201);
  const { artifact, version } = await res.json();
  await s.c.post(`/api/v1/versions/${version.id}/submit`, { headers: s.headers });
  await s.c.post(`/api/v1/versions/${version.id}/publish`, { headers: s.headers, data: {} });
  return { artifact, version };
}

async function search(s: Session, q: string, types?: string) {
  const p = new URLSearchParams({ q, ...(types ? { types } : {}) });
  const res = await s.c.get(`/api/v1/search?${p}`, { headers: s.headers });
  expect(res.status(), await res.text()).toBe(200);
  return (await res.json()) as { items: Hit[]; types: string[] };
}

async function events(s: Session, params: Record<string, string>) {
  const res = await s.c.get(`/api/v1/audit/events?${new URLSearchParams(params)}`, { headers: s.headers });
  expect(res.status(), await res.text()).toBe(200);
  return (await res.json()) as { items: Ev[]; next_cursor: string | null };
}

test.describe('search @r1 @search', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-SRCH-01 ranked results across SOPs, policies, people, departments and groups with highlighted snippets; typos and prefixes match', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    await publish(owner, 'sop', 'it.onboarding', 'IT onboarding', 'Order laptops for every new starter <script>alert(1)</script>.');
    await publish(owner, 'policy', 'fin.expenses', 'Expense claims', 'Submit receipts within 30 days.');
    expect((await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'Finance' } })).status()).toBe(201);
    expect((await owner.c.post('/api/v1/groups', { headers: h, data: { name: 'Laptop champions', description: 'Hardware helpers' } })).status()).toBe(201);

    const laptop = await search(owner, 'laptop');
    expect(laptop.items.map((i) => [i.type, i.title])).toEqual([['group', 'Laptop champions'], ['sop', 'IT onboarding']]);
    const sop = laptop.items.find((i) => i.type === 'sop');
    expect(sop?.snippet).toContain('<mark>laptops</mark>');
    expect(sop?.snippet).not.toContain('<script>');
    expect((await search(owner, 'expnse')).items.map((i) => i.title)).toEqual(['Expense claims']);
    expect((await search(owner, 'Fin', 'department')).items.map((i) => i.title)).toEqual(['Finance']);
    expect((await search(owner, 'olivia')).items.map((i) => [i.type, i.title])).toEqual([['member', 'Olivia']]);
    expect((await search(owner, 'receipts', 'sop')).items).toEqual([]);
    expect((await owner.c.get('/api/v1/search?q=%20', { headers: h })).status()).toBe(422);
    expect((await owner.c.get('/api/v1/search?q=x&types=process', { headers: h })).status()).toBe(422);
    expect((await owner.c.get('/api/v1/search?q=x&bogus=1', { headers: h })).status()).toBe(400);
  });

  test('E2E-R1-SRCH-02 results are permission-filtered on the server: a SOP-only reader never sees policies, people or the directory', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    await publish(owner, 'sop', 'hr.leave', 'Leave requests', 'Book leave in the portal. Finance approves.');
    await publish(owner, 'policy', 'hr.finance-travel', 'Finance travel policy', 'Economy class.');
    await owner.c.post('/api/v1/departments', { headers: owner.headers, data: { name: 'Finance' } });
    const role = await owner.c.post('/api/v1/roles', { headers: owner.headers, data: { key: 'sop.reader', name: 'SOP reader', permissions: ['sop:view'] } });
    expect(role.status(), await role.text()).toBe(201);
    const reader = await joinAs(request, owner, ws.id, 'reader', 'sop.reader');
    const all = await search(reader, 'finance');
    expect(all.types).toEqual(['sop']);
    expect(all.items.map((i) => [i.type, i.title])).toEqual([['sop', 'Leave requests']]);
    expect((await search(reader, 'olivia')).items).toEqual([]);
    expect((await search(reader, 'finance', 'policy,department')).items).toEqual([]);
    // The owner sees every type.
    expect((await search(owner, 'finance')).items.map((i) => i.type).sort()).toEqual(['department', 'policy', 'sop']);
  });

  test('E2E-R1-SRCH-03 search is workspace-isolated and the index follows edits at once', async ({ request }) => {
    const a = await workspaceWithOwner(request);
    const b = await workspaceWithOwner(request);
    const word = `zephyr${Date.now().toString(36)}`;
    await publish(b.owner, 'sop', 'ops.secret', 'Other workspace SOP', `The ${word} procedure.`);
    expect((await search(b.owner, word)).items).toHaveLength(1);
    expect((await search(a.owner, word)).items).toEqual([]);
    const { version } = await (await a.owner.c.post('/api/v1/artifacts', { headers: a.owner.headers, data: { kind: 'sop', key: 'ops.draft', title: 'Draft SOP', content: doc('Nothing yet.') } })).json();
    expect((await search(a.owner, `${word}x`)).items).toEqual([]);
    await a.owner.c.patch(`/api/v1/versions/${version.id}`, { headers: a.owner.headers, data: { content: doc(`Now covers ${word}x checks.`) } });
    expect((await search(a.owner, `${word}x`)).items.map((i) => i.title)).toEqual(['Draft SOP']);
  });
});

test.describe('audit explorer @r1 @audit', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-AUDX-01 filters (action prefix, category, object, actor, member, time) and keyset pages newest first with actor names', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const { artifact, version } = await publish(owner, 'sop', 'audx.one', 'Audit one', 'One.');
    await publish(owner, 'sop', 'audx.two', 'Audit two', 'Two.');
    const all = await events(owner, { action: 'version.', 'page[size]': '100' });
    expect(all.items.length).toBeGreaterThanOrEqual(6);
    expect(all.items.every((e) => e.action.startsWith('version.'))).toBe(true);
    // Keyset pages of 2 concatenate to the same list, strictly descending, with no duplicates.
    const paged: number[] = [];
    let cursor: string | null = null;
    do {
      const page = await events(owner, { action: 'version.', 'page[size]': '2', ...(cursor ? { 'page[cursor]': cursor } : {}) });
      expect(page.items.length).toBeLessThanOrEqual(2);
      paged.push(...page.items.map((e) => e.seq));
      cursor = page.next_cursor;
    } while (cursor);
    expect(paged).toEqual(all.items.map((e) => e.seq));
    expect(paged.every((s, i) => i === 0 || paged[i - 1]! > s)).toBe(true);
    // Object, category and actor filters; names are resolved for members.
    const created = await events(owner, { object_type: 'artifact_version', object_id: version.id, action: 'artifact.created' });
    expect(created.items).toHaveLength(1);
    expect(created.items[0]).toMatchObject({ category: 'content', actor_name: 'Olivia', diff: { artifact_id: artifact.id } });
    const subject = created.items[0]!.actor_subject_ref!;
    expect((await events(owner, { actor: subject, category: 'content' })).items.every((e) => e.actor_subject_ref === subject)).toBe(true);
    const ownerMember = (await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json()).items.find((m: { display_name: string }) => m.display_name === 'Olivia');
    expect((await events(owner, { member: ownerMember.id, action: 'artifact.created' })).items).toHaveLength(2);
    expect((await events(owner, { result: 'denied', action: 'artifact.' })).items).toEqual([]);
    const future = new Date(Date.now() + 365 * 86_400_000).toISOString();
    expect((await events(owner, { from: future })).items).toEqual([]);
    expect((await owner.c.get('/api/v1/audit/events?category=bogus', { headers: owner.headers })).status()).toBe(422);
    expect((await owner.c.get('/api/v1/audit/events?page[cursor]=abc', { headers: owner.headers })).status()).toBe(422);
  });

  test('E2E-R1-AUDX-02 audit:view_own members see only their own events and cannot verify or export', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    await publish(owner, 'sop', 'audx.own', 'Own', 'Own.');
    const member = await joinAs(request, owner, ws.id, 'plain', 'member');
    const mine = await events(member, { 'page[size]': '200' });
    expect(mine.items.length).toBeGreaterThanOrEqual(1);
    const me = new Set(mine.items.map((e) => e.actor_subject_ref));
    expect(me.size).toBe(1);
    expect(mine.items.map((e) => e.action)).toContain('member.joined');
    expect((await events(member, { action: 'artifact.' })).items).toEqual([]);
    expect((await member.c.get('/api/v1/audit/verify', { headers: member.headers })).status()).toBe(403);
    expect((await member.c.post('/api/v1/audit/exports', { headers: member.headers, data: {} })).status()).toBe(403);
    // No audit permission at all: 403.
    await owner.c.post('/api/v1/roles', { headers: owner.headers, data: { key: 'no.audit', name: 'No audit', permissions: ['sop:view'] } });
    const outsider = await joinAs(request, owner, ws.id, 'noaudit', 'no.audit');
    expect((await outsider.c.get('/api/v1/audit/events', { headers: outsider.headers })).status()).toBe(403);
  });

  test('E2E-R1-AUDX-03 an artifact timeline spans its versions oldest first', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const { artifact } = await publish(owner, 'sop', 'audx.tl', 'Timeline', 'v1');
    const v2 = await (await owner.c.post(`/api/v1/artifacts/${artifact.id}/versions`, { headers: owner.headers })).json();
    await owner.c.patch(`/api/v1/versions/${v2.id}`, { headers: owner.headers, data: { content: doc('v2') } });
    const tl = await (await owner.c.get(`/api/v1/audit/objects/artifact/${artifact.id}/timeline`, { headers: owner.headers })).json();
    expect(tl.items.map((e: Ev) => e.action)).toEqual(['artifact.created', 'version.submitted', 'version.approved', 'version.published', 'version.created', 'version.edited']);
    const one = await (await owner.c.get(`/api/v1/audit/objects/artifact_version/${v2.id}/timeline`, { headers: owner.headers })).json();
    expect(one.items.map((e: Ev) => e.action)).toEqual(['version.created', 'version.edited']);
    expect(tl.next_cursor).toBeNull();
  });

  test('E2E-R1-AUDX-04 chain verification reports an intact workspace chain', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    await publish(owner, 'sop', 'audx.verify', 'Verify', 'x');
    const v = await (await owner.c.get('/api/v1/audit/verify', { headers: owner.headers })).json();
    expect(v.ok).toBe(true);
    expect(v.first_broken).toBeNull();
    expect(v.checked).toBeGreaterThan(3);
  });

  test('E2E-R1-AUDX-05 saved queries are private to their owner, validated and audited', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const member = await joinAs(request, owner, ws.id, 'saver', 'member');
    const created = await owner.c.post('/api/v1/audit/saved-queries', { headers: h, data: { name: 'Publications', filters: { action: 'version.published', category: 'content' } } });
    expect(created.status(), await created.text()).toBe(201);
    const sq = await created.json();
    expect(sq.filters).toEqual({ action: 'version.published', category: 'content' });
    expect((await owner.c.post('/api/v1/audit/saved-queries', { headers: h, data: { name: 'publications' } })).status()).toBe(409);
    expect((await owner.c.post('/api/v1/audit/saved-queries', { headers: h, data: { name: 'Bad', filters: { colour: 'red' } } })).status()).toBe(422);
    expect((await owner.c.post('/api/v1/audit/saved-queries', { headers: h, data: { name: 'Bad', filters: { result: 'maybe' } } })).status()).toBe(422);
    expect((await member.c.post('/api/v1/audit/saved-queries', { headers: member.headers, data: { name: 'Mine' } })).status()).toBe(201);
    expect((await (await owner.c.get('/api/v1/audit/saved-queries', { headers: h })).json()).items.map((q: { name: string }) => q.name)).toEqual(['Publications']);
    expect((await (await member.c.get('/api/v1/audit/saved-queries', { headers: member.headers })).json()).items.map((q: { name: string }) => q.name)).toEqual(['Mine']);
    expect((await member.c.delete(`/api/v1/audit/saved-queries/${sq.id}`, { headers: member.headers })).status()).toBe(404);
    expect((await owner.c.delete(`/api/v1/audit/saved-queries/${sq.id}`, { headers: h })).status()).toBe(204);
    const trail = await events(owner, { action: 'audit.query_' });
    expect(trail.items.map((e) => e.action)).toEqual(['audit.query_deleted', 'audit.query_saved', 'audit.query_saved']);
    expect(trail.items[0]).toMatchObject({ object_id: sq.id, category: 'admin' });
  });

  test('E2E-R1-AUDX-06 exports need audit:export and a recent sign-in, honour filters, and are themselves audited', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    await publish(owner, 'sop', 'audx.export', 'Export', 'x');
    const csv = await owner.c.post('/api/v1/audit/exports', { headers: h, data: { format: 'csv', filters: { action: 'version.' } } });
    expect(csv.status(), await csv.text()).toBe(200);
    expect(csv.headers()['content-type']).toContain('text/csv');
    expect(csv.headers()['content-disposition']).toMatch(/^attachment; filename="audit-\d+\.csv"$/);
    expect(csv.headers()['procera-export-truncated']).toBe('false');
    const lines = (await csv.text()).trim().split('\r\n');
    expect(lines[0]).toBe('seq,occurred_at,actor_subject_ref,actor_name,actor_kind,on_behalf_of,ip,user_agent,category,action,object_type,object_id,result,reason,policy_ids,diff,trace_id,hash');
    expect(lines.length - 1).toBe(Number(csv.headers()['procera-export-rows']));
    expect(lines.slice(1).every((l) => /,version\./.test(l))).toBe(true);
    const jsonl = await owner.c.post('/api/v1/audit/exports', { headers: h, data: { format: 'jsonl', filters: { action: 'artifact.created' } } });
    const rows = (await jsonl.text()).trim().split('\n').map((l) => JSON.parse(l));
    expect(rows.map((r) => r.action)).toEqual(['artifact.created']);
    // A sign-in older than the step-up window is refused.
    const old = await (await owner.c.post('/__test/session/backdate', { headers: h, data: { seconds: 600 } })).json();
    const stale = await owner.c.post('/api/v1/audit/exports', { headers: { authorization: `Bearer ${old.access_token}` }, data: {} });
    expect([stale.status(), (await stale.json()).code]).toEqual([401, 'step_up_required']);
    // Both exports are on the record, with what was exported.
    const exp = await events(owner, { category: 'export' });
    expect(exp.items.map((e) => e.action)).toEqual(['audit.exported', 'audit.exported']);
    expect(exp.items[0]!.diff).toMatchObject({ format: 'jsonl', rows: 1, truncated: false, filters: { action: 'artifact.created' } });
    expect((await owner.c.post('/api/v1/audit/exports', { headers: h, data: { format: 'xlsx' } })).status()).toBe(422);
  });
});
