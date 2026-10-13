import { type APIRequestContext, type Browser, type Page } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { expect, test } from '../../fixtures/test-controls';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R2-UI-15 — employee onboarding through the web UI (BPMS-103): an HR person starts onboarding
// for a new member; IT, Facilities and the new member's manager each see their task in My work and
// mark it done (in any order: the three run at the same time); then the new member sees the
// acknowledgement and confirms it, and the request ends "Onboarded". Axe on the form and the
// request page, and on the acknowledgement.

async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const s = await enter(email, ws);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, recovery: s.recovery, session: s as Session, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

type Person = Awaited<ReturnType<typeof person>>;

async function group(owner: Session, name: string, members: Person[]) {
  const res = await owner.c.post('/api/v1/groups', { headers: owner.headers, data: { name } });
  expect(res.status(), await res.text()).toBe(201);
  const g = await res.json();
  for (const m of members) expect((await owner.c.post(`/api/v1/groups/${g.id}/members`, { headers: owner.headers, data: { membership_id: m.id } })).ok()).toBe(true);
}

async function as(browser: Browser, who: Person): Promise<Page> {
  const page = await (await browser.newContext()).newPage();
  await signIn(page, who.email, who.recovery[0]!);
  return page;
}

/** Opens the onboarding item for `ref` in My work. */
async function openWork(page: Page, ref: string) {
  await nav(page, 'My work');
  await page.getByRole('navigation', { name: 'Open work' }).getByRole('link').filter({ hasText: ref }).click();
}

/** Marks a task done from My work, filling its one field. */
async function markDone(page: Page, ref: string, step: string, field: string, value: string) {
  await openWork(page, ref);
  const panel = page.getByRole('region', { name: 'Your decision' });
  await expect(panel).toContainText(step);
  await panel.getByRole('textbox', { name: field }).fill(value);
  await panel.getByRole('button', { name: 'Mark done' }).click();
  await expect(page.getByRole('status').filter({ hasText: `Done ${ref}. It’s off your list.` })).toBeVisible();
}

const inTwoWeeks = () => new Date(Date.now() + 14 * 86_400_000).toISOString().slice(0, 10);

test.describe('employee onboarding @r2 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // provisions a workspace and signs five people in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-UI-15 HR starts onboarding; IT, Facilities and the manager finish their tasks in any order; the new member acknowledges', async ({ page, browser, request, controls }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const hana = await person(request, owner, ws.id, 'Hana HR');
    const ian = await person(request, owner, ws.id, 'Ian IT');
    const fay = await person(request, owner, ws.id, 'Fay Facilities');
    const mo = await person(request, owner, ws.id, 'Mo Manager');
    const nia = await person(request, owner, ws.id, 'Nia New');
    expect((await owner.c.patch(`/api/v1/members/${nia.id}`, { headers: owner.headers, data: { manager_membership_id: mo.id } })).ok()).toBe(true);
    await group(owner, 'IT', [ian]);
    await group(owner, 'Facilities', [fay]);

    // Hana fills the onboarding form.
    await signIn(page, hana.email, hana.recovery[0]!);
    await nav(page, 'Requests');
    await page.getByRole('navigation', { name: 'Request views' }).getByRole('link', { name: 'New request', exact: true }).click();
    await page.getByRole('link', { name: 'Employee onboarding', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Employee onboarding' })).toBeVisible();
    const form = page.getByRole('form', { name: 'Employee onboarding' });
    await form.getByRole('button', { name: /New member/ }).click();
    await page.getByRole('option', { name: 'Nia New', exact: true }).click();
    await form.getByLabel(/^First day/).fill(inTwoWeeks());
    await form.getByRole('textbox', { name: /^Job title/ }).fill('Product designer');
    await form.getByRole('textbox', { name: /^Department/ }).fill('Design');
    await form.getByRole('radiogroup', { name: /Equipment/ }).getByText('Standard laptop', { exact: true }).click();
    await form.getByRole('textbox', { name: /^Notes for the team/ }).fill('Prefers a standing desk');
    const who = page.getByRole('region', { name: 'Who approves this' });
    await expect(who).toContainText("Ian IT (IT), Fay Facilities (Facilities) and Mo Manager (Nia New's manager) at the same time, then Nia New");
    await axe(page);
    await form.getByRole('button', { name: 'Submit request' }).click();
    const submitted = page.getByRole('status').filter({ hasText: /^Success: Submitted ON-\d{4}-\d{6}/ });
    await expect(submitted).toBeVisible();
    const ref = (await submitted.textContent())!.match(/ON-\d{4}-\d{6}/)![0];
    const requestUrl = page.url().replace(/\?.*$/, '');

    // The request page shows the three tasks running at the same time.
    const steps = page.getByRole('list', { name: 'Approval steps' });
    const together = steps.getByRole('list', { name: '3 steps at the same time' });
    await expect(together.getByRole('listitem')).toHaveCount(3);
    await expect(steps).toContainText('At the same time');
    await expect(steps).toContainText('0 of 3 done · any order');
    for (const s of ['Set up accounts and equipment', 'Prepare workspace and access badge', 'Plan the first week']) await expect(together).toContainText(s);
    await expect(together).toContainText('Waiting for Ian IT');
    await expect(together).toContainText('Waiting for Mo Manager');
    await axe(page);
    await controls.drain();

    // Hana's list says where it is in a few words.
    await nav(page, 'Requests');
    await page.getByRole('navigation', { name: 'Request views' }).getByRole('link', { name: 'My requests', exact: true }).click();
    await expect(page.getByRole('row').filter({ hasText: ref })).toContainText('3 steps at once');

    // Facilities first, then the manager, then IT: the order does not matter.
    const facilities = await as(browser, fay);
    await markDone(facilities, ref, 'Prepare workspace and access badge', 'Desk and badge number', '3F-12, badge 0042');
    const manager = await as(browser, mo);
    await openWork(manager, ref);
    await expect(manager.getByRole('region', { name: 'Your decision' })).toContainText("You're Nia New's manager");
    await markDone(manager, ref, 'Plan the first week', 'Plan for the first week', 'Pairing with the design lead on Monday');

    // The new member has nothing yet: the join waits for IT.
    const newbie = await as(browser, nia);
    await nav(newbie, 'My work');
    await expect(newbie.getByText('Nothing is waiting for you', { exact: true })).toBeVisible();

    const it = await as(browser, ian);
    await markDone(it, ref, 'Set up accounts and equipment', 'Laptop asset tag', 'LT-0193');
    await controls.drain();

    // Nia confirms: one button, no approve or reject.
    await newbie.reload();
    await openWork(newbie, ref);
    const ack = newbie.getByRole('region', { name: 'Your acknowledgement' });
    await expect(ack).toContainText('Confirm equipment and handbook');
    await expect(ack).toContainText('I confirm I have my equipment and have read the employee handbook.');
    await expect(ack.getByRole('button', { name: 'Approve' })).toHaveCount(0);
    await expect(ack.getByRole('textbox', { name: 'Comment' })).toHaveCount(0);
    await axe(newbie);
    await ack.getByRole('button', { name: 'Acknowledge' }).click();
    await expect(newbie.getByRole('status').filter({ hasText: `Acknowledged ${ref}. It’s off your list.` })).toBeVisible();
    await controls.drain();

    // Hana sees it finished: Onboarded, with every step done.
    await page.goto(requestUrl);
    await expect(page.getByText('Onboarded', { exact: true })).toBeVisible();
    await expect(page.getByRole('list', { name: 'Approval steps' })).toContainText('3 of 3 done');
    await expect(page.getByRole('list', { name: 'Approval steps' })).toContainText('Acknowledged by Nia New');
    const timeline = page.getByRole('region', { name: 'Timeline' });
    await expect(timeline).toContainText('Finished: Onboarded');
    await expect(timeline).toContainText('Set up accounts and equipment: done by Ian IT');
    await axe(page);
  });
});
