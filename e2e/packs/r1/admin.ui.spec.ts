import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-14..17 — Administration screens in the browser: manage announcements, campaigns,
// settings and the audit log (BPMS-031 with BPMS-046/047/027/064; docs/05 §3.16, §3.19, §3.23, §3.24).

const doc = (text: string) => ({ blocks: [{ type: 'paragraph', text }] });

test.describe('administration @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // provisions a workspace and signs in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-14 manage announcements: compose a draft; a workspace-wide post is sent for approval; a targeted draft is published and shows receipts', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const draft = await owner.c.post('/api/v1/announcements', { headers: owner.headers, data: { title: 'Owners meeting', content: doc('Thursday at 10.'), audience: { roles: ['workspace.owner'] } } });
    expect(draft.status()).toBe(201);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Manage announcements');
    await expect(page.getByRole('heading', { level: 1, name: 'Manage announcements' })).toBeVisible();
    await page.getByRole('button', { name: 'New announcement' }).first().click();
    const composer = page.getByRole('dialog', { name: 'New announcement' });
    await composer.getByLabel('Title').fill('Office closed on Friday');
    await composer.getByLabel('Message').fill('The office is closed for maintenance.\n\n- Work from home\n- Badges still work');
    await composer.getByText('Ask people to confirm they’ve read it').click();
    await axe(page);
    await composer.getByRole('button', { name: 'Save draft' }).click();

    // The drawer opens on the new draft. Everyone-posts need approval in a Standard workspace.
    const drawer = page.getByRole('dialog', { name: 'Office closed on Friday' });
    await expect(drawer.getByText('Draft', { exact: true })).toBeVisible();
    await drawer.getByRole('button', { name: 'Publish now' }).click();
    await expect(drawer.getByText('This one needs approval')).toBeVisible();
    await drawer.getByRole('button', { name: 'Send for approval' }).click();
    await expect(drawer.getByText('Waiting for approval', { exact: true })).toBeVisible();
    await axe(page);
    await drawer.getByRole('button', { name: 'Close' }).click();

    // A post to one role needs no approval: publish it and see receipts.
    await page.getByRole('button', { name: 'Owners meeting' }).click();
    const d2 = page.getByRole('dialog', { name: 'Owners meeting' });
    await d2.getByRole('button', { name: 'Publish now' }).click();
    await expect(d2.getByText('Published', { exact: true })).toBeVisible();
    await expect(d2.getByRole('heading', { name: 'Receipts' })).toBeVisible();
    await expect(d2.getByText(/read \(0 of 1\)/)).toBeVisible();
    await d2.getByRole('button', { name: 'Close' }).click();
    await nav(page, 'Announcements');
    await expect(page.getByRole('button', { name: /Owners meeting/ })).toBeVisible();
  });

  test('E2E-R1-UI-15 campaigns: create from a published document with an audience, check the reach, launch, follow progress and download the evidence', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const { version } = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'policy', key: 'conduct', title: 'Code of Conduct', content: doc('Be kind.') } })).json();
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: owner.headers });
    expect((await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: owner.headers, data: {} })).status()).toBe(200);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Campaigns');
    await expect(page.getByText('No campaigns yet')).toBeVisible();
    await page.getByRole('button', { name: 'New campaign' }).first().click();
    const dialog = page.getByRole('dialog', { name: 'New campaign' });
    await dialog.getByRole('button', { name: /Code of Conduct/ }).click();
    await expect(dialog.getByLabel('Name')).toHaveValue('Acknowledge Code of Conduct');
    await expect(dialog.getByText('Sends to:')).toContainText('Everyone');
    await axe(page);
    await dialog.getByRole('button', { name: 'Create draft' }).click();

    await expect(page.getByRole('heading', { level: 1, name: 'Acknowledge Code of Conduct' })).toBeVisible();
    await expect(page.getByText('will be asked to acknowledge')).toBeVisible();
    await page.getByRole('button', { name: 'Launch' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Launched' })).toBeVisible();
    await expect(page.getByRole('table', { name: 'People in this campaign' }).getByText('Pending')).toBeVisible();
    await axe(page);
    const download = page.waitForEvent('download');
    await page.getByRole('button', { name: 'Evidence CSV' }).click();
    expect((await download).suggestedFilename()).toMatch(/evidence\.csv$/);
    await page.getByRole('list', { name: 'Breadcrumb' }).getByRole('link', { name: 'Campaigns' }).click(); // back to the list
    await expect(page.getByRole('row', { name: /Acknowledge Code of Conduct/ }).getByText('0 of 1')).toBeVisible();
  });

  test('E2E-R1-UI-16 settings: grouped by area; change a value in place, see why it applies, and step-up keys ask to confirm identity', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Settings');
    await expect(page.getByRole('heading', { level: 2, name: 'Security and sign-in' })).toBeVisible();
    await axe(page);

    const row = page.getByRole('listitem').filter({ has: page.getByText('Sign out after inactivity', { exact: true }) });
    await row.getByRole('button', { name: 'Change Sign out after inactivity' }).click();
    await row.getByRole('spinbutton').fill('45');
    await row.getByRole('combobox', { name: 'Unit' }).selectOption({ label: 'minutes' });
    await row.getByLabel('Reason for the change').fill('Shorter sessions on shared desks');
    await row.getByRole('button', { name: 'Save' }).click();
    await expect(row.getByText('45 minutes')).toBeVisible();
    await row.getByRole('button', { name: 'Why this value' }).click();
    await expect(row.getByText('Set for this workspace').last()).toBeVisible();
    await expect(row.getByText('In effect')).toBeVisible();

    // A step-up key asks the person to confirm it's them before it saves (once the recent sign-in is older than the window).
    // Age this person's sign-in (not the shared clock), then reload so the browser refreshes its token.
    expect((await request.post('/__test/session/age', { data: { email: ownerEmail, seconds: 3600 } })).status()).toBe(204);
    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Settings' })).toBeVisible();
    const mfa = page.getByRole('listitem').filter({ has: page.getByText('Two-step verification required for', { exact: true }) });
    await mfa.getByRole('button', { name: /^Change/ }).click();
    await mfa.getByRole('button', { name: 'Reviewer' }).click();
    await mfa.getByLabel('Reason for the change').fill('Reviewers approve policies');
    await mfa.getByRole('button', { name: 'Save' }).click();
    const stepUp = page.getByRole('dialog', { name: 'Confirm it’s you' });
    await expect(stepUp).toBeVisible();
    await axe(page);
    await stepUp.getByRole('button', { name: 'Cancel' }).click();
  });

  test('E2E-R1-UI-17 audit log: chain verified, events filter by category and person, a row expands to the record, an object opens its timeline', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'policy', key: 'travel', title: 'Travel', content: doc('Economy for short flights.') } });

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { name: 'For you' })).toBeVisible();
    await nav(page, 'Audit log');
    await expect(page.getByRole('heading', { level: 1, name: 'Audit log' })).toBeVisible();
    await expect(page.getByText('Chain verified')).toBeVisible();
    const table = page.getByRole('table', { name: 'Audit events' });
    await expect(table.getByRole('row').nth(1)).toBeVisible();
    await axe(page);

    await page.getByLabel('Category').selectOption({ label: 'Content' });
    await expect(table.getByText('Artifact created').first()).toBeVisible();
    await table.getByRole('button', { name: /^Artifact created .*details$/ }).first().click();
    await expect(table.getByText('Record', { exact: true })).toBeVisible();
    await table.getByRole('button', { name: /artifact version/i }).first().click();
    const tl = page.getByRole('dialog', { name: /history$/ });
    await expect(tl.getByText('Artifact created')).toBeVisible();
    await axe(page);
    await tl.getByRole('button', { name: 'Close' }).click();

    await page.getByLabel('Action').fill('no.such.action');
    await page.getByRole('button', { name: 'Apply' }).click();
    await expect(page.getByText('No events match these filters')).toBeVisible();
  });
});
