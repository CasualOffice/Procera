import { createHash } from 'node:crypto';
import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser, serverNow } from '../../fixtures/accounts';
import { EICAR, upload } from '../../fixtures/files';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-FILE-01..03 — files pipeline (BPMS-060/062; FR-F01–F03, F05–F07).

type Req = Parameters<typeof seedUser>[0];
const doc = (text: string) => ({ blocks: [{ type: 'paragraph', text }] });

async function member(request: Req, owner: Session, ws: string, tag: string) {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const bearer = { authorization: `Bearer ${pw.token}` };
  await pw.c.post('/api/v1/invitations/accept', { headers: bearer, data: { token } });
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: bearer, data: { workspace_id: ws } })).json();
  return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` } };
}

test.describe('files @r1 @files', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-FILE-01 a chunked upload resumes, is hashed, typed by content and downloads only through a valid signed link', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const pdf = Buffer.concat([Buffer.from('%PDF-1.7\n'), Buffer.alloc(2_500_000, 'x')]);
    const created = await (await owner.c.post('/api/v1/files', { headers: h, data: { name: 'report.txt', size: pdf.length, content_type: 'text/plain' } })).json();
    const id = created.file.id;
    const put = (off: number, end: number) => owner.c.put(`/api/v1/files/${id}/content?offset=${off}`, { headers: { ...h, 'content-type': 'application/octet-stream' }, data: pdf.subarray(off, end) });
    expect((await put(0, 1_000_000)).status()).toBe(200);
    // The connection drops; the client asks how much arrived and resumes from there.
    expect((await put(500_000, 900_000)).status()).toBe(409);
    const at = (await (await owner.c.get(`/api/v1/files/${id}`, { headers: h })).json()).file.received;
    expect(at).toBe(1_000_000);
    expect((await owner.c.post(`/api/v1/files/${id}/complete`, { headers: h })).status()).toBe(409); // not all bytes yet
    expect((await put(at, pdf.length)).status()).toBe(200);
    const done = await (await owner.c.post(`/api/v1/files/${id}/complete`, { headers: h })).json();
    expect([done.state, done.sniffed_type, done.declared_type, done.sha256]).toEqual(['clean', 'application/pdf', 'text/plain', createHash('sha256').update(pdf).digest('hex')]);

    const { url, expires_at } = await (await owner.c.get(`/api/v1/files/${id}/download-url`, { headers: h })).json();
    const left = Date.parse(expires_at) - (await serverNow(request));
    expect(left).toBeGreaterThan(0);
    expect(left).toBeLessThanOrEqual(5 * 60_000);
    const got = await request.get(url); // no bearer: the link itself is the credential
    expect(got.status()).toBe(200);
    expect(got.headers()['content-disposition']).toContain('report.txt');
    expect(got.headers()['x-content-type-options']).toBe('nosniff');
    expect(Buffer.from(await got.body()).equals(pdf)).toBe(true);
    const tampered = url.slice(0, -2) + (url.endsWith('A') ? 'BB' : 'AA');
    expect((await request.get(tampered)).status()).toBe(403);
    // Another member cannot see or download the uploader's loose file.
    const m = await member(request, owner, ws.id, 'filepeer');
    expect((await m.c.get(`/api/v1/files/${id}`, { headers: m.headers })).status()).toBe(404);
    expect((await m.c.get(`/api/v1/files/${id}/download-url`, { headers: m.headers })).status()).toBe(404);
    const audit = await (await owner.c.get('/api/v1/audit/events?action=file.', { headers: h })).json();
    expect(audit.items.map((e: { action: string }) => e.action)).toEqual(expect.arrayContaining(['file.uploaded', 'file.downloaded']));
  });

  test('E2E-R1-FILE-02 EICAR is quarantined and never downloadable; executables are rejected by content; oversize is refused', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const bad = await upload(owner, 'invoice.txt', Buffer.from(EICAR));
    expect([bad.state, bad.scan_result]).toEqual(['quarantined', 'infected: Eicar-Test-Signature']);
    expect((await owner.c.get(`/api/v1/files/${bad.id}/download-url`, { headers: h })).status()).toBe(409);
    const exe = await upload(owner, 'notes.pdf', Buffer.concat([Buffer.from('MZ'), Buffer.alloc(64, 0)]));
    expect([exe.state, exe.sniffed_type]).toEqual(['rejected', 'application/x-msdownload']);
    const html = await upload(owner, 'page.txt', Buffer.from('<!doctype html><script>alert(1)</script>'));
    expect(html.state).toBe('rejected');
    expect((await owner.c.post('/api/v1/files', { headers: h, data: { name: 'huge.bin', size: 500 * 1024 * 1024 } })).status()).toBe(413);
    expect((await owner.c.post('/api/v1/files', { headers: h, data: { name: 'empty.txt', size: 0 } })).status()).toBe(422);
    const usage = await (await owner.c.get('/api/v1/files/usage', { headers: h })).json();
    expect(usage.states).toMatchObject({ quarantined: 1, rejected: 2 });
  });

  test('E2E-R1-FILE-03 attachments: a draft takes files, a published version keeps them fixed, readers download them; orphans are listed and purged', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'ops.forms', title: 'Forms', content: doc('See the attached form.') } })).json();
    const f = await upload(owner, 'form.txt', Buffer.from('Name: ____'));
    const loose = await upload(owner, 'scratch.txt', Buffer.from('notes'));
    expect((await owner.c.post(`/api/v1/files/${f.id}/links`, { headers: h, data: { object_type: 'artifact_version', object_id: r.version.id } })).status()).toBe(204);
    const att = await (await owner.c.get(`/api/v1/versions/${r.version.id}/attachments`, { headers: h })).json();
    expect([att.editable, att.items.map((x: { name: string }) => x.name)]).toEqual([true, ['form.txt']]);
    await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: h });
    await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: h, data: {} });
    expect((await owner.c.delete(`/api/v1/files/${f.id}/links/artifact_version/${r.version.id}`, { headers: h })).status()).toBe(409);
    expect((await owner.c.delete(`/api/v1/files/${f.id}`, { headers: h })).status()).toBe(409);
    // A reader of the published SOP downloads its attachment.
    const m = await member(request, owner, ws.id, 'formreader');
    const { url } = await (await m.c.get(`/api/v1/files/${f.id}/download-url`, { headers: m.headers })).json();
    expect(await (await request.get(url)).text()).toBe('Name: ____');
    // The unattached file becomes an orphan after the orphan period.
    expect((await request.post('/__test/files/backdate', { data: { workspace: ws.id, file: loose.id, seconds: 8 * 86_400 } })).status()).toBe(204);
    const orphans = await (await owner.c.get('/api/v1/files/orphans', { headers: h })).json();
    expect(orphans.items.map((x: { id: string }) => x.id)).toEqual([loose.id]);
    expect((await m.c.post('/api/v1/files/orphans/purge', { headers: m.headers })).status()).toBe(403);
    expect((await (await owner.c.post('/api/v1/files/orphans/purge', { headers: h })).json()).purged).toBe(1);
    expect((await owner.c.get(`/api/v1/files/${loose.id}`, { headers: h })).status()).toBe(404);
  });
});
