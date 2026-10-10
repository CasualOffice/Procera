import { expect, test } from '../../fixtures/test-controls';

// E2E-P0-01..04 — harness smoke (docs/23 §5, gate G1 in docs/07 §4).

test.describe('harness smoke @smoke', () => {
  test('E2E-P0-01 server is healthy and ready', async ({ request }) => {
    const health = await request.get('/healthz');
    expect(health.status()).toBe(200);
    expect(await health.json()).toEqual({ status: 'ok' });

    const ready = await request.get('/readyz');
    expect(ready.status()).toBe(200);
    const r = await ready.json();
    expect(['memory', 'valkey']).toContain(r.cache); // advisory cache reported, never blocks readiness
    expect(['ok', 'degraded']).toContain(r.cache_health);
  });

  test('E2E-P0-02 version endpoint reports name and server time', async ({ request }) => {
    const res = await request.get('/version');
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.name).toBe('procera');
    expect(Number.isNaN(Date.parse(body.server_time))).toBe(false);
  });

  test('E2E-P0-03 test clock moves forward only and drives server time', async ({ request, controls }) => {
    const before = await controls.now();
    const after = await controls.advanceClock('P30D');
    expect(after.getTime() - before.getTime()).toBeGreaterThanOrEqual(30 * 24 * 3600 * 1000);

    const version = await (await request.get('/version')).json();
    expect(new Date(version.server_time).getTime()).toBeGreaterThanOrEqual(after.getTime());

    const invalid = await request.post('/__test/clock', { data: { advance: 'P1Y' } });
    expect(invalid.status()).toBe(422);
    expect(invalid.headers()['content-type']).toContain('application/problem+json');
    expect((await invalid.json()).code).toBe('validation_failed');
  });

  test('E2E-P0-04 drain reports idle and unknown routes return 404', async ({ request, controls }) => {
    await controls.drain();
    const missing = await request.get('/api/v1/does-not-exist');
    expect(missing.status()).toBe(404);
  });
});
