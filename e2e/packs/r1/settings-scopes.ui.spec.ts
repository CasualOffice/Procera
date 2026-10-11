import { expect, test } from '@playwright/test';
import { hasDatabase, serverNow } from '../../fixtures/accounts';
import { totp } from '../../fixtures/totp';
import { axe, nav, openSettings, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-35..36 — Settings: department overrides on the setting itself, and the starting point
// (configuration profile) switch from the index banner (BPMS-027; docs/20 §0-§1; docs/05 §3.16;
// docs/design/ia-competitive-analysis.md §6.4-6.5).

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
    await openSettings(page, 'General & branding');
    const row = page.getByRole('listitem').filter({ has: page.getByText('Time zone', { exact: true }) });
    await row.getByRole('button', { name: 'Same for all departments: Time zone' }).click();
    const panel = page.getByRole('dialog', { name: 'Time zone by department' });
    const finRow = panel.getByRole('listitem').filter({ has: page.getByText('Finance', { exact: true }) });
    const payRow = panel.getByRole('listitem').filter({ has: page.getByText('Payroll', { exact: true }) });
    await expect(finRow.getByText('Inherited from workspace')).toBeVisible();
    await expect(finRow.getByText('UTC', { exact: true })).toBeVisible();
    await axe(page);

    await finRow.getByRole('button', { name: 'Override Time zone for Finance' }).click();
    await finRow.getByRole('textbox', { name: 'Time zone for Finance' }).fill('Europe/London');
    await finRow.getByRole('button', { name: 'Save for this department' }).click();
    await expect(finRow.getByText('Set for Finance')).toBeVisible();
    await expect(finRow.getByText('Europe/London', { exact: true })).toBeVisible();

    // The child department inherits its parent's override and has nothing of its own to clear.
    await expect(payRow.getByText('Inherited from Finance')).toBeVisible();
    await expect(payRow.getByText('Europe/London', { exact: true })).toBeVisible();
    await expect(payRow.getByRole('button', { name: /^Use .* value for Payroll$/ })).toHaveCount(0);
    await axe(page);

    // The setting itself now says it differs in one department.
    await panel.getByRole('button', { name: 'Close' }).click();
    await row.getByRole('button', { name: 'Different in 1 department: Time zone' }).click();
    await finRow.getByRole('button', { name: 'Use workspace value for Finance' }).click();
    await expect(finRow.getByText('Override removed')).toBeVisible();
    await expect(finRow.getByText('Inherited from workspace')).toBeVisible();
    const e = await (await owner.c.get(`/api/v1/settings/effective?key=locale.timezone&scope=department:${pay.id}`, { headers: owner.headers })).json();
    expect([e.value, e.source]).toEqual(['UTC', 'default']);
  });

  test('E2E-R1-UI-36 starting point: preview a switch with every change, then switch with a reason', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request, 'standard');
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Settings');
    await expect(page.getByRole('heading', { level: 2, name: 'Starting point: Standard' })).toBeVisible();
    await page.getByRole('button', { name: 'Change starting point' }).click();
    const panel = page.getByRole('dialog', { name: 'Change starting point' });
    await expect(panel.getByText('This workspace uses the')).toContainText('Standard');
    await panel.getByRole('radio', { name: /^Regulated/ }).check();

    const table = panel.getByRole('table', { name: 'Changes in this switch' });
    const idle = table.getByRole('row', { name: /Sign out after inactivity/ });
    await expect(idle.getByRole('cell', { name: '30 minutes' })).toBeVisible();
    await expect(idle.getByRole('cell', { name: '15 minutes' })).toBeVisible();
    await expect(table.getByRole('row', { name: /Approvals needed before publishing/ })).toBeVisible();
    await axe(page);

    const go = panel.getByRole('button', { name: 'Switch to Regulated' });
    await expect(go).toBeDisabled();
    await panel.getByLabel('Reason for the switch').fill('Regulator asked for two reviewers');
    await go.click();
    // A parallel test may have moved the shared clock past the step-up window: confirm if asked.
    const done = panel.getByText('This workspace now uses the Regulated starting point.');
    const stepUp = page.getByRole('dialog', { name: 'Confirm it’s you' });
    await expect(done.or(stepUp)).toBeVisible();
    if (await stepUp.isVisible()) {
      await stepUp.getByLabel('Authentication code').fill(totp(owner.secret, await serverNow(owner.c), 1));
      await stepUp.getByRole('button', { name: /Confirm|Verify/ }).click();
    }
    await expect(done).toBeVisible();
    await expect(panel.getByText('This workspace uses the')).toContainText('Regulated');
    await axe(page);
    await panel.getByRole('button', { name: 'Close' }).click();
    await expect(page.getByRole('heading', { level: 2, name: 'Starting point: Regulated' })).toBeVisible();

    const e = await (await owner.c.get('/api/v1/settings/effective?key=auth.session.idle_timeout', { headers: owner.headers })).json();
    expect([e.value, e.source]).toEqual([900, 'profile']);
  });
});
