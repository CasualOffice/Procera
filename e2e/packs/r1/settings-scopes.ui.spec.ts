import { expect, test } from '@playwright/test';
import { hasDatabase, serverNow } from '../../fixtures/accounts';
import { totp } from '../../fixtures/totp';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-35..36 — Settings screen: department overrides and the configuration profile switch
// (BPMS-027; docs/20 §0-§1; docs/05 §3.16).

test.describe('settings scopes @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // provisions a workspace and signs in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-35 departments: override a setting for a parent department, see a child inherit it, then clear it', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const fin = await (await owner.c.post('/api/v1/departments', { headers: owner.headers, data: { name: 'Finance' } })).json();
    const pay = await (await owner.c.post('/api/v1/departments', { headers: owner.headers, data: { name: 'Payroll', parent_id: fin.id } })).json();

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Settings');
    await page.getByRole('tab', { name: 'Departments' }).click();
    const dept = page.getByRole('combobox', { name: 'Department', exact: true });
    await dept.selectOption(fin.id);
    const row = page.getByRole('listitem').filter({ has: page.getByText('Time zone', { exact: true }) });
    await expect(row.getByText('Workspace value')).toBeVisible();
    await expect(row.getByText('UTC', { exact: true })).toBeVisible();
    await axe(page);

    await row.getByRole('button', { name: 'Override Time zone for Finance' }).click();
    await row.getByRole('textbox', { name: 'Time zone' }).fill('Europe/London');
    await row.getByRole('button', { name: 'Save for this department' }).click();
    await expect(row.getByText('Set for Finance')).toBeVisible();
    await expect(row.getByText('Europe/London', { exact: true })).toBeVisible();

    // The child department inherits its parent's override.
    await dept.selectOption(pay.id);
    await expect(row.getByText('From Finance')).toBeVisible();
    await expect(row.getByText('Europe/London', { exact: true })).toBeVisible();
    await expect(row.getByRole('button', { name: 'Clear Time zone for Payroll' })).toHaveCount(0);
    await axe(page);

    await dept.selectOption(fin.id);
    await row.getByRole('button', { name: 'Clear Time zone for Finance' }).click();
    await expect(row.getByText('Override removed')).toBeVisible();
    await expect(row.getByText('Workspace value')).toBeVisible();
    const e = await (await owner.c.get(`/api/v1/settings/effective?key=locale.timezone&scope=department:${pay.id}`, { headers: owner.headers })).json();
    expect([e.value, e.source]).toEqual(['UTC', 'default']);
  });

  test('E2E-R1-UI-36 configuration profile: preview a switch with every change, then switch with a reason', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request, 'standard');
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Settings');
    await page.getByRole('tab', { name: 'Configuration profile' }).click();
    await expect(page.getByText('This workspace uses the')).toContainText('Standard');
    await page.getByRole('radio', { name: /^Regulated/ }).check();

    const table = page.getByRole('table', { name: 'Changes in this switch' });
    const idle = table.getByRole('row', { name: /Sign out after inactivity/ });
    await expect(idle.getByRole('cell', { name: '30 minutes' })).toBeVisible();
    await expect(idle.getByRole('cell', { name: '15 minutes' })).toBeVisible();
    await expect(table.getByRole('row', { name: /Approvals needed before publishing/ })).toBeVisible();
    await axe(page);

    const go = page.getByRole('button', { name: 'Switch to Regulated' });
    await expect(go).toBeDisabled();
    await page.getByLabel('Reason for the switch').fill('Regulator asked for two reviewers');
    await go.click();
    // A parallel test may have moved the shared clock past the step-up window: confirm if asked.
    const done = page.getByText('This workspace now uses the Regulated profile.');
    const stepUp = page.getByRole('dialog', { name: 'Confirm it’s you' });
    await expect(done.or(stepUp)).toBeVisible();
    if (await stepUp.isVisible()) {
      await stepUp.getByLabel('Authentication code').fill(totp(owner.secret, await serverNow(owner.c), 1));
      await stepUp.getByRole('button', { name: /Confirm|Verify/ }).click();
    }
    await expect(done).toBeVisible();
    await expect(page.getByText('This workspace uses the')).toContainText('Regulated');
    await axe(page);

    const e = await (await owner.c.get('/api/v1/settings/effective?key=auth.session.idle_timeout', { headers: owner.headers })).json();
    expect([e.value, e.source]).toEqual([900, 'profile']);
  });
});
