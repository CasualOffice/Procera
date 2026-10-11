import { type APIRequestContext, type Page, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { axe, nav, signIn, WEB } from '../../fixtures/ui';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// Directory and member management in the browser (BPMS-023, docs/design/ia-competitive-analysis.md §5.4, §8.3).
// E2E-R1-UI-51 — departments: empty state, add (parent, head), rename inline, move (no cycles), delete only when empty.
// E2E-R1-UI-52 — groups: empty state, new group, add several people, remove one, delete.
// E2E-R1-UI-53 — person page: edit profile, add to and remove from a group, manager loop refused, role chip opens the role.
// E2E-R1-UI-54 — deactivate, then reactivate with a reason and step-up; resend and revoke a pending invitation.
// E2E-R1-UI-55 — Members bulk actions: Set department and Add to group on the selected rows (invitations skipped).

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

  test('E2E-R1-UI-52 an owner creates a group, adds and removes people, and deletes it', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    await member(request, owner, ws.id, 'Nina');
    await member(request, owner, ws.id, 'Theo');
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await home(page);
    await nav(page, 'Groups');
    await expect(page.getByRole('heading', { level: 1, name: 'Groups' })).toBeVisible();
    await expect(page.getByText('No groups yet')).toBeVisible();
    await expect(page.getByText(/Groups are flexible sets of people across departments/)).toBeVisible();
    await axe(page);

    await page.getByRole('button', { name: 'New group' }).click();
    let dialog = page.getByRole('dialog', { name: 'New group' });
    await dialog.getByRole('textbox', { name: 'Name' }).fill('Fire wardens');
    await dialog.getByRole('textbox', { name: /Purpose/ }).fill('Trained to lead evacuations');
    await dialog.getByRole('button', { name: 'Create group' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Fire wardens' })).toBeVisible();
    await expect(page.getByText('Group created. Add the people who belong in it.')).toBeVisible();

    await page.getByRole('button', { name: 'Add members' }).first().click();
    dialog = page.getByRole('dialog', { name: 'Add members to Fire wardens' });
    await dialog.getByRole('textbox', { name: 'Find people' }).fill('ni');
    await dialog.getByRole('checkbox', { name: /^Nina/ }).check();
    await dialog.getByRole('textbox', { name: 'Find people' }).fill('');
    await dialog.getByRole('checkbox', { name: /^Theo/ }).check();
    await axe(page);
    await dialog.getByRole('button', { name: 'Add 2 people' }).click();
    await expect(page.getByText('Added 2 people to Fire wardens.')).toBeVisible();
    const list = page.getByRole('list', { name: 'Members of Fire wardens' });
    await expect(list.getByRole('link')).toHaveText(['Nina', 'Theo']);
    await axe(page);

    await list.getByRole('button', { name: 'Remove Theo' }).click();
    dialog = page.getByRole('dialog', { name: 'Remove Theo from Fire wardens?' });
    await dialog.getByRole('button', { name: 'Remove' }).click();
    await expect(page.getByText('Theo removed from Fire wardens.')).toBeVisible();
    await expect(list.getByRole('link')).toHaveText(['Nina']);

    await page.getByRole('navigation', { name: 'People' }).getByRole('link', { name: 'Groups' }).click();
    const row = page.getByRole('row', { name: /Fire wardens/ });
    await expect(row).toContainText('Trained to lead evacuations');
    await expect(row).toContainText('1 person');
    await axe(page);

    await row.getByRole('link', { name: 'Fire wardens' }).click();
    await page.getByRole('button', { name: 'Delete group' }).click();
    dialog = page.getByRole('dialog', { name: 'Delete Fire wardens?' });
    await dialog.getByRole('button', { name: 'Delete group' }).click();
    await expect(page.getByText('No groups yet')).toBeVisible();
    expect((await (await owner.c.get('/api/v1/groups', { headers: owner.headers })).json()).items).toEqual([]);
  });

  test('E2E-R1-UI-53 an admin edits a person’s profile, groups and sees their roles; a manager loop is refused', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const nina = await member(request, owner, ws.id, 'Nina');
    const theo = await member(request, owner, ws.id, 'Theo');
    expect((await owner.c.post('/api/v1/departments', { headers: owner.headers, data: { name: 'Finance' } })).ok()).toBe(true);
    expect((await owner.c.post('/api/v1/groups', { headers: owner.headers, data: { name: 'Auditors' } })).ok()).toBe(true);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await home(page);

    await page.goto(`${WEB}/members/${nina.id}`);
    await expect(page.getByRole('heading', { level: 1, name: 'Nina' })).toBeVisible();
    await page.getByRole('button', { name: 'Edit profile' }).click();
    let dialog = page.getByRole('dialog', { name: 'Edit Nina’s profile' });
    await dialog.getByRole('textbox', { name: 'Title' }).fill('Accountant');
    await pick(page, dialog, /Department/, 'Finance');
    await pick(page, dialog, /Manager/, 'Theo');
    await dialog.getByRole('textbox', { name: 'Location' }).fill('Bangalore');
    await axe(page);
    await dialog.getByRole('button', { name: 'Save profile' }).click();
    await expect(page.getByText('Profile saved.')).toBeVisible();
    await expect(page.getByText('Accountant', { exact: true })).toBeVisible();
    await expect(page.getByRole('definition').filter({ hasText: 'Finance' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Theo' })).toBeVisible();
    await expect(page.getByRole('definition').filter({ hasText: 'Bangalore' })).toBeVisible();

    // Groups: add, then remove after confirming.
    await page.getByRole('button', { name: 'Add to group' }).click();
    dialog = page.getByRole('dialog', { name: 'Add Nina to a group' });
    await expect(dialog.getByRole('button', { name: /Group/ })).toContainText('Auditors');
    await dialog.getByRole('button', { name: 'Add to group' }).click();
    await expect(page.getByText('Nina added to Auditors.')).toBeVisible();
    const groups = page.getByRole('list', { name: 'Groups' });
    await expect(groups.getByRole('link', { name: 'Auditors' })).toBeVisible();
    await axe(page);
    await groups.getByRole('button', { name: 'Remove from Auditors' }).click();
    await page.getByRole('dialog', { name: 'Remove Nina from Auditors?' }).getByRole('button', { name: 'Remove' }).click();
    await expect(page.getByText('Nina removed from Auditors.')).toBeVisible();
    await expect(groups).toHaveCount(0);

    // Theo can't report to Nina: she reports to him.
    await page.goto(`${WEB}/members/${theo.id}`);
    await page.getByRole('button', { name: 'Edit profile' }).click();
    dialog = page.getByRole('dialog', { name: 'Edit Theo’s profile' });
    await pick(page, dialog, /Manager/, 'Nina');
    await dialog.getByRole('button', { name: 'Save profile' }).click();
    await expect(dialog.getByRole('alert')).toContainText('Can’t save: this manager link would create a reporting cycle.');
    await dialog.getByRole('button', { name: 'Cancel' }).click();

    // Role chips open the role on Roles & access.
    await page.getByRole('link', { name: 'Role: Member' }).click();
    await expect(page).toHaveURL(/\/admin\/roles\?role=/);
    await expect(page.getByRole('heading', { level: 1, name: 'Roles & access' })).toBeVisible();

    const dir = await (await owner.c.get(`/api/v1/members/${nina.id}/directory`, { headers: owner.headers })).json();
    expect([dir.title, dir.location, dir.manager_membership_id, dir.group_ids]).toEqual(['Accountant', 'Bangalore', theo.id, []]);
  });

  test('E2E-R1-UI-54 an owner deactivates and reactivates a member (reason, step-up) and resends and revokes an invitation', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const nina = await member(request, owner, ws.id, 'Nina');
    const pending = uniqueEmail('pending');
    expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: pending, roles: ['member'] } })).ok()).toBe(true);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await home(page);

    await page.goto(`${WEB}/members/${nina.id}`);
    await page.getByRole('button', { name: 'Deactivate' }).click();
    await page.getByRole('dialog', { name: 'Deactivate Nina?' }).getByRole('button', { name: 'Deactivate' }).click();
    await expect(page.getByText('Nina can no longer sign in to this workspace.')).toBeVisible();

    // Reactivation restores access, so it asks for a reason (and step-up once the sign-in is older than
    // the window — covered by the API tests; the frozen test clock leaves no unused TOTP step here).
    await page.getByRole('button', { name: 'Reactivate' }).click();
    const dialog = page.getByRole('dialog', { name: 'Reactivate Nina?' });
    await expect(dialog).toContainText('with the roles they had (Member)');
    await dialog.getByRole('button', { name: 'Reactivate' }).click();
    await expect(dialog.getByText('Give a reason.')).toBeVisible();
    await dialog.getByRole('textbox', { name: 'Reason' }).fill('Back from leave');
    await axe(page);
    await dialog.getByRole('button', { name: 'Reactivate' }).click();
    await expect(page.getByText('Nina can sign in to this workspace again.')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Deactivate' })).toBeVisible();

    // Pending invitations: resend (a new link), then revoke after confirming.
    await page.goto(`${WEB}/admin/members`);
    await page.getByRole('button', { name: `Resend invitation to ${pending}` }).click();
    await expect(page.getByText(`Invitation sent again to ${pending}. The earlier link no longer works.`)).toBeVisible();
    await page.getByRole('button', { name: `Revoke invitation to ${pending}` }).click();
    await page.getByRole('dialog', { name: `Revoke the invitation to ${pending}?` }).getByRole('button', { name: 'Revoke invitation' }).click();
    await expect(page.getByText(`Invitation to ${pending} revoked.`)).toBeVisible();
    await expect(page.getByRole('button', { name: `Resend invitation to ${pending}` })).toHaveCount(0);
    await axe(page);

    const list = (await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json()).items as { id: string; status: string }[];
    expect(list.find((m) => m.id === nina.id)!.status).toBe('active');
    expect((await (await owner.c.get('/api/v1/members/invitations', { headers: owner.headers })).json()).items).toEqual([]);
  });

  test('E2E-R1-UI-55 an owner selects members and sets their department and adds them to a group in bulk', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const nina = await member(request, owner, ws.id, 'Nina');
    const theo = await member(request, owner, ws.id, 'Theo');
    const pending = uniqueEmail('pending');
    expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: pending, roles: ['member'] } })).ok()).toBe(true);
    const finance = await (await owner.c.post('/api/v1/departments', { headers: owner.headers, data: { name: 'Finance' } })).json();
    const auditors = await (await owner.c.post('/api/v1/groups', { headers: owner.headers, data: { name: 'Auditors' } })).json();
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await home(page);
    await nav(page, 'Members');
    await expect(page.getByRole('heading', { level: 1, name: 'Members' })).toBeVisible();

    for (const who of ['Nina', 'Theo', pending]) await page.getByRole('checkbox', { name: `Select ${who}` }).click();
    await page.getByRole('button', { name: 'Set department' }).click();
    let dialog = page.getByRole('dialog', { name: 'Set department' });
    await pick(page, dialog, /Department/, 'Finance');
    await expect(dialog.getByText('Moves Nina, Theo to Finance.', { exact: false })).toBeVisible();
    await expect(dialog.getByText('Skipped: 1 invitation (only active members can be changed).')).toBeVisible();
    await axe(page);
    await dialog.getByRole('button', { name: 'Set department for 2 people' }).click();
    await expect(page.getByText('2 people now in Finance.')).toBeVisible();

    for (const who of ['Nina', 'Theo']) await page.getByRole('checkbox', { name: `Select ${who}` }).click();
    await page.getByRole('button', { name: 'Add to group' }).click();
    dialog = page.getByRole('dialog', { name: 'Add to group' });
    await expect(dialog.getByRole('button', { name: /Group/ })).toContainText('Auditors');
    await dialog.getByRole('button', { name: 'Add 2 people' }).click();
    await expect(page.getByText('Added 2 people to Auditors.')).toBeVisible();

    for (const id of [nina.id, theo.id]) {
      const dir = await (await owner.c.get(`/api/v1/members/${id}/directory`, { headers: owner.headers })).json();
      expect([dir.department_id, dir.group_ids]).toEqual([finance.id, [auditors.id]]);
    }
  });
});

