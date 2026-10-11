import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser, serverNow } from '../../fixtures/accounts';
import { totp } from '../../fixtures/totp';
import { enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-CFG-01..05 — layered settings, floors and the explainer (BPMS-027; docs/20 §0-§2).

test.describe('settings @r1 @settings', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-CFG-01 the creation profile seeds values; the explainer shows every layer', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request, 'starter');
    const idle = await owner.c.get('/api/v1/settings/effective?key=auth.session.idle_timeout', { headers: owner.headers });
    expect(idle.status()).toBe(200);
    const e = await idle.json();
    expect([e.value, e.source, e.clamped]).toEqual([28800, 'profile', false]);
    expect(e.layers.map((l: { layer: string }) => l.layer)).toEqual(['default', 'platform_floor', 'profile']);
    const all = await (await owner.c.get('/api/v1/settings', { headers: owner.headers })).json();
    const week = all.items.find((i: { key: string }) => i.key === 'locale.week_start');
    expect([week.value, week.source]).toEqual(['mon', 'default']);
    expect((await owner.c.get('/api/v1/settings/no.such.key', { headers: owner.headers })).status()).toBe(404);
  });

  test('E2E-R1-CFG-02 floors: looser rejected (422 setting_below_floor), stricter saved with ETag/If-Match, reasons for ⚡ keys', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const put = (key: string, data: object, extra: Record<string, string> = {}) =>
      owner.c.put(`/api/v1/settings/${key}`, { headers: { ...owner.headers, ...extra }, data });
    const looser = await put('auth.session.idle_timeout', { value: 86400, reason: 'longer sessions' });
    expect(looser.status()).toBe(422);
    expect((await looser.json()).code).toBe('setting_below_floor');
    const noReason = await put('auth.session.idle_timeout', { value: 900 });
    expect((await noReason.json()).code).toBe('reason_required');
    const ok = await put('auth.session.idle_timeout', { value: 900, reason: 'security review' }, { 'if-match': '"1"' });
    expect(ok.status()).toBe(200);
    expect(ok.headers()['etag']).toBe('"2"');
    expect((await ok.json()).source).toBe('workspace');
    expect((await put('auth.session.idle_timeout', { value: 600, reason: 'x' }, { 'if-match': '"1"' })).status()).toBe(412);
    expect((await put('auth.password.breach_check', { value: false, reason: 'x' })).status()).toBe(403);
    expect((await put('branding.accent', { value: 'teal' })).status()).toBe(422);
    const accent = await (await put('branding.accent', { value: '#0f766e' })).json();
    expect(accent.value).toBe('#0F766E');
    const history = await (await owner.c.get('/api/v1/settings/auth.session.idle_timeout/history', { headers: owner.headers })).json();
    expect(history.items[0]).toMatchObject({ layer: 'workspace', after: 900, reason: 'security review' });
  });

  test('E2E-R1-CFG-03 step-up keys need a recent authentication; POST /session/step-up refreshes it', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const key = '/api/v1/settings/auth.mfa.required_roles';
    // Simulate a sign-in 10 minutes ago (beyond the default 5-minute window) without moving the shared clock.
    const old = await (await owner.c.post('/__test/session/backdate', { headers: owner.headers, data: { seconds: 600 } })).json();
    const stale = { authorization: `Bearer ${old.access_token}` };
    const denied = await owner.c.put(key, { headers: stale, data: { value: ['*'], reason: 'everyone' } });
    expect([denied.status(), (await denied.json()).code]).toEqual([401, 'step_up_required']);
    expect((await owner.c.post('/api/v1/session/step-up', { headers: stale, data: { code: '000000' } })).status()).toBe(401);
    // TOTP codes are single-use per 30 s step and sign-in used the next step: advance, then use the step after.
    await request.post('/__test/clock', { data: { advance: 'PT30S' } });
    const now = await serverNow(owner.c);
    const up = await owner.c.post('/api/v1/session/step-up', { headers: stale, data: { code: totp(owner.secret, now, 1) } });
    expect(up.status()).toBe(200);
    const fresh = { authorization: `Bearer ${(await up.json()).access_token}` };
    expect((await owner.c.put(key, { headers: fresh, data: { value: ['*'], reason: 'everyone' } })).status()).toBe(200);
  });

  test('E2E-R1-CFG-04 members without settings permissions can neither view nor change settings', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const email = await seedUser(request, 'plain');
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
    const token = await inviteToken(request, ws.id, email);
    const pw = await passwordSession(email);
    await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });
    const m = await enter(email, ws.id);
    expect((await m.c.get('/api/v1/settings', { headers: m.headers })).status()).toBe(403);
    expect((await m.c.put('/api/v1/settings/locale.week_start', { headers: m.headers, data: { value: 'sun' } })).status()).toBe(403);
  });

  test('E2E-R1-CFG-05 raising a platform floor clamps looser workspace values without rewriting them', async ({ request }) => {
    const { sys, owner } = await workspaceWithOwner(request);
    const key = 'api.tokens.max_ttl';
    expect((await owner.c.put(`/api/v1/settings/${key}`, { headers: owner.headers, data: { value: 30 * 86400, reason: 'shorter tokens' } })).status()).toBe(200);
    const floors = await (await sys.c.get('/sysapi/v1/settings', { headers: sys.headers })).json();
    const original = floors.items.find((f: { key: string }) => f.key === key).value;
    try {
      const raise = await sys.c.put(`/sysapi/v1/settings/${key}`, { headers: sys.headers, data: { value: 7 * 86400, reason: 'policy tightening' } });
      expect(raise.status()).toBe(200);
      const e = await (await owner.c.get(`/api/v1/settings/effective?key=${key}`, { headers: owner.headers })).json();
      expect([e.value, e.source, e.clamped]).toEqual([7 * 86400, 'platform_floor', true]);
      expect(e.layers.find((l: { layer: string }) => l.layer === 'workspace').value).toBe(30 * 86400);
      expect((await sys.c.put(`/sysapi/v1/settings/${key}`, { headers: sys.headers, data: { value: 7 * 86400, reason: '' } })).status()).toBe(422);
      expect((await sys.c.put('/sysapi/v1/settings/locale.week_start', { headers: sys.headers, data: { value: 'mon', reason: 'no floor' } })).status()).toBe(422);
    } finally {
      await sys.c.put(`/sysapi/v1/settings/${key}`, { headers: sys.headers, data: { value: original, reason: 'restore after E2E-R1-CFG-05' } });
    }
  });
});
