import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { freshAuth, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-CFG-06..08 — department overrides, profile switch plan/apply and the effective-settings
// cache (BPMS-027 remainder; docs/20 §0, §1, §5; CFG-004, CFG-017).

type Item = { key: string; value: unknown; source: string; inherited_from: string | null; override_revision: number; layers: { layer: string; scope_id?: string }[] };

test.describe('settings layers @r1 @settings', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-CFG-06 department overrides: set on a parent, inherited by a child, explained, cleared; catalogue and workspace isolation enforced', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const fin = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'Finance' } })).json();
    const pay = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'Payroll', parent_id: fin.id } })).json();
    const tz = (items: Item[]) => items.find((i) => i.key === 'locale.timezone')!;
    const view = async (id: string) => (await (await owner.c.get(`/api/v1/settings/departments/${id}`, { headers: h })).json()).items as Item[];

    const before = await view(pay.id);
    expect(before.every((i) => i.key !== 'auth.session.idle_timeout'), 'only department-scopable keys are listed').toBe(true);
    expect([tz(before).value, tz(before).source]).toEqual(['UTC', 'default']);

    const set = await owner.c.put(`/api/v1/settings/departments/${fin.id}/locale.timezone`, { headers: { ...h, 'if-match': '"0"' }, data: { value: 'Europe/London', reason: 'UK office' } });
    expect(set.status()).toBe(200);
    expect(set.headers()['etag']).toBe('"1"');
    const child = tz(await view(pay.id));
    expect([child.value, child.source, child.inherited_from, child.override_revision]).toEqual(['Europe/London', 'department', fin.id, 0]);

    const why = await (await owner.c.get(`/api/v1/settings/effective?key=locale.timezone&scope=department:${pay.id}`, { headers: h })).json();
    expect(why.layers.map((l: { layer: string }) => l.layer)).toEqual(['default', 'department']);
    expect(why.layers[1].scope_id).toBe(fin.id);
    // The workspace level is unchanged.
    expect((await (await owner.c.get('/api/v1/settings/effective?key=locale.timezone', { headers: h })).json()).value).toBe('UTC');

    const put = (dept: string, key: string, data: object, extra: Record<string, string> = {}) =>
      owner.c.put(`/api/v1/settings/departments/${dept}/${key}`, { headers: { ...h, ...extra }, data });
    const notScopable = await put(fin.id, 'auth.session.idle_timeout', { value: 900, reason: 'x' });
    expect([notScopable.status(), (await notScopable.json()).code]).toEqual([422, 'not_department_scopable']);
    expect((await put(fin.id, 'locale.timezone', { value: 'UTC' }, { 'if-match': '"0"' })).status()).toBe(412);
    expect((await put(crypto.randomUUID(), 'locale.timezone', { value: 'UTC' })).status()).toBe(404);
    expect((await put(fin.id, 'locale.week_start', { value: 'someday' })).status()).toBe(422);

    // Another workspace cannot see or change this department (isolation sweep).
    const other = await workspaceWithOwner(request);
    expect((await other.owner.c.get(`/api/v1/settings/departments/${fin.id}`, { headers: other.owner.headers })).status()).toBe(404);
    expect((await other.owner.c.put(`/api/v1/settings/departments/${fin.id}/locale.timezone`, { headers: other.owner.headers, data: { value: 'UTC' } })).status()).toBe(404);

    const clear = await owner.c.delete(`/api/v1/settings/departments/${fin.id}/locale.timezone`, { headers: { ...h, 'if-match': '"1"' } });
    expect(clear.status()).toBe(200);
    expect([tz(await view(pay.id)).value, tz(await view(pay.id)).source]).toEqual(['UTC', 'default']);
    expect((await owner.c.delete(`/api/v1/settings/departments/${fin.id}/locale.timezone`, { headers: h })).status()).toBe(404);
    const history = await (await owner.c.get('/api/v1/settings/locale.timezone/history', { headers: h })).json();
    expect(history.items.slice(0, 2).map((r: { layer: string; scope_id: string; after: unknown }) => [r.layer, r.scope_id, r.after])).toEqual([
      ['department', fin.id, null],
      ['department', fin.id, 'Europe/London'],
    ]);
  });

  test('E2E-R1-CFG-07 profile switch: the plan shows current, new and floor values; apply needs If-Match, a reason and step-up; then nothing is left to change', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request, 'standard');
    const plan = await owner.c.get('/api/v1/settings/profile/plan?profile=regulated', { headers: owner.headers });
    expect(plan.status()).toBe(200);
    const p = await plan.json();
    expect([p.from, p.to]).toEqual(['standard', 'regulated']);
    const idle = p.items.find((i: { key: string }) => i.key === 'auth.session.idle_timeout');
    expect(idle).toMatchObject({ current: 1800, proposed: 900, after: 900, clamped: false, floor: 28800 });
    expect(p.items.find((i: { key: string }) => i.key === 'governance.review.min_reviewers')).toMatchObject({ current: 1, after: 2 });
    const etag = plan.headers()['etag'];
    expect(etag).toBe(`"${p.basis}"`);
    expect((await owner.c.get('/api/v1/settings/profile/plan?profile=nope', { headers: owner.headers })).status()).toBe(422);

    const apply = (headers: Record<string, string>, data: object = { profile: 'regulated', reason: 'Audit finding' }) =>
      owner.c.post('/api/v1/settings/profile/apply', { headers, data });
    const fresh = await freshAuth(owner);
    expect((await apply(fresh)).status()).toBe(428);
    expect((await apply({ ...fresh, 'if-match': '"0000"' })).status()).toBe(412);
    expect((await (await apply({ ...fresh, 'if-match': etag! }, { profile: 'regulated', reason: ' ' })).json()).code).toBe('reason_required');
    const old = await (await owner.c.post('/__test/session/backdate', { headers: fresh, data: { seconds: 600 } })).json();
    const stale = await apply({ authorization: `Bearer ${old.access_token}`, 'if-match': etag! });
    expect([stale.status(), (await stale.json()).code]).toEqual([401, 'step_up_required']);

    const ok = await apply({ ...fresh, 'if-match': etag! });
    expect(ok.status()).toBe(200);
    expect((await ok.json()).items.length).toBe(p.items.length);
    const e = await (await owner.c.get('/api/v1/settings/effective?key=auth.session.idle_timeout', { headers: owner.headers })).json();
    expect([e.value, e.source]).toEqual([900, 'profile']);
    const again = await (await owner.c.get('/api/v1/settings/profile/plan?profile=regulated', { headers: owner.headers })).json();
    expect([again.from, again.items.length]).toEqual(['regulated', 0]);
    const h = await (await owner.c.get('/api/v1/settings/auth.session.idle_timeout/history', { headers: owner.headers })).json();
    expect(h.items[0]).toMatchObject({ layer: 'profile', after: 900, reason: 'profile standard → regulated: Audit finding' });
  });

  test('E2E-R1-CFG-08 a settings change is visible on the very next read, through the cache when one is configured', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const list = async () => {
      const r = await owner.c.get('/api/v1/settings', { headers: h });
      expect(r.status()).toBe(200);
      const week = (await r.json()).items.find((i: { key: string }) => i.key === 'locale.week_start');
      return { value: week.value, cache: r.headers()['x-settings-cache'] };
    };
    const first = await list();
    expect(['off', 'miss', 'hit']).toContain(first.cache);
    const second = await list();
    if (first.cache !== 'off') expect(second.cache).toBe('hit');
    expect(second.value).toBe('mon');

    expect((await owner.c.put('/api/v1/settings/locale.week_start', { headers: h, data: { value: 'sun' } })).status()).toBe(200);
    const after = await list();
    expect(after.value).toBe('sun');
    if (first.cache !== 'off') expect(after.cache).toBe('miss');

    // A department override is visible at once too.
    const dept = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'Ops' } })).json();
    const tz = async () => (await (await owner.c.get(`/api/v1/settings/departments/${dept.id}`, { headers: h })).json()).items.find((i: Item) => i.key === 'locale.timezone').value;
    expect(await tz()).toBe('UTC');
    await owner.c.put(`/api/v1/settings/departments/${dept.id}/locale.timezone`, { headers: h, data: { value: 'Asia/Kolkata' } });
    expect(await tz()).toBe('Asia/Kolkata');

    // Writes that bypass the service (test set-up) invalidate as well.
    expect((await request.post('/__test/settings', { data: { workspace: ws.id, key: 'locale.week_start', value: 'sat' } })).status()).toBe(204);
    expect((await list()).value).toBe('sat');
  });
});
