import { expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-NOT-01..02 — invitation email and the in-app inbox (BPMS-048; FR-N01/N02).

test.describe('notifications @r1 @notify', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-NOT-01 an invitation sends an email whose link registers the invitee; the inviter is told in-app', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const email = uniqueEmail('mailed');
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
    await request.post('/__test/drain');
    const mail = await (await request.get(`/__test/mail?workspace=${ws.id}&to=${encodeURIComponent(email)}`)).json();
    expect(mail.items).toHaveLength(1);
    const m = mail.items[0];
    expect(m.kind).toBe('invitation');
    expect(m.subject).toBe('Olivia invited you to Acme on Procera');
    expect(m.html).toContain('Accept invitation');
    const token = /\/invite\?token=([A-Za-z0-9_-]+)/.exec(m.text)?.[1];
    expect(token).toBeTruthy();
    const anon = await client();
    expect((await anon.post('/api/v1/invitations/register', { data: { token, display_name: 'Zoe', password: PW } })).status()).toBe(201);

    await request.post('/__test/drain');
    const inbox = await (await owner.c.get('/api/v1/notifications', { headers: owner.headers })).json();
    const joined = inbox.items.find((n: { kind: string }) => n.kind === 'member_joined');
    expect(joined).toMatchObject({ title: 'Zoe joined Acme', body: 'Zoe accepted your invitation.', link: '/members' });
    expect(inbox.unread_count).toBeGreaterThanOrEqual(1);
    expect((await owner.c.post(`/api/v1/notifications/${joined.id}/read`, { headers: owner.headers })).status()).toBe(204);
    const unread = await (await owner.c.get('/api/v1/notifications?unread=true', { headers: owner.headers })).json();
    expect(unread.items.find((n: { id: string }) => n.id === joined.id)).toBeUndefined();
    expect((await (await owner.c.post('/api/v1/notifications/read-all', { headers: owner.headers })).json()).marked).toBe(0);
  });

  test('E2E-R1-NOT-02 notifications are private to their member', async ({ request }) => {
    const a = await workspaceWithOwner(request);
    const email = uniqueEmail('mailed2');
    await a.owner.c.post('/api/v1/members/invitations', { headers: a.owner.headers, data: { email, roles: ['member'] } });
    await request.post('/__test/drain');
    const mail = await (await request.get(`/__test/mail?workspace=${a.ws.id}&to=${encodeURIComponent(email)}`)).json();
    const token = /\/invite\?token=([A-Za-z0-9_-]+)/.exec(mail.items[0].text)?.[1];
    await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: 'Yan', password: PW } });
    await request.post('/__test/drain');
    const mine = await (await a.owner.c.get('/api/v1/notifications', { headers: a.owner.headers })).json();
    const b = await workspaceWithOwner(request);
    expect((await b.owner.c.post(`/api/v1/notifications/${mine.items[0].id}/read`, { headers: b.owner.headers })).status()).toBe(404);
  });
});
