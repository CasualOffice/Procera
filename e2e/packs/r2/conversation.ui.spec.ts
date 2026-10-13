import { type APIRequestContext } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { expect, test } from '../../fixtures/test-controls';
import { WEB, axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R2-UI-17 — the conversation on a request (BPMS-214): the approver comments, writes an
// internal note mentioning HR (someone who can't see the request can't be mentioned), and watches
// it. The requester sees the comment but never the note; the mentioned person is told.

const key = () => `e2e-${Date.now()}-${Math.random().toString(36).slice(2)}`;
const leave = { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23' };

async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const s = await enter(email, ws);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, recovery: s.recovery, session: s as Session, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

const kinds = async (p: { session: Session }) =>
  ((await (await p.session.c.get('/api/v1/notifications', { headers: p.session.headers })).json()).items as { kind: string }[]).map((n) => n.kind);

test.describe('request conversation @r2 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-UI-17 the approver comments, adds an internal note mentioning HR, and watches; the requester never sees the note', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const bo = await person(request, owner, ws.id, 'Bo Boss');
    const ann = await person(request, owner, ws.id, 'Ann Asker');
    const hal = await person(request, owner, ws.id, 'Hal Hr');
    await person(request, owner, ws.id, 'Zed Elsewhere');
    expect((await owner.c.patch(`/api/v1/members/${ann.id}`, { headers: owner.headers, data: { manager_membership_id: bo.id } })).ok()).toBe(true);
    expect((await owner.c.post('/api/v1/role-grants', { headers: owner.headers, data: { membership_id: hal.id, role_key: 'workspace.admin' } })).status()).toBe(201);
    const r = await ann.session.c.post('/api/v1/requests', { headers: { ...ann.session.headers, 'idempotency-key': key() }, data: { type: 'hr.leave-request', input: leave } });
    expect(r.status(), await r.text()).toBe(201);
    const { id } = await r.json();

    await signIn(page, bo.email, bo.recovery[0]!);
    await nav(page, 'My work');
    await page.goto(`${WEB}/requests/${id}`);
    const conv = page.getByRole('region', { name: /^Conversation/ });
    await expect(conv).toContainText('No comments yet');
    const form = conv.getByRole('form', { name: 'Add a comment' });
    await form.getByRole('textbox', { name: 'Comment' }).fill('Is the 23rd confirmed with the team?');
    await form.getByRole('button', { name: 'Comment', exact: true }).click();
    const comments = conv.getByRole('list', { name: 'Comments' });
    await expect(comments.getByRole('listitem')).toHaveCount(1);

    // An internal note mentioning HR; Zed can't see the request, so mentioning Zed is refused.
    await form.getByRole('checkbox', { name: /Internal note, hidden from Ann Asker/ }).check();
    const pick = async (name: string) => { await form.getByRole('button', { name: /Mention/ }).click(); await page.getByRole('option', { name }).click(); };
    await pick('Hal Hr');
    await pick('Zed Elsewhere');
    await form.getByRole('textbox', { name: 'Internal note' }).pressSequentially('team is thin that week');
    await form.getByRole('button', { name: 'Add note' }).click();
    await expect(form.getByRole('alert')).toContainText('Zed Elsewhere can’t see this request');
    await form.getByRole('button', { name: 'Remove mention of Zed Elsewhere' }).click();
    await form.getByRole('button', { name: 'Add note' }).click();
    await expect(comments.getByRole('listitem')).toHaveCount(2);
    await expect(comments.getByRole('listitem').nth(1)).toContainText('Internal');
    await expect(comments.getByRole('listitem').nth(1)).toContainText('@Hal Hr');

    await conv.getByRole('button', { name: 'Watch' }).click();
    await expect(conv.getByRole('button', { name: 'Stop watching' })).toHaveAttribute('aria-pressed', 'true');
    await expect(conv).toContainText('1 watching');
    await axe(page);

    // The requester sees the comment, never the note; HR was mentioned.
    const forAnn = await (await ann.session.c.get(`/api/v1/requests/${id}/comments`, { headers: ann.session.headers })).json();
    expect(forAnn.comments.map((c: { body: string }) => c.body)).toEqual(['Is the 23rd confirmed with the team?']);
    expect(forAnn.can_post_internal).toBe(false);
    expect(await kinds(ann)).toContain('request_comment');
    expect(await kinds(hal)).toContain('request_mention');
  });
});
