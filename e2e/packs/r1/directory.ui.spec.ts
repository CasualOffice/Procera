import { type APIRequestContext, type Page, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// Directory and member management in the browser (BPMS-023, docs/design/ia-competitive-analysis.md §5.4, §8.3).
// E2E-R1-UI-51 — departments: empty state, add (parent, head), rename inline, move (no cycles), delete only when empty.

const home = (page: Page) => expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();

/** A member who joined by invitation; returns their email and membership id. */
async function member(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase());
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

/** Picks `option` in the React Aria select labelled `label` inside `scope`. */
async function pick(page: Page, scope: ReturnType<Page['getByRole']>, label: RegExp, option: string) {
  await scope.getByRole('button', { name: label }).click();
  await page.getByRole('option', { name: option, exact: true }).click();
}

test.describe('directory @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-51 an owner builds the department tree: add, rename, move, and delete only when empty', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await home(page);
    await nav(page, 'Departments');
    await expect(page.getByRole('heading', { level: 1, name: 'Departments' })).toBeVisible();
    await expect(page.getByText('No departments yet')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Import from CSV' })).toBeVisible();
    await axe(page);

    // Add a top-level department, then one inside it with a head.
    await page.getByRole('button', { name: 'Add department' }).click();
    let dialog = page.getByRole('dialog', { name: 'Add department' });
    await dialog.getByRole('textbox', { name: 'Name' }).fill('Operations');
    await dialog.getByRole('button', { name: 'Add department' }).click();
    await expect(page.getByText('Operations added.')).toBeVisible();
    const tree = page.getByRole('list', { name: 'Department tree' });
    await expect(tree.getByRole('button', { name: /^Operations/ })).toHaveAttribute('aria-current', 'true');

    await page.getByRole('button', { name: 'Add department' }).click();
    dialog = page.getByRole('dialog', { name: 'Add department' });
    await expect(dialog.getByRole('button', { name: /Inside/ })).toContainText('Operations'); // defaults to the selected one
    await dialog.getByRole('textbox', { name: 'Name' }).fill('operations/x');
    await expect(dialog.getByText('Names can’t contain “/”.')).toBeVisible();
    await dialog.getByRole('textbox', { name: 'Name' }).fill('Support');
    await pick(page, dialog, /Head/, 'Olivia');
    await axe(page);
    await dialog.getByRole('button', { name: 'Add department' }).click();
    await expect(page.getByText('Support added.')).toBeVisible();
    const detail = page.getByRole('region', { name: 'Support' });
    await expect(detail.getByText('Inside Operations')).toBeVisible();
    await expect(detail.getByRole('link', { name: 'Olivia' })).toBeVisible();

    // Rename inline.
    await detail.getByRole('button', { name: 'Rename' }).click();
    await page.getByRole('textbox', { name: 'Department name' }).fill('Customer Support');
    await page.getByRole('button', { name: 'Save name' }).click();
    await expect(page.getByText('Renamed to Customer Support.')).toBeVisible();
    await expect(page.getByRole('heading', { level: 2, name: 'Customer Support' })).toBeVisible();

    // Operations can't be deleted while it holds a sub-department, and can't move under it.
    await tree.getByRole('button', { name: /^Operations/ }).click();
    const ops = page.getByRole('region', { name: 'Operations' });
    await ops.getByRole('button', { name: 'Delete department' }).click();
    dialog = page.getByRole('dialog', { name: 'Move things out of Operations first' });
    await expect(dialog.getByText(/Customer Support/)).toBeVisible();
    await dialog.getByRole('button', { name: 'Got it' }).click();
    await ops.getByRole('button', { name: 'Move to…' }).click();
    dialog = page.getByRole('dialog', { name: 'Move Operations' });
    await expect(dialog.getByText('Operations can’t move inside itself or one of its own sub-departments, so Customer Support isn’t listed.')).toBeVisible();
    await dialog.getByRole('button', { name: /Move inside/ }).click();
    await expect(page.getByRole('option', { name: 'Top level' })).toBeVisible();
    await expect(page.getByRole('option', { name: /Customer Support/ })).toHaveCount(0);
    await page.keyboard.press('Escape');
    await dialog.getByRole('button', { name: 'Cancel' }).click();

    // Move the child to the top level; then Operations is empty and can go.
    await tree.getByRole('button', { name: /^Customer Support/ }).click();
    await page.getByRole('region', { name: 'Customer Support' }).getByRole('button', { name: 'Move to…' }).click();
    dialog = page.getByRole('dialog', { name: 'Move Customer Support' });
    await pick(page, dialog, /Move inside/, 'Top level');
    await dialog.getByRole('button', { name: 'Move', exact: true }).click();
    await expect(page.getByText('Customer Support moved to the top level.')).toBeVisible();
    await tree.getByRole('button', { name: /^Operations/ }).click();
    await page.getByRole('region', { name: 'Operations' }).getByRole('button', { name: 'Delete department' }).click();
    dialog = page.getByRole('dialog', { name: 'Delete Operations?' });
    await dialog.getByRole('button', { name: 'Delete department' }).click();
    await expect(page.getByText('Operations deleted.')).toBeVisible();
    await expect(tree.getByRole('button')).toHaveCount(1);
    await axe(page);

    const items = (await (await owner.c.get('/api/v1/departments', { headers: owner.headers })).json()).items;
    expect(items.map((d: { name: string; parent_id: string | null }) => [d.name, d.parent_id])).toEqual([['Customer Support', null]]);
  });
});

