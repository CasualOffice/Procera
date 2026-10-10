import { type APIRequestContext, expect, test as base } from '@playwright/test';

/**
 * Test-only controls exposed by an `e2e` build of procera (docs/23 §4).
 * Never use fixed sleeps in scenarios — use `drain()` and `advanceClock()` instead (docs/23 §7).
 */
export class TestControls {
  constructor(private readonly request: APIRequestContext) {}

  async now(): Promise<Date> {
    const res = await this.request.get('/__test/clock');
    expect(res.ok(), 'test controls must be available: build procera with --features e2e').toBeTruthy();
    return new Date((await res.json()).now);
  }

  /** Advance the server clock by an ISO-8601 duration such as "P30D" or "PT2H". */
  async advanceClock(advance: string): Promise<Date> {
    const res = await this.request.post('/__test/clock', { data: { advance } });
    expect(res.status(), `advance clock by ${advance}`).toBe(200);
    return new Date((await res.json()).now);
  }

  /** Wait until outbox, jobs and timers are idle. */
  async drain(): Promise<void> {
    const res = await this.request.post('/__test/drain');
    expect(res.ok()).toBeTruthy();
    expect((await res.json()).idle).toBe(true);
  }
}

export const test = base.extend<{ controls: TestControls }>({
  controls: async ({ request }, use) => {
    await use(new TestControls(request));
  },
});

export { expect };
