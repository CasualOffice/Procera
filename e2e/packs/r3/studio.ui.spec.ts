import { type APIRequestContext, type Browser, type Page } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { expect, test } from '../../fixtures/test-controls';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R3-UI-01 — the Guided Studio (BPMS-100, docs/05 §3.5): an owner designs "Laptop request" from
// the Blank template in Admin › Processes (request card, two form fields, a manager approval due in
// 2 business days with a reminder), validates it clean (axe on the designer) and publishes it; an
// employee then finds it under New request and submits it, their manager approves it from My work,
// and the request ends Approved.
// E2E-R3-UI-01 also test-runs the draft (send back, then approve) before publishing.
// E2E-R3-UI-02 — a step assigned to a group that doesn't exist: Validate lists the problem, it shows
// on that step, and Publish is refused with the same problems.
// E2E-R3-UI-05 — "who will this go to": under each Who row the designer shows who it reaches today
// (a group's members by name, a missing group as nobody, the manager rule with how many lack one).

/** A member who joined by invitation with a display name; returns what a browser sign-in needs. */
async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const s = await enter(email, ws);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, recovery: s.recovery, session: s as Session, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

async function secondPage(browser: Browser): Promise<Page> {
  return (await browser.newContext()).newPage();
}

/** Admin › Processes › New process from the Blank template; lands in the designer. */
async function newBlankProcess(page: Page, title: string) {
  await nav(page, 'Processes');
  await expect(page.getByRole('heading', { level: 1, name: 'Processes' })).toBeVisible();
  await page.getByRole('button', { name: 'New process' }).first().click();
  const dialog = page.getByRole('dialog', { name: 'New process' });
  await dialog.getByRole('textbox', { name: /^Name/ }).fill(title);
  await expect(dialog.getByRole('textbox', { name: /^Key/ })).toHaveValue(title.toLowerCase().replace(/\s+/g, '-'));
  await dialog.getByRole('radiogroup', { name: /Start from/ }).getByText(/^Blank/).first().click(); // the input is visually hidden
  await dialog.getByRole('button', { name: 'Create and open' }).click();
  await expect(page).toHaveURL(/\/admin\/processes\/[^/?]+$/);
  await expect(page.getByRole('heading', { level: 1, name: title })).toBeVisible();
}

/** Adds a step from the flow's "Add step" menu and names it in the step settings. */
async function addStep(page: Page, kind: 'Approval' | 'Task' | 'Acknowledgement', name?: string) {
  await page.getByRole('button', { name: 'Add step', exact: true }).click();
  await page.getByRole('menuitem', { name: new RegExp(`^${kind}`) }).click();
  const props = page.getByRole('complementary', { name: kind });
  await expect(props).toBeVisible();
  if (name) {
    await props.getByRole('textbox', { name: /^Step name/ }).fill(name);
    await expect(page.getByRole('complementary', { name })).toBeVisible();
  }
  return page.getByRole('complementary', { name: name ?? kind });
}

const saved = (page: Page) => expect(page.getByText('All changes saved')).toBeVisible();

test.describe('guided studio @r3 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // provisions a workspace and signs three people in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R3-UI-01 an owner designs and publishes a laptop request; an employee submits it and their manager approves it', async ({ page, browser, request, controls }) => {
    const { ws, owner, ownerEmail } = await workspaceWithOwner(request);
    const mo = await person(request, owner, ws.id, 'Mo Manager');
    const eve = await person(request, owner, ws.id, 'Eve Employee');
    expect((await owner.c.patch(`/api/v1/members/${eve.id}`, { headers: owner.headers, data: { manager_membership_id: mo.id } })).ok()).toBe(true);

    await signIn(page, ownerEmail, owner.recovery[0]!);
    await newBlankProcess(page, 'Laptop request');

    // The request card: how it appears under New request.
    const card = page.getByRole('region', { name: 'Request', exact: true });
    await card.getByRole('textbox', { name: /^Request name/ }).fill('Laptop request');
    await card.getByRole('textbox', { name: 'Description' }).fill('Ask for a new or replacement laptop.');
    await card.getByRole('button', { name: /Category/ }).click();
    await page.getByRole('option', { name: 'IT', exact: true }).click();
    await card.getByRole('textbox', { name: 'Reference prefix' }).fill('lap');
    await expect(card.getByRole('textbox', { name: 'Reference prefix' })).toHaveValue('LAP');

    // Two form fields: a choice with two options and a long text.
    const fields = page.getByRole('region', { name: 'Form fields' });
    await fields.getByRole('button', { name: 'Add field' }).click();
    let dialog = page.getByRole('dialog', { name: 'Add a field' });
    await dialog.getByRole('textbox', { name: /^Label/ }).fill('Model');
    await dialog.getByRole('button', { name: /Type/ }).click();
    await page.getByRole('option', { name: 'Choice', exact: true }).click();
    await dialog.getByRole('textbox', { name: 'Options' }).fill('MacBook Air\nThinkPad X1');
    await dialog.getByText('Required', { exact: true }).click(); // React Aria's input is visually hidden: press the label
    await dialog.getByRole('button', { name: 'Add field' }).click();
    await expect(dialog).toHaveCount(0);
    await fields.getByRole('button', { name: 'Add field' }).click();
    dialog = page.getByRole('dialog', { name: 'Add a field' });
    await dialog.getByRole('textbox', { name: /^Label/ }).fill('Reason');
    await dialog.getByRole('button', { name: /Type/ }).click();
    await page.getByRole('option', { name: 'Long text', exact: true }).click();
    await dialog.getByText('Required', { exact: true }).click(); // React Aria's input is visually hidden: press the label
    await dialog.getByRole('button', { name: 'Add field' }).click();
    const list = fields.getByRole('list', { name: 'Form fields' });
    await expect(list.getByRole('listitem')).toHaveCount(2);
    await expect(list.getByRole('listitem').first()).toContainText('Model');
    await expect(list.getByRole('listitem').first()).toContainText('Choice');
    await expect(list.getByRole('listitem').nth(1)).toContainText('Long text');

    // One approval: the requester's manager, due in 2 business days, with a reminder.
    const props = await addStep(page, 'Approval', 'Manager approval');
    await expect(props.getByRole('button', { name: /Who/ })).toContainText('The requester’s manager');
    await expect(props.getByRole('checkbox', { name: 'Set a deadline' })).toBeChecked();
    await props.getByRole('textbox', { name: /^Business days to finish/ }).fill('2');
    await props.getByRole('button', { name: /Reminders/ }).click();
    await page.getByRole('option', { name: 'One reminder after a day' }).click();
    const steps = page.getByRole('list', { name: 'Steps' });
    const stepCard = steps.getByRole('listitem').filter({ has: page.getByRole('button', { name: 'Manager approval' }) });
    await expect(stepCard).toContainText('The requester’s manager');
    await expect(stepCard).toContainText('Due in 2 business days · 1 reminder');
    await expect(stepCard.getByRole('list', { name: 'Outcomes of Manager approval' })).toContainText('Approve');
    await expect(stepCard.getByRole('list', { name: 'Outcomes of Manager approval' })).toContainText('Back to the requester');
    await saved(page);

    await page.getByRole('button', { name: 'Validate' }).click();
    await expect(page.getByRole('status').filter({ hasText: /No (blocking )?problems/ })).toBeVisible();
    await expect(page.getByRole('region', { name: /problems? to fix/ })).toHaveCount(0);
    await axe(page);

    // Test run: send back, then approve — the requester revises, then it ends Approved; nothing is stored.
    await page.getByRole('button', { name: 'Test run' }).click();
    const test = page.getByRole('dialog', { name: 'Test run' });
    await test.getByRole('button', { name: /Manager approval/ }).click();
    await page.getByRole('option', { name: 'Send back, then approve' }).click();
    await test.getByRole('button', { name: 'Run test' }).click();
    const trace = test.getByRole('list', { name: 'Test run trace' });
    await expect(trace).toContainText('goes to the requester');
    await expect(trace.getByRole('listitem')).toHaveCount(4); // submitted, sent back, revised, approved
    await expect(test.getByRole('status')).toContainText('Ends Approved');
    await axe(page);
    await test.getByRole('button', { name: 'Close', exact: true }).last().click();

    await page.getByRole('button', { name: 'Publish' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Published v1. People can now request it under New request.' })).toBeVisible();
    const listed = await (await owner.c.get('/api/v1/processes', { headers: owner.headers })).json();
    const row = (listed.items as { title: string; published: { version_no: number } | null }[]).find((p) => p.title === 'Laptop request');
    expect(row?.published?.version_no).toBe(1);

    // Eve asks for a laptop.
    const emp = await secondPage(browser);
    await signIn(emp, eve.email, eve.recovery[0]!);
    await nav(emp, 'Requests');
    await emp.getByRole('navigation', { name: 'Request views' }).getByRole('link', { name: 'New request', exact: true }).click();
    await emp.getByRole('link', { name: 'Laptop request', exact: true }).click();
    await expect(emp.getByRole('heading', { level: 1, name: 'Laptop request' })).toBeVisible();
    const form = emp.getByRole('form', { name: 'Laptop request' });
    await form.getByRole('radiogroup', { name: /Model/ }).getByText('ThinkPad X1', { exact: true }).click();
    await form.getByRole('textbox', { name: /^Reason/ }).fill('My current laptop no longer charges.');
    await form.getByRole('button', { name: 'Submit request' }).click();
    const done = emp.getByRole('status').filter({ hasText: /^Success: Submitted / });
    await expect(done).toContainText('Mo Manager will review it.');
    const ref = (/Submitted (\S+)\./.exec((await done.textContent()) ?? '') ?? [])[1] ?? '';
    expect(ref).toMatch(/^LAP-[\d-]+$/); // e.g. LAP-2026-000001
    const eveRequest = emp.url();
    await controls.drain();

    // Mo approves it from My work.
    const mgr = await secondPage(browser);
    await signIn(mgr, mo.email, mo.recovery[0]!);
    await nav(mgr, 'My work');
    await mgr.getByRole('navigation', { name: 'Open work' }).getByRole('link').filter({ hasText: ref }).click();
    const panel = mgr.getByRole('region', { name: 'Your decision' });
    await panel.getByRole('button', { name: 'Approve' }).click();
    await expect(mgr.getByRole('status').filter({ hasText: `Approved ${ref}.` })).toBeVisible();
    await controls.drain();

    await emp.goto(eveRequest.replace(/\?.*$/, ''));
    await expect(emp.getByText('Approved', { exact: true }).first()).toBeVisible();
    await expect(emp.getByRole('list', { name: 'Approval steps' })).toContainText('Approved by Mo Manager');
  });

  test('E2E-R3-UI-02 a step for a group that does not exist is a problem on that step, and Publish is refused', async ({ page, request }) => {
    const { owner, ownerEmail } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery[0]!);
    await newBlankProcess(page, 'Night desk request');
    await page.getByRole('region', { name: 'Request', exact: true }).getByRole('textbox', { name: 'Reference prefix' }).fill('NDR');

    const props = await addStep(page, 'Approval', 'Night desk approval');
    await props.getByRole('button', { name: /Who/ }).click();
    await page.getByRole('option', { name: 'Anyone in a group' }).click();
    await props.getByRole('combobox', { name: 'Group' }).fill('Night owls');
    await expect(props.getByTestId('who-reach')).toHaveText('Nobody yet: it would go to the fallback.');
    await saved(page);

    await page.getByRole('button', { name: 'Validate' }).click();
    const problems = page.getByRole('region', { name: /1 problem to fix|problems to fix/ });
    await expect(problems).toBeVisible();
    await expect(problems.getByRole('button', { name: 'Go to Night desk approval' })).toBeVisible();
    const stepCard = page.getByRole('list', { name: 'Steps' }).getByRole('listitem').filter({ has: page.getByRole('button', { name: 'Night desk approval' }) });
    await expect(stepCard).toContainText('Problem:');
    await expect(props.getByRole('list', { name: 'Problems with this step' })).toBeVisible();
    await problems.getByText('Details').first().click();
    await expect(problems).toContainText('IR-E025');

    await page.getByRole('button', { name: 'Publish' }).click();
    await expect(page.getByRole('alert').filter({ hasText: 'Not published. Fix the problems below first.' })).toBeVisible();
    await expect(stepCard).toContainText('Problem:');
    const listed = await (await owner.c.get('/api/v1/processes', { headers: owner.headers })).json();
    const row = (listed.items as { title: string; published: unknown }[]).find((p) => p.title === 'Night desk request');
    expect(row?.published).toBeNull();
  });

  test('E2E-R3-UI-05 each Who row shows who it reaches today', async ({ page, request }) => {
    const { ws, owner, ownerEmail } = await workspaceWithOwner(request);
    const h = owner.headers;
    const ann = await person(request, owner, ws.id, 'Ann Asker');
    const bo = await person(request, owner, ws.id, 'Bo Boss');
    const grp = await (await owner.c.post('/api/v1/groups', { headers: h, data: { name: 'Laptop desk' } })).json();
    for (const m of [bo, ann]) expect((await owner.c.post(`/api/v1/groups/${grp.id}/members`, { headers: h, data: { membership_id: m.id } })).ok()).toBe(true);
    await signIn(page, ownerEmail, owner.recovery[0]!);
    await newBlankProcess(page, 'Desk request');

    const props = await addStep(page, 'Approval', 'Desk approval');
    const reach = props.getByTestId('who-reach');
    // The default is the requester's manager: nobody in this workspace has one yet.
    await expect(reach).toHaveText('3 people have no manager set; their requests go to the fallback.');
    await props.getByRole('button', { name: /Who/ }).click();
    await page.getByRole('option', { name: 'Anyone in a group' }).click();
    await props.getByRole('combobox', { name: 'Group' }).fill('Laptop desk');
    await expect(reach).toHaveText('Goes to 2 people: Ann Asker, Bo Boss.');
    await page.keyboard.press('Escape'); // close the group suggestions
    await props.getByRole('button', { name: 'Add someone else' }).click();
    await props.getByRole('combobox', { name: 'Group' }).nth(1).fill('Nobody here');
    await expect(reach.nth(1)).toHaveText('Nobody yet: it would go to the fallback.');
    await axe(page);
  });
});
