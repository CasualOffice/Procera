import { expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-TOK-01..02 (and E2E-R4-11) — personal API tokens (BPMS-144, docs/06 §3.5): a token acts as its member,
// narrowed to the permissions chosen for it; it never decides, touches the account or manages
// tokens; it stops working once revoked; nobody can give a token permissions they don't hold.

const api = async (token: string) => ({ c: await client(), h: { authorization: `Bearer ${token}` } });

test.describe('api tokens @r4 @tokens', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-TOK-01 a token reads within its permissions, is refused outside them and for decisions, and stops when revoked', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const made = await owner.c.post('/api/v1/me/api-tokens', { headers: h, data: { name: 'Nightly report', permissions: ['request:view_own', 'request:start'], expires_in_days: 30 } });
    expect(made.status(), await made.text()).toBe(201);
    const { id, token, last4 } = await made.json();
    expect(token).toMatch(/^prc_live_[A-Za-z0-9]{12}_[A-Za-z0-9]{43}$/);
    expect(token.endsWith(last4)).toBe(true);

    const { c, h: th } = await api(token);
    expect((await c.get('/api/v1/requests?scope=mine', { headers: th })).status()).toBe(200);
    expect((await c.get('/api/v1/me', { headers: th })).status()).toBe(200);
    // Outside its permissions (the owner could, the token can't).
    expect((await owner.c.get('/api/v1/members', { headers: h })).status()).toBe(200);
    expect((await c.get('/api/v1/members', { headers: th })).status()).toBe(403);
    // E2E-R4-11: a token for requests cannot read the audit trail (the owner can; audit:view_own is not in it either).
    expect((await owner.c.get('/api/v1/audit/events', { headers: h })).status()).toBe(200);
    expect((await c.get('/api/v1/audit/events', { headers: th })).status()).toBe(403);
    // Never: decisions, the account, tokens.
    const nope = await c.post('/api/v1/work/00000000-0000-0000-0000-000000000000/decisions', { headers: th, data: { outcome: 'approved' } });
    expect(nope.status()).toBe(403);
    expect(await nope.text()).toContain('token_not_allowed');
    expect((await c.get('/api/v1/me/api-tokens', { headers: th })).status()).toBe(403);
    expect((await c.post('/api/v1/me/password', { headers: th, data: { current_password: PW, new_password: 'x'.repeat(16) } })).status()).toBe(403);
    // A wrong secret is refused.
    expect((await c.get('/api/v1/requests', { headers: { authorization: `Bearer ${token.slice(0, -4)}AAAA` } })).status()).toBe(401);

    // The owner sees it (last used now set) and revokes it; it stops at once.
    const listed = (await (await owner.c.get('/api/v1/me/api-tokens', { headers: h })).json()).items;
    expect(listed[0]).toMatchObject({ id, name: 'Nightly report', state: 'active', last4 });
    expect(listed[0].last_used_at).toBeTruthy();
    expect(JSON.stringify(listed)).not.toContain(token);
    expect((await owner.c.delete(`/api/v1/me/api-tokens/${id}`, { headers: h })).status()).toBe(204);
    expect((await c.get('/api/v1/requests', { headers: th })).status()).toBe(401);
    const all = (await (await owner.c.get('/api/v1/api-tokens', { headers: h })).json()).items;
    expect(all.find((t: { id: string }) => t.id === id).state).toBe('revoked');
  });

  test('E2E-R4-TOK-02 a member cannot give a token permissions they lack, nor list or revoke others’ tokens', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const email = uniqueEmail('tok-member');
    expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
    const inv = await inviteToken(request, ws.id, email);
    expect((await (await client()).post('/api/v1/invitations/register', { data: { token: inv, display_name: 'Tess Token', password: PW } })).status()).toBe(201);
    const mem = await enter(email, ws.id);
    const tooMuch = await mem.c.post('/api/v1/me/api-tokens', { headers: mem.headers, data: { name: 'Admin', permissions: ['member:view', 'api_token:manage'] } });
    expect(tooMuch.status()).toBe(403);
    const unknown = await mem.c.post('/api/v1/me/api-tokens', { headers: mem.headers, data: { name: 'Typo', permissions: ['request:fly'] } });
    expect(unknown.status()).toBe(422);
    const ownerTok = await (await owner.c.post('/api/v1/me/api-tokens', { headers: owner.headers, data: { name: 'Owner one', permissions: ['request:view_own'] } })).json();
    expect((await mem.c.get('/api/v1/api-tokens', { headers: mem.headers })).status()).toBe(403);
    expect((await mem.c.delete(`/api/v1/me/api-tokens/${ownerTok.id}`, { headers: mem.headers })).status()).toBe(404);
    expect((await mem.c.delete(`/api/v1/api-tokens/${ownerTok.id}`, { headers: mem.headers })).status()).toBe(403);
  });
});
