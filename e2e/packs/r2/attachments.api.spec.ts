import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { EICAR, PNG_1PX, tinyPdf, upload } from '../../fixtures/files';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R2-40..43 — requests slice 3 (BPMS-060/101/103) over the API: "who approves this" before
// sending, files on requests and with decisions (scan, ownership, preview permissions, send back),
// and the policy version a decider was shown.

type Req = Parameters<typeof seedUser>[0];
type Member = Session & { email: string; membership: string; name: string };

async function joinAs(request: Req, owner: Session, ws: string, tag: string): Promise<Member> {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const bearer = { authorization: `Bearer ${pw.token}` };
  const joined = await (await pw.c.post('/api/v1/invitations/accept', { headers: bearer, data: { token } })).json();
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: bearer, data: { workspace_id: ws } })).json();
  return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` }, email, membership: joined.membership_id, name: tag };
}

async function reportsTo(owner: Session, who: Member, manager: Member) {
  const r = await owner.c.patch(`/api/v1/members/${who.membership}`, { headers: owner.headers, data: { manager_membership_id: manager.membership } });
  expect(r.status(), await r.text()).toBe(200);
}

const key = () => `e2e-${Date.now()}-${Math.random().toString(36).slice(2)}`;
const start = (m: Session, type: string, input: Record<string, unknown>) =>
  m.c.post('/api/v1/requests', { headers: { ...m.headers, 'idempotency-key': key() }, data: { type, input } });
const preview = async (m: Session, type: string, input: Record<string, unknown>) => {
  const r = await m.c.post('/api/v1/requests/preview', { headers: m.headers, data: { type, input } });
  expect(r.status(), await r.text()).toBe(200);
  return (await r.json()) as { summary: string; unassigned: boolean; steps: { node: string; applies: boolean | null }[] };
};

type Work = { id: string; revision: number; request: { id: string }; policy: { title: string; version: string; version_id: string } | null; attachments: { name: string }[] };
const itemFor = async (m: Session, id: string): Promise<Work> => {
  const items = (await (await m.c.get('/api/v1/work?state=open', { headers: m.headers })).json()).items as Work[];
  const w = items.find((x) => x.request.id === id);
  expect(w, 'an open step for this request').toBeTruthy();
  return w!;
};
const decide = (m: Session, w: Work, data: Record<string, unknown>) =>
  m.c.post(`/api/v1/work/${w.id}/decisions`, { headers: { ...m.headers, 'if-match': `"${w.revision}"` }, data });
const detail = async (m: Session, id: string) => (await m.c.get(`/api/v1/requests/${id}`, { headers: m.headers })).json();

const leave = { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23', reason: 'Holidays' };
const expense = (amount?: string) => ({ ...(amount ? { amount: { amount, currency: 'EUR' } } : {}), category: 'travel', description: 'Client visit', expense_date: '2026-10-01' });

test.describe('requests slice 3 @r2 @requests', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-40 who approves this: the manager first, finance above 500 EUR, nobody reads as unassigned; nothing is saved', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [olivia, ann, otto] = [await joinAs(request, owner, ws.id, 'Olivia Owner'), await joinAs(request, owner, ws.id, 'Ann'), await joinAs(request, owner, ws.id, 'Otto')];
    await reportsTo(owner, ann, olivia);

    const open = await preview(ann, 'hr.expense-claim', expense());
    expect(open.summary).toMatch(/^Olivia Owner \(your manager\), then .+ if the amount is over 500 EUR$/);
    expect((await preview(ann, 'hr.expense-claim', expense('900'))).summary).toMatch(/\(the amount is over 500 EUR\)$/);
    const small = await preview(ann, 'hr.expense-claim', expense('120'));
    expect(small.summary).toBe('Olivia Owner (your manager)');
    expect(small.steps.find((s) => s.node === 'finance_approval')?.applies).toBe(false);
    // Bad or unknown answers are left out, not refused: the form is still being filled.
    expect((await preview(ann, 'hr.expense-claim', { amount: 'lots', nope: 1 })).summary).toContain('Olivia Owner (your manager)');

    // Otto has no manager; the owner holds every role, so the fallback names them unless SoD drops them.
    const lone = await preview(otto, 'hr.leave-request', leave);
    expect(lone.summary.length).toBeGreaterThan(0);
    expect(lone.unassigned || /as nobody else could be found/.test(lone.summary)).toBe(true);

    const mine = (await (await ann.c.get('/api/v1/requests', { headers: ann.headers })).json()).items;
    expect(mine).toHaveLength(0);
    expect((await ann.c.post('/api/v1/requests/preview', { headers: ann.headers, data: { type: 'no.such-type', input: {} } })).status()).toBe(422);
  });

  test('E2E-R2-41 an image and a PDF attach to a leave request; quarantined and other people’s files are refused; only people on the request may preview them', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [olivia, ann, ben] = [await joinAs(request, owner, ws.id, 'Olivia Owner'), await joinAs(request, owner, ws.id, 'Ann'), await joinAs(request, owner, ws.id, 'Ben')];
    await reportsTo(owner, ann, olivia);
    const types = (await (await ann.c.get('/api/v1/request-types', { headers: ann.headers })).json()).items as { key: string; form: { fields: { name: string; type: string; multiple?: boolean; required: boolean }[] } }[];
    for (const t of types) expect(t.form.fields.find((f) => f.name === 'attachments')).toMatchObject({ type: 'file', multiple: true, required: false });

    const png = await upload(ann, 'note.png', PNG_1PX);
    const pdf = await upload(ann, 'certificate.pdf', tinyPdf('Doctor note'));
    const virus = await upload(ann, 'virus.pdf', Buffer.from(EICAR));
    expect(virus.state).toBe('quarantined');
    const bens = await upload(ben, 'bens.pdf', tinyPdf('Not yours'));

    const refused = async (ids: string[], code: string) => {
      const r = await start(ann, 'hr.leave-request', { ...leave, attachments: ids });
      expect(r.status(), await r.text()).toBe(422);
      expect((await r.json()).errors[0]).toMatchObject({ pointer: '/input/attachments', code });
    };
    await refused([virus.id], 'quarantined');
    await refused([bens.id], 'not_yours');

    const r = await start(ann, 'hr.leave-request', { ...leave, supporting_document: [pdf.id], attachments: [png.id, pdf.id] });
    expect(r.status(), await r.text()).toBe(201);
    const { id } = await r.json();
    const d = await detail(ann, id);
    expect(d.attachments.map((a: { name: string; source: string }) => `${a.source}: ${a.name}`)).toEqual(['Supporting document: certificate.pdf', 'Attachments: note.png', 'Attachments: certificate.pdf']);

    // The manager is on the request: both preview. Ben is not: 404, as if the files did not exist.
    for (const [f, kind] of [[png, 'image'], [pdf, 'pdf']] as const) {
      const p = await olivia.c.get(`/api/v1/files/${f.id}/preview-url`, { headers: olivia.headers });
      expect(p.status(), await p.text()).toBe(200);
      expect((await p.json()).kind).toBe(kind);
      expect((await ben.c.get(`/api/v1/files/${f.id}/preview-url`, { headers: ben.headers })).status()).toBe(404);
    }
    expect((await itemFor(olivia, id)).attachments.map((a) => a.name)).toContain('note.png');
    // Files on a request are part of its record: the uploader can no longer delete them.
    expect((await ann.c.delete(`/api/v1/files/${png.id}`, { headers: ann.headers })).status()).toBe(409);
  });

  test('E2E-R2-42 the approver attaches a file with the decision; a sent-back request resubmits with a file removed', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [olivia, ann] = [await joinAs(request, owner, ws.id, 'Olivia Owner'), await joinAs(request, owner, ws.id, 'Ann')];
    await reportsTo(owner, ann, olivia);
    const [a, b] = [await upload(ann, 'a.pdf', tinyPdf('A')), await upload(ann, 'b.png', PNG_1PX)];
    const { id } = await (await start(ann, 'hr.leave-request', { ...leave, attachments: [a.id, b.id] })).json();

    let w = await itemFor(olivia, id);
    expect((await decide(olivia, w, { outcome: 'changes_requested', comment: 'Only the certificate, please' })).status()).toBe(200);
    w = await itemFor(ann, id);
    const re = await decide(ann, w, { outcome: 'submitted', fields: { ...leave, attachments: [a.id] } });
    expect(re.status(), await re.text()).toBe(200);
    expect((await detail(ann, id)).attachments.map((x: { name: string }) => x.name)).toEqual(['a.pdf']);

    const signed = await upload(olivia, 'signed.pdf', tinyPdf('Signed'));
    w = await itemFor(olivia, id);
    expect((await decide(olivia, w, { outcome: 'approved', files: [a.id] })).status()).toBe(422); // not hers
    const ok = await decide(olivia, w, { outcome: 'approved', files: [signed.id] });
    expect(ok.status(), await ok.text()).toBe(200);
    const d = await detail(ann, id);
    const decided = d.timeline.find((t: { kind: string; text: string }) => t.kind === 'decided' && /approved/.test(t.text));
    expect(decided.files.map((f: { name: string }) => f.name)).toEqual(['signed.pdf']);
    expect(d.attachments.map((x: { source: string; name: string }) => `${x.source}: ${x.name}`)).toContain('Manager approval decision: signed.pdf');
    // The requester may open what the approver attached.
    expect((await ann.c.get(`/api/v1/files/${signed.id}/download-url`, { headers: ann.headers })).status()).toBe(200);
  });

  test('E2E-R2-43 a request type names its policy; the decider sees its published version and the decision records it', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const [olivia, ann] = [await joinAs(request, owner, ws.id, 'Olivia Owner'), await joinAs(request, owner, ws.id, 'Ann')];
    await reportsTo(owner, ann, olivia);
    const created = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'policy', key: `leave-${Date.now()}`, title: 'Leave and absence', content: { blocks: [{ type: 'paragraph', text: 'Ask early.' }] } } })).json();
    await owner.c.post(`/api/v1/versions/${created.version.id}/submit`, { headers: owner.headers });
    expect((await owner.c.post(`/api/v1/versions/${created.version.id}/publish`, { headers: owner.headers, data: {} })).status()).toBe(200);

    expect((await owner.c.put('/api/v1/settings/requests.types.policy', { headers: owner.headers, data: { value: { 'hr.leave-request': crypto.randomUUID() } } })).status()).toBe(422);
    const set = await owner.c.put('/api/v1/settings/requests.types.policy', { headers: owner.headers, data: { value: { 'hr.leave-request': created.artifact.id } } });
    expect(set.status(), await set.text()).toBe(200);

    const { id } = await (await start(ann, 'hr.leave-request', leave)).json();
    const w = await itemFor(olivia, id);
    expect(w.policy).toMatchObject({ title: 'Leave and absence', version: '1.0', version_id: created.version.id });
    expect((await detail(ann, id)).policy.title).toBe('Leave and absence');
    expect((await decide(olivia, w, { outcome: 'approved', policy_version: crypto.randomUUID() })).status()).toBe(422);
    expect((await decide(olivia, w, { outcome: 'approved', policy_version: w.policy!.version_id })).status()).toBe(200);
    // The work_item.decided diff carries policy_version (asserted in crates/requests/tests/attachments.rs).
  });
});
