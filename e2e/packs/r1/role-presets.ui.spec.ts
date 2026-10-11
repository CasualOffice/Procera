import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-56 — roles for common jobs: an owner adds "HR lead" from the presets and is offered to grant it.

test('E2E-R1-UI-56 an owner adds the HR lead role from the job presets and goes straight to granting it @r1 @ui', async ({ page, request }) => {
  test.slow();
  test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  const { ownerEmail, owner } = await workspaceWithOwner(request);
  await signIn(page, ownerEmail, owner.recovery![0]!);
  await nav(page, 'Roles & access');
  await page.getByRole('button', { name: 'Add a role for a job' }).click();
  const presets = page.getByRole('dialog', { name: 'Add a role for a job' });
  await expect(presets.getByRole('list', { name: 'Roles for common jobs' }).getByRole('listitem').filter({ hasText: 'Finance lead' })).toBeVisible();
  await axe(page);
  await presets.getByRole('button', { name: 'Add HR lead' }).click();
  await expect(page.getByRole('dialog', { name: 'Grant a role' })).toBeVisible();
  await expect(page.getByText('“HR lead” is ready.')).toBeVisible();
  const roles = await (await owner.c.get('/api/v1/roles', { headers: owner.headers })).json();
  const hr = roles.items.find((r: { key: string }) => r.key === 'job.hr_lead');
  expect(hr.permissions).toEqual(expect.arrayContaining(['announcement:create', 'campaign:manage', 'policy:publish']));
});
