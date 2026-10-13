import { type APIRequestContext, type Page } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { expect, test } from '../../fixtures/test-controls';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R3-UI-04 — the form builder (BPMS-101): an owner builds a form whose "Reason" is asked only
// for a laptop, caps its length, keeps it confidential and limits the budget; the preview shows
// the rule live; once published, the server holds the same rules (a budget under the minimum is
// refused, a hidden answer is dropped, a shown one is required).

const key = () => `e2e-${Date.now()}-${Math.random().toString(36).slice(2)}`;

async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  return { email, session: (await enter(email, ws)) as Session };
}

async function addField(page: Page, label: string, type: string, options?: string) {
  const fields = page.getByRole('region', { name: 'Form fields' });
  await fields.getByRole('button', { name: 'Add field' }).click();
  const dialog = page.getByRole('dialog', { name: 'Add a field' });
  await dialog.getByRole('textbox', { name: /^Label/ }).fill(label);
  await dialog.getByRole('button', { name: /Type/ }).click();
  await page.getByRole('option', { name: type, exact: true }).click();
  if (options) await dialog.getByRole('textbox', { name: 'Options' }).fill(options);
  await dialog.getByRole('button', { name: 'Add field' }).click();
  await expect(dialog).toHaveCount(0);
}

async function editField(page: Page, label: string) {
  const fields = page.getByRole('list', { name: 'Form fields' });
  const row = fields.getByRole('listitem').filter({ hasText: label });
  const toggle = row.getByRole('button', { name: new RegExp(`^${label}`) });
  if ((await toggle.getAttribute('aria-expanded')) !== 'true') await toggle.click();
  return row.getByRole('group', { name: `Edit ${label}` });
}

test.describe('form builder @r3 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R3-UI-04 an owner builds a form with a conditional, capped, confidential question and a budget range; the server holds the same rules', async ({ page, request }) => {
    const { ws, owner, ownerEmail } = await workspaceWithOwner(request);
    const eve = await person(request, owner, ws.id, 'Eve Employee');
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Processes');
    await page.getByRole('button', { name: 'New process' }).first().click();
    const create = page.getByRole('dialog', { name: 'New process' });
    await create.getByRole('textbox', { name: /^Name/ }).fill('Device request');
    await create.getByRole('radiogroup', { name: /Start from/ }).getByText(/^Blank/).first().click();
    await create.getByRole('button', { name: 'Create and open' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Device request' })).toBeVisible();

    await addField(page, 'Device', 'Choice', 'Laptop\nPhone');
    await addField(page, 'Reason', 'Long text');
    await addField(page, 'Budget', 'Money');

    let edit = await editField(page, 'Reason');
    await edit.getByText('Required', { exact: true }).click();
    await edit.getByRole('textbox', { name: /Longest answer/ }).fill('20');
    await edit.getByRole('button', { name: /Show only when/ }).click();
    await page.getByRole('option', { name: '“Device” is…' }).click();
    await expect(edit.getByRole('button', { name: /Value of Device/ })).toContainText('Laptop');
    await edit.getByRole('button', { name: /Who may see the answer/ }).click();
    await page.getByRole('option', { name: 'Confidential' }).click();

    edit = await editField(page, 'Budget');
    await edit.getByRole('textbox', { name: /^Smallest/ }).fill('100');
    await edit.getByRole('textbox', { name: /^Largest/ }).fill('2000');
    await expect(page.getByText('All changes saved')).toBeVisible();

    // Preview: Reason appears only once Laptop is chosen.
    await page.getByRole('region', { name: 'Form fields' }).getByRole('button', { name: 'Preview' }).click();
    const preview = page.getByRole('dialog', { name: 'Preview the form' });
    await expect(preview.getByRole('textbox', { name: /^Reason/ })).toHaveCount(0);
    await preview.getByRole('radiogroup', { name: /Device/ }).getByText('Laptop', { exact: true }).click();
    await expect(preview.getByRole('textbox', { name: /^Reason/ })).toBeVisible();
    await preview.getByRole('button', { name: 'Phone' }).click();
    await axe(page);
    await preview.getByRole('button', { name: 'Close', exact: true }).last().click();

    await page.getByRole('button', { name: 'Publish' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Published v1. People can now request it under New request.' })).toBeVisible();

    // The server keeps the same rules.
    const types = (await (await eve.session.c.get('/api/v1/request-types', { headers: eve.session.headers })).json()).items as { key: string; form: { fields: { name: string; rules?: object; classification?: string }[] } }[];
    const t = types.find((x) => x.key === 'device-request')!;
    expect(t.form.fields.find((f) => f.name === 'reason')).toMatchObject({ classification: 'confidential', rules: { max_length: 20, visible_when: { field: 'device', equals: 'Laptop' } } });
    const start = (input: object) => eve.session.c.post('/api/v1/requests', { headers: { ...eve.session.headers, 'idempotency-key': key() }, data: { type: 'device-request', input } });
    const low = await start({ device: 'Phone', budget: { amount: '50', currency: 'EUR' } });
    expect(low.status()).toBe(422);
    expect(JSON.stringify(await low.json())).toContain('minimum');
    const missing = await start({ device: 'Laptop', budget: { amount: '500', currency: 'EUR' } });
    expect(missing.status()).toBe(422);
    expect(JSON.stringify(await missing.json())).toContain('/input/reason');
    const phone = await start({ device: 'Phone', reason: 'ignored because hidden', budget: { amount: '500', currency: 'EUR' } });
    expect(phone.status(), await phone.text()).toBe(201);
    const detail = await (await eve.session.c.get(`/api/v1/requests/${(await phone.json()).id}`, { headers: eve.session.headers })).json();
    expect(detail.input.reason).toBeUndefined();
  });
});
