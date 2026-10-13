import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-UI-06 — Admin › Webhooks (BPMS-143): the owner adds an endpoint for every event, copies the
// secret from the show-once dialog, sends a test event (a receiver on this machine gets it, signed),
// sees it as Delivered in the delivery log with its body, and turns the endpoint off; axe.

test.describe('webhooks @r4 @ui @webhooks', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-UI-06 an owner adds a webhook endpoint, sends a test event and sees it delivered', async ({ page, request }) => {
    const hits: { headers: Record<string, unknown>; body: string }[] = [];
    const server = createServer((req, res) => { let b = ''; req.on('data', (c) => (b += c)); req.on('end', () => { hits.push({ headers: req.headers, body: b }); res.writeHead(200); res.end(); }); });
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
    const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}/hooks`;
    try {
      const { owner, ownerEmail } = await workspaceWithOwner(request);
      await signIn(page, ownerEmail, owner.recovery![0]!);
      await nav(page, 'Admin');
      await page.getByRole('link', { name: 'Webhooks' }).first().click();
      await expect(page.getByRole('heading', { level: 1, name: 'Webhooks' })).toBeVisible();
      await expect(page.getByText('No webhook endpoints')).toBeVisible();
      await axe(page);

      await page.getByRole('button', { name: 'Add endpoint' }).click();
      const dialog = page.getByRole('dialog', { name: 'Add a webhook endpoint' });
      await dialog.getByRole('textbox', { name: /^Endpoint URL/ }).fill('http://169.254.169.254/latest');
      await dialog.getByRole('button', { name: 'Add endpoint' }).click();
      await expect(dialog.getByRole('alert')).toContainText('private or reserved');
      await dialog.getByRole('textbox', { name: /^Endpoint URL/ }).fill(url);
      await dialog.getByRole('textbox', { name: /^Description/ }).fill('HR sync');
      await dialog.getByRole('button', { name: 'Add endpoint' }).click();
      const once = page.getByRole('dialog', { name: 'Your signing secret' });
      await expect(once.getByLabel('Signing secret')).toHaveText(/^whsec_/);
      await once.getByRole('button', { name: 'I’ve saved it' }).click();

      const list = page.getByRole('list', { name: 'Webhook endpoints' });
      await expect(list.getByText(url)).toBeVisible();
      await expect(list.getByText(/HR sync · Every event/)).toBeVisible();
      await list.getByRole('button', { name: 'Send test event' }).click();
      await expect(page.getByRole('status').filter({ hasText: 'A test event is on its way' })).toBeVisible();
      await expect.poll(async () => { await request.post('/__test/drain'); return hits.length; }).toBe(1);
      expect(JSON.parse(hits[0]!.body).type).toBe('webhook.test');
      expect(String(hits[0]!.headers['webhook-signature'])).toMatch(/^v1,/);
      const log = page.getByRole('table', { name: 'Deliveries' });
      await page.getByRole('button', { name: 'Refresh' }).click();
      await expect(log.getByText('Delivered')).toBeVisible();
      await log.getByRole('button', { name: 'webhook.test' }).click();
      await expect(log.getByLabel('Event body')).toContainText('"type": "webhook.test"');
      await axe(page);
      await list.getByRole('button', { name: 'Turn off' }).click();
      await expect(list.getByText('Off', { exact: true })).toBeVisible();
    } finally { await new Promise<void>((r) => server.close(() => r())); }
  });
});
