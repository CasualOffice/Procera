import { createHash } from 'node:crypto';
import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { PNG_1PX, pdfPages, upload } from '../../fixtures/files';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-SIGN-01..03 — employee e-signing (BPMS-107, FR-DS01–06): an owner sends a two-page PDF to
// two members in order; the second can't sign before the first; a signature must cite the document
// hash the signer was shown; the last signature completes the envelope and the signed copy (values
// stamped, a certificate page appended) has the recorded SHA-256. Declining ends an envelope for
// everyone; the sender can cancel one; non-PDFs are refused; API tokens never reach the ceremony.

async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const s = (await enter(email, ws)) as Session;
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, session: s, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

const sha256 = (b: Buffer) => createHash('sha256').update(b).digest('hex');

test.describe('e-signing @r4 @signing', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-SIGN-01 two signers in order; the hash is checked; the last signature completes it with a certificate', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const ann = await person(request, owner, ws.id, 'Ann Asker');
    const bo = await person(request, owner, ws.id, 'Bo Boss');
    const src = pdfPages(['Offer letter', 'Terms']);
    const file = await upload(owner, 'offer-letter.pdf', src);

    const made = await owner.c.post('/api/v1/envelopes', { headers: h, data: { title: 'Offer letter', message: 'Welcome aboard!', file_id: file.id, order: 'sequential' } });
    expect(made.status(), await made.text()).toBe(201);
    let env = await made.json();
    expect(env).toMatchObject({ state: 'draft', document_hash: sha256(src) });
    expect(env.pages).toHaveLength(2);
    env = await (await owner.c.put(`/api/v1/envelopes/${env.id}/recipients`, { headers: h, data: { recipients: [{ membership_id: ann.id }, { membership_id: bo.id }] } })).json();
    const [ra, rb] = env.recipients as { id: string; name: string }[];
    expect(ra.name).toBe('Ann Asker');
    // Sending needs a signature field per signer.
    const early = await owner.c.post(`/api/v1/envelopes/${env.id}/send`, { headers: h });
    expect(early.status()).toBe(422);
    expect(await early.text()).toContain('Ann Asker');
    const put = await owner.c.put(`/api/v1/envelopes/${env.id}/fields`, {
      headers: h,
      data: {
        fields: [
          { recipient_id: ra.id, kind: 'signature', page: 1, x: 0.1, y: 0.7, w: 0.35, h: 0.08 },
          { recipient_id: ra.id, kind: 'date_signed', page: 1, x: 0.55, y: 0.72, w: 0.3, h: 0.05 },
          { recipient_id: rb.id, kind: 'signature', page: 2, x: 0.1, y: 0.7, w: 0.35, h: 0.08 },
          { recipient_id: rb.id, kind: 'text', page: 2, x: 0.1, y: 0.5, w: 0.5, h: 0.05, label: 'Start date' },
          { recipient_id: rb.id, kind: 'checkbox', page: 2, x: 0.1, y: 0.4, w: 0.05, h: 0.04, label: 'I accept the terms' },
        ],
      },
    });
    expect(put.status(), await put.text()).toBe(200);
    const outside = await owner.c.put(`/api/v1/envelopes/${env.id}/fields`, { headers: h, data: { fields: [{ recipient_id: ra.id, kind: 'signature', page: 3, x: 0.1, y: 0.1, w: 0.2, h: 0.1 }] } });
    expect(outside.status()).toBe(422);
    const sent = await owner.c.post(`/api/v1/envelopes/${env.id}/send`, { headers: h });
    expect(sent.status(), await sent.text()).toBe(200);
    expect((await sent.json()).state).toBe('sent');
    // A sent envelope keeps its fields.
    expect((await owner.c.put(`/api/v1/envelopes/${env.id}/fields`, { headers: h, data: { fields: [] } })).status()).toBe(409);

    // Ann's turn; Bo waits.
    const annBox = (await (await ann.session.c.get('/api/v1/me/signatures', { headers: ann.session.headers })).json()).items;
    expect(annBox[0]).toMatchObject({ title: 'Offer letter', to_sign: true, sender_name: expect.any(String) });
    const boBox = (await (await bo.session.c.get('/api/v1/me/signatures', { headers: bo.session.headers })).json()).items;
    expect(boBox[0].to_sign).toBe(false);
    const sig = (adopted: object, values = {}, hash = sha256(src)) => ({ document_hash: hash, consent: true, adopted, values });
    expect((await bo.session.c.post(`/api/v1/signing/${rb.id}/sign`, { headers: bo.session.headers, data: sig({ kind: 'typed', text: 'Bo Boss' }) })).status()).toBe(409);
    // Someone else's recipient row is not found.
    expect((await bo.session.c.get(`/api/v1/signing/${ra.id}`, { headers: bo.session.headers })).status()).toBe(404);

    const cer = await (await ann.session.c.get(`/api/v1/signing/${ra.id}`, { headers: ann.session.headers })).json();
    expect(cer).toMatchObject({ my_turn: true, envelope: { title: 'Offer letter', document_hash: sha256(src) } });
    expect(cer.fields).toHaveLength(2);
    const doc = await ann.session.c.get(`/api/v1/envelopes/${env.id}/document`, { headers: ann.session.headers });
    expect(doc.headers()['content-type']).toBe('application/pdf');
    expect(sha256(Buffer.from(await doc.body()))).toBe(sha256(src));
    const stale = await ann.session.c.post(`/api/v1/signing/${ra.id}/sign`, { headers: ann.session.headers, data: sig({ kind: 'typed', text: 'Ann Asker' }, {}, 'ab'.repeat(32)) });
    expect(stale.status()).toBe(409);
    const noConsent = await ann.session.c.post(`/api/v1/signing/${ra.id}/sign`, { headers: ann.session.headers, data: { ...sig({ kind: 'typed', text: 'Ann Asker' }), consent: false } });
    expect(noConsent.status()).toBe(422);
    const a = await ann.session.c.post(`/api/v1/signing/${ra.id}/sign`, { headers: ann.session.headers, data: sig({ kind: 'typed', text: 'Ann Asker' }) });
    expect(a.status(), await a.text()).toBe(200);
    expect((await a.json()).state).toBe('sent');
    expect((await ann.session.c.post(`/api/v1/signing/${ra.id}/sign`, { headers: ann.session.headers, data: sig({ kind: 'typed', text: 'Ann Asker' }) })).status()).toBe(409);

    // Bo's turn: a required text and checkbox, a drawn signature.
    const boCer = await (await bo.session.c.get(`/api/v1/signing/${rb.id}`, { headers: bo.session.headers })).json();
    expect(boCer.my_turn).toBe(true);
    const fields = boCer.fields as { id: string; kind: string }[];
    const text = fields.find((f) => f.kind === 'text')!.id;
    const box = fields.find((f) => f.kind === 'checkbox')!.id;
    const drawn = { kind: 'drawn', text: 'Bo Boss', image: `data:image/png;base64,${PNG_1PX.toString('base64')}` };
    expect((await bo.session.c.post(`/api/v1/signing/${rb.id}/sign`, { headers: bo.session.headers, data: sig(drawn, { [text]: '1 December 2026' }) })).status()).toBe(422);
    const b = await bo.session.c.post(`/api/v1/signing/${rb.id}/sign`, { headers: bo.session.headers, data: sig(drawn, { [text]: '1 December 2026', [box]: true }) });
    expect(b.status(), await b.text()).toBe(200);
    expect((await b.json()).state).toBe('completed');

    // The signed copy: values stamped, a certificate page appended, its SHA-256 recorded.
    const done = await (await owner.c.get(`/api/v1/envelopes/${env.id}`, { headers: h })).json();
    expect(done).toMatchObject({ state: 'completed', completed_file_id: expect.any(String) });
    expect(done.recipients.map((r: { state: string }) => r.state)).toEqual(['signed', 'signed']);
    const signed = Buffer.from(await (await owner.c.get(`/api/v1/envelopes/${env.id}/document?signed=true`, { headers: h })).body());
    expect(signed.subarray(0, 5).toString()).toBe('%PDF-');
    expect(sha256(signed)).toBe(done.completed_hash);
    expect(signed.toString('latin1').match(/\/Type\s*\/Page(?!s)/g)!.length).toBe(3);
    expect(signed.toString('latin1')).toContain('(Ann Asker) Tj');
    // Both signers can fetch it; the original is untouched.
    expect((await ann.session.c.get(`/api/v1/envelopes/${env.id}/document?signed=true`, { headers: ann.session.headers })).status()).toBe(200);
    expect(sha256(Buffer.from(await (await owner.c.get(`/api/v1/envelopes/${env.id}/document`, { headers: h })).body()))).toBe(sha256(src));
    // Everyone was told.
    const notes = (await (await ann.session.c.get('/api/v1/notifications', { headers: ann.session.headers })).json()).items as { title: string }[];
    expect(notes.map((n) => n.title)).toEqual(expect.arrayContaining(['Please sign Offer letter', 'Offer letter is signed']));
    // The signed copy can't be deleted.
    expect((await owner.c.delete(`/api/v1/files/${done.completed_file_id}`, { headers: h })).ok()).toBe(false);
  });

  test('E2E-R4-SIGN-02 declining ends it for everyone; the sender can cancel one; only PDFs; tokens never reach signing', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const ann = await person(request, owner, ws.id, 'Ann Asker');
    const bo = await person(request, owner, ws.id, 'Bo Boss');
    const file = await upload(owner, 'policy.pdf', pdfPages(['Code of conduct']));
    const draft = async (title: string) => {
      const e = await (await owner.c.post('/api/v1/envelopes', { headers: h, data: { title, file_id: file.id, order: 'parallel' } })).json();
      const r = await (await owner.c.put(`/api/v1/envelopes/${e.id}/recipients`, { headers: h, data: { recipients: [{ membership_id: ann.id }, { membership_id: bo.id }] } })).json();
      await owner.c.put(`/api/v1/envelopes/${e.id}/fields`, { headers: h, data: { fields: r.recipients.map((x: { id: string }, i: number) => ({ recipient_id: x.id, kind: 'signature', page: 1, x: 0.1, y: 0.2 + i * 0.2, w: 0.3, h: 0.08 })) } });
      expect((await owner.c.post(`/api/v1/envelopes/${e.id}/send`, { headers: h })).status()).toBe(200);
      return r.recipients as { id: string; membership_id: string }[];
    };
    const [ra] = await draft('Code of conduct');
    // Parallel: both may sign now. Ann declines; it ends for Bo too.
    expect((await ann.session.c.post(`/api/v1/signing/${ra!.id}/decline`, { headers: ann.session.headers, data: { reason: '' } })).status()).toBe(422);
    const d = await ann.session.c.post(`/api/v1/signing/${ra!.id}/decline`, { headers: ann.session.headers, data: { reason: 'Section 4 is wrong' } });
    expect((await d.json()).state).toBe('declined');
    const boBox = (await (await bo.session.c.get('/api/v1/me/signatures', { headers: bo.session.headers })).json()).items;
    expect(boBox[0]).toMatchObject({ envelope_state: 'declined', to_sign: false });
    const ownerNotes = (await (await owner.c.get('/api/v1/notifications', { headers: h })).json()).items as { title: string }[];
    expect(ownerNotes.map((n) => n.title)).toContain('Ann Asker declined to sign Code of conduct');

    const [, rb2] = await draft('Second try');
    const envId = (await (await bo.session.c.get(`/api/v1/signing/${rb2!.id}`, { headers: bo.session.headers })).json()).envelope.id;
    expect((await bo.session.c.post(`/api/v1/envelopes/${envId}/void`, { headers: bo.session.headers, data: { reason: 'no' } })).status()).toBe(404);
    const v = await owner.c.post(`/api/v1/envelopes/${envId}/void`, { headers: h, data: { reason: 'Replaced by v2' } });
    expect((await v.json()).state).toBe('voided');
    expect((await bo.session.c.post(`/api/v1/signing/${rb2!.id}/sign`, { headers: bo.session.headers, data: { document_hash: '00', consent: true, adopted: { kind: 'typed', text: 'Bo' } } })).status()).toBe(409);

    // Only PDFs.
    const txt = await upload(owner, 'notes.txt', Buffer.from('plain words'));
    expect((await owner.c.post('/api/v1/envelopes', { headers: h, data: { title: 'Notes', file_id: txt.id } })).status()).toBe(422);
    // A member without document:send_for_signature can't send.
    expect((await ann.session.c.post('/api/v1/envelopes', { headers: ann.session.headers, data: { title: 'X', file_id: file.id } })).status()).toBe(403);
    // API tokens never reach the ceremony.
    const tok = (await (await ann.session.c.post('/api/v1/me/api-tokens', { headers: ann.session.headers, data: { name: 'x', permissions: ['request:view_own'] } })).json()).token;
    expect((await (await client()).get(`/api/v1/signing/${ra!.id}`, { headers: { authorization: `Bearer ${tok}` } })).status()).toBe(403);
  });
});
