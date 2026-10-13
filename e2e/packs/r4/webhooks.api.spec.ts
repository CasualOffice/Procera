import { createHmac } from 'node:crypto';
import { type IncomingHttpHeaders, type Server, createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-WH-01..03 — outgoing webhooks (BPMS-143, WH-01..04, SSRF-01..07): a receiver on this
// machine (allowed for e2e servers with PROCERA_EGRESS_ALLOW=127.0.0.1/32) gets a test event and a
// real `member.joined` event, signed per Standard Webhooks, thin (ids only); private and metadata
// addresses are refused; a 500 is retried later, a 410 turns the endpoint off; rolling the secret
// signs with both; a delivery can be replayed with the same webhook-id.

type Hit = { headers: IncomingHttpHeaders; body: string };

async function receiver(status: () => number) {
  const hits: Hit[] = [];
  const server: Server = createServer((req, res) => {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => { hits.push({ headers: req.headers, body }); res.writeHead(status()); res.end('ok'); });
  });
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
  const port = (server.address() as AddressInfo).port;
  return { url: `http://127.0.0.1:${port}/procera`, hits, close: () => new Promise<void>((r) => server.close(() => r())) };
}

/** Verifies a Standard Webhooks signature header against a `whsec_` secret; returns how many signatures match. */
function verified(h: Hit, secret: string): number {
  const key = Buffer.from(secret.replace(/^whsec_/, ''), 'base64');
  const want = createHmac('sha256', key).update(`${h.headers['webhook-id']}.${h.headers['webhook-timestamp']}.${h.body}`).digest('base64');
  return String(h.headers['webhook-signature']).split(' ').filter((s) => s === `v1,${want}`).length;
}

test.describe('webhooks @r4 @webhooks', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-WH-01 a test event and a real event arrive signed and thin; private and metadata addresses are refused', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    for (const bad of ['http://169.254.169.254/latest/meta-data', 'https://10.0.0.5/hook', 'https://localhost/x', 'ftp://example.com/x']) {
      const r = await owner.c.post('/api/v1/webhooks', { headers: h, data: { url: bad, events: ['*'] } });
      expect(r.status(), bad).toBe(422);
    }
    expect((await owner.c.post('/api/v1/webhooks', { headers: h, data: { url: 'https://hooks.example.com/x', events: ['nope'] } })).status()).toBe(422);
    const rx = await receiver(() => 200);
    try {
      const made = await owner.c.post('/api/v1/webhooks', { headers: h, data: { url: rx.url, events: ['member.joined'], description: 'HR sync' } });
      expect(made.status(), await made.text()).toBe(201);
      const ep = await made.json();
      expect(ep.secret).toMatch(/^whsec_[A-Za-z0-9+/=]{32}$/);
      expect(JSON.stringify((await (await owner.c.get('/api/v1/webhooks', { headers: h })).json()).items)).not.toContain(ep.secret);
      const catalogue = (await (await owner.c.get('/api/v1/webhook-events', { headers: h })).json()).items as { type: string }[];
      expect(catalogue.map((e) => e.type)).toEqual(expect.arrayContaining(['request.completed', 'signature.completed', 'member.joined']));

      // A test event, on demand.
      expect((await owner.c.post(`/api/v1/webhooks/${ep.id}/test`, { headers: h })).status()).toBe(202);
      await expect.poll(async () => { await request.post('/__test/drain'); return rx.hits.length; }).toBe(1);
      const t = rx.hits[0]!;
      expect(JSON.parse(t.body)).toMatchObject({ type: 'webhook.test', workspace_id: ws.id, data: { endpoint_id: ep.id } });
      expect(t.headers['webhook-id']).toBe(JSON.parse(t.body).id);
      expect(Math.abs(Number(t.headers['webhook-timestamp']) - Date.now() / 1000)).toBeLessThan(3600 * 24 * 400); // the server's (test) clock
      expect(verified(t, ep.secret)).toBe(1);

      // A real event: someone joins. Only ids reach the receiver.
      const email = uniqueEmail('wh-joiner');
      await owner.c.post('/api/v1/members/invitations', { headers: h, data: { email, roles: ['member'] } });
      const token = await inviteToken(request, ws.id, email);
      expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: 'Jo Joiner', password: PW } })).status()).toBe(201);
      await expect.poll(async () => { await request.post('/__test/drain'); return rx.hits.length; }).toBe(2);
      const joined = JSON.parse(rx.hits[1]!.body);
      expect(joined).toMatchObject({ type: 'member.joined', data: { membership_id: expect.any(String) } });
      expect(JSON.stringify(joined)).not.toContain('Jo Joiner');
      expect(JSON.stringify(joined)).not.toContain(email);
      expect(verified(rx.hits[1]!, ep.secret)).toBe(1);
      const log = (await (await owner.c.get(`/api/v1/webhooks/${ep.id}/deliveries`, { headers: h })).json()).items;
      expect(log.map((d: { state: string }) => d.state)).toEqual(['delivered', 'delivered']);
      expect(log[0].last_status).toBe(200);
    } finally { await rx.close(); }
  });

  test('E2E-R4-WH-02 a failing receiver is retried later; 410 turns the endpoint off; replay resends the same event', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    let code = 500;
    const rx = await receiver(() => code);
    try {
      const ep = await (await owner.c.post('/api/v1/webhooks', { headers: h, data: { url: rx.url, events: ['*'] } })).json();
      await owner.c.post(`/api/v1/webhooks/${ep.id}/test`, { headers: h });
      await expect.poll(async () => { await request.post('/__test/drain'); return rx.hits.length; }).toBe(1);
      let log = (await (await owner.c.get(`/api/v1/webhooks/${ep.id}/deliveries`, { headers: h })).json()).items;
      expect(log[0]).toMatchObject({ state: 'pending', attempts: 1, last_status: 500 });
      expect(new Date(log[0].next_attempt_at).getTime()).toBeGreaterThan(Date.now() - 60_000); // retried later, not now
      await request.post('/__test/drain');
      expect(rx.hits.length).toBe(1);

      // Replay sends it again now, with the same webhook-id; this time the receiver says 410 Gone.
      code = 410;
      expect((await owner.c.post(`/api/v1/webhooks/deliveries/${log[0].id}/replay`, { headers: h })).status()).toBe(200);
      await expect.poll(async () => { await request.post('/__test/drain'); return rx.hits.length; }).toBe(2);
      expect(rx.hits[1]!.headers['webhook-id']).toBe(rx.hits[0]!.headers['webhook-id']);
      const off = await (await owner.c.get(`/api/v1/webhooks/${ep.id}`, { headers: h })).json();
      expect(off).toMatchObject({ enabled: false, disabled_reason: 'The receiver answered 410 Gone.' });
      const notes = (await (await owner.c.get('/api/v1/notifications', { headers: h })).json()).items as { title: string }[];
      expect(notes.some((n) => n.title.includes('turned off'))).toBe(true);
      log = (await (await owner.c.get(`/api/v1/webhooks/${ep.id}/deliveries`, { headers: h })).json()).items;
      expect(log[0]).toMatchObject({ state: 'failed', last_status: 410 });
      // Turning it back on clears the reason.
      const on = await owner.c.patch(`/api/v1/webhooks/${ep.id}`, { headers: { ...h, 'if-match': `"${off.revision}"` }, data: { enabled: true } });
      expect((await on.json())).toMatchObject({ enabled: true, disabled_reason: null });
    } finally { await rx.close(); }
  });

  test('E2E-R4-WH-03 rolling the secret signs with both for a day; only admins manage webhooks', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const rx = await receiver(() => 204);
    try {
      const ep = await (await owner.c.post('/api/v1/webhooks', { headers: h, data: { url: rx.url, events: ['*'] } })).json();
      const rolled = await owner.c.post(`/api/v1/webhooks/${ep.id}/rotate-secret`, { headers: h });
      expect(rolled.status(), await rolled.text()).toBe(200);
      const { secret, previous_valid_until } = await rolled.json();
      expect(secret).not.toBe(ep.secret);
      expect(previous_valid_until).toBeTruthy();
      await owner.c.post(`/api/v1/webhooks/${ep.id}/test`, { headers: h });
      await expect.poll(async () => { await request.post('/__test/drain'); return rx.hits.length; }).toBe(1);
      expect(String(rx.hits[0]!.headers['webhook-signature']).split(' ')).toHaveLength(2);
      expect(verified(rx.hits[0]!, secret)).toBe(1);
      expect(verified(rx.hits[0]!, ep.secret)).toBe(1);
      // A member can't see or change webhooks.
      const email = uniqueEmail('wh-member');
      await owner.c.post('/api/v1/members/invitations', { headers: h, data: { email, roles: ['member'] } });
      const token = await inviteToken(request, ws.id, email);
      await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: 'Mo Member', password: PW } });
      const { enter } = await import('../../fixtures/workspace');
      const mem = await enter(email, ws.id);
      expect((await mem.c.get('/api/v1/webhooks', { headers: mem.headers })).status()).toBe(403);
      expect((await mem.c.post('/api/v1/webhooks', { headers: mem.headers, data: { url: rx.url, events: ['*'] } })).status()).toBe(403);
      expect((await mem.c.delete(`/api/v1/webhooks/${ep.id}`, { headers: mem.headers })).status()).toBe(403);
    } finally { await rx.close(); }
  });
});
