import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { WEB, axe, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-24 — re-acknowledgement on publish through the web UI (BPMS-045, FR-PL03): the publish dialog
// offers a major or minor change with its number, previews who is asked to re-acknowledge and by when,
// and the started campaign is linked from the document.

const doc = (text: string) => ({ blocks: [{ type: 'paragraph', text }] });

test.describe('re-acknowledgement on publish @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // provisions a workspace and signs in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-24 an owner publishes v2 as a major change, sees who will be asked, then finds the campaign', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    // A second member, so "everyone" is two people.
    const email = await seedUser(request, 'staff');
    await owner.c.post('/api/v1/members/invitations', { headers: h, data: { email, roles: ['member'] } });
    const token = await inviteToken(request, ws.id, email);
    const pw = await passwordSession(email);
    await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });
    await enter(email, ws.id);

    const { artifact, version } = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'safety.fire', title: 'Fire safety', content: doc('Know your exits.') } })).json();
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h });
    expect((await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: h, data: {} })).status()).toBe(200);
    const v2 = await (await owner.c.post(`/api/v1/artifacts/${artifact.id}/versions`, { headers: h })).json();
    await owner.c.patch(`/api/v1/versions/${v2.id}`, { headers: h, data: { content: doc('Know your exits. Drill every month.') } });
    expect((await (await owner.c.post(`/api/v1/versions/${v2.id}/submit`, { headers: h })).json()).lifecycle).toBe('approved');

    await signIn(page, ownerEmail, (owner as Session).recovery![0]!);
    await expect(page.getByRole('heading', { name: 'For you' })).toBeVisible();
    await page.goto(`${WEB}/library/${artifact.id}`);
    await expect(page.getByRole('heading', { level: 1, name: 'Fire safety' })).toBeVisible();

    await page.getByRole('button', { name: 'Publish', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Publish v2' });
    await dialog.getByLabel('What changed?').fill('Monthly drills');
    await expect(dialog.getByRole('radio', { name: /Major/ })).toBeChecked();
    await expect(dialog.getByText(/^Publishing v2\.0 asks 2 people to re-acknowledge by /)).toBeVisible();
    await axe(page);
    await dialog.getByText('Minor', { exact: true }).click();
    await expect(dialog.getByText('Publishing v1.1 does not ask anyone to re-acknowledge.')).toBeVisible();
    await dialog.getByText('Major', { exact: true }).click();
    await expect(dialog.getByText(/^Publishing v2\.0 asks 2 people/)).toBeVisible();
    await dialog.getByRole('button', { name: 'Publish now' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Published v2.0. 2 people asked to re-acknowledge.' })).toBeVisible();

    // The document links to the campaign; the owner is in the audience too, so they are asked as well.
    await expect(page.getByText('· 2.0')).toBeVisible();
    await expect(page.getByRole('region', { name: 'Acknowledgement requested' })).toBeVisible();
    await page.getByRole('link', { name: '0 of 2 people' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Re-acknowledge Fire safety v2.0' })).toBeVisible();
    await expect(page.getByText('Started by publishing')).toBeVisible();
    await axe(page);
  });
});
