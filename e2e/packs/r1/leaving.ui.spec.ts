import { type APIRequestContext, type Page, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { WEB, axe, signIn } from '../../fixtures/ui';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-74 — leaving the organisation (BPMS-023, BPMS-046; docs/22 A-44): Deactivate opens a
// 3-step dialog. Review lists what they hold; Hand over defaults to their manager ("When someone
// leaves" = manager) and can be changed; Confirm deactivates, hands the document and the direct
// report over, and their pending acknowledgement closes as "left", outside the campaign's counts.

async function member(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase());
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id;
}

async function pick(page: Page, scope: ReturnType<Page['getByRole']>, label: RegExp, option: string) {
  await scope.getByRole('button', { name: label }).click();
  await page.getByRole('option', { name: option, exact: true }).click();
}

test.describe('leaving the organisation @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-74 an owner deactivates a leaver in three steps: review, hand over, confirm', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const [nina, theo, sam] = [await member(request, owner, ws.id, 'Nina'), await member(request, owner, ws.id, 'Theo'), await member(request, owner, ws.id, 'Sam')];
    const me = (await (await owner.c.get('/api/v1/me/permissions', { headers: h })).json()).membership_id as string;
    expect((await owner.c.patch(`/api/v1/members/${nina}`, { headers: h, data: { manager_membership_id: theo } })).ok()).toBe(true);
    expect((await owner.c.patch(`/api/v1/members/${sam}`, { headers: h, data: { manager_membership_id: nina } })).ok()).toBe(true);
    const { artifact, version } = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'conduct', title: 'Code of Conduct', content: { blocks: [{ type: 'paragraph', text: 'Be kind.' }] } } })).json();
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h });
    expect((await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: h, data: {} })).ok()).toBe(true);
    expect((await owner.c.put(`/api/v1/artifacts/${artifact.id}/owner`, { headers: h, data: { membership_id: nina } })).ok()).toBe(true);
    const camp = await (await owner.c.post('/api/v1/campaigns', { headers: h, data: { version_id: version.id, title: 'Conduct attestation', audience: { all: true } } })).json();
    expect((await owner.c.post(`/api/v1/campaigns/${camp.id}/activate`, { headers: h })).ok()).toBe(true);
    const before = (await (await owner.c.get(`/api/v1/campaigns/${camp.id}`, { headers: h })).json()).progress;
    expect((await request.post('/__test/settings', { data: { workspace: ws.id, key: 'directory.deactivation.reassign_strategy', value: 'manager' } })).ok()).toBe(true);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await page.goto(`${WEB}/members/${nina}`);
    await page.getByRole('button', { name: 'Deactivate' }).click();
    const dialog = page.getByRole('dialog', { name: 'Deactivate Nina?' });

    // 1. Review.
    const holds = dialog.getByRole('definition');
    await expect(dialog.getByText('Step 1 of 3: what they hold in this workspace.')).toBeVisible();
    await expect(holds.filter({ hasText: 'Code of Conduct' })).toBeVisible();
    await expect(holds.filter({ hasText: 'Sam' })).toBeVisible();
    await expect(holds.filter({ hasText: /^1: they close as “left organisation”/ })).toBeVisible();
    await expect(holds.filter({ hasText: 'Member' })).toBeVisible(); // the role grant
    await axe(page);
    await dialog.getByRole('button', { name: 'Next' }).click();

    // 2. Hand over: both default to Theo (her manager); the report goes to me instead.
    await expect(dialog.getByRole('button', { name: /New owner for their documents/ })).toContainText('Theo');
    await expect(dialog.getByRole('button', { name: /New manager for their direct reports/ })).toContainText('Theo');
    await pick(page, dialog, /New manager for their direct reports/, 'Olivia (you)');
    await axe(page);
    await dialog.getByRole('button', { name: 'Next' }).click();

    // 3. Confirm.
    const what = dialog.getByRole('list', { name: 'What happens' });
    await expect(what).toContainText('1 document goes to Theo.');
    await expect(what).toContainText('1 person will report to Olivia (you).');
    await expect(what).toContainText('1 pending acknowledgement closes as “left organisation”.');
    await dialog.getByRole('button', { name: 'Back' }).click();
    await expect(dialog.getByText('Step 2 of 3: who takes over their work.')).toBeVisible();
    await dialog.getByRole('button', { name: 'Next' }).click();
    await dialog.getByRole('button', { name: 'Deactivate' }).click();
    await expect(page.getByText('Nina can no longer sign in to this workspace.')).toBeVisible();
    await expect(page.getByText('1 document now owned by Theo.')).toBeVisible();
    await expect(page.getByText('1 person now reports to Olivia (you).')).toBeVisible();

    // The server agrees, and the acknowledgement closed as "left" once the event was handled.
    await request.post('/__test/drain');
    expect((await (await owner.c.get(`/api/v1/artifacts/${artifact.id}`, { headers: h })).json()).artifact.owner_membership_id).toBe(theo);
    expect((await (await owner.c.get(`/api/v1/members/${sam}/directory`, { headers: h })).json()).manager_membership_id).toBe(me);
    const after = (await (await owner.c.get(`/api/v1/campaigns/${camp.id}`, { headers: h })).json()).progress;
    expect([after.assigned, after.left, after.acknowledged]).toEqual([before.assigned - 1, 1, 0]);
    const left = (await (await owner.c.get(`/api/v1/campaigns/${camp.id}/recipients`, { headers: h })).json()).items.find((r: { membership_id: string }) => r.membership_id === nina);
    expect(left.state).toBe('left');
  });
});
