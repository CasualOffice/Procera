import { type APIRequestContext, type Browser, type Locator, type Page } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { expect, test } from '../../fixtures/test-controls';
import { WEB, axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R2-UI-01..05 — requests and approvals through the web UI (BPMS-103/105): an employee submits a
// leave request, their manager approves it from Home and My work, and the employee sees the step
// history; an expense is sent back and resubmitted; a missing required answer shows on its field;
// axe on the catalogue, the form, the request page and My work.
//
// The request types and their forms come from the server's catalogue (GET /request-types), so the
// form is filled from its schema rather than from hard-coded field names.

type Field = { name: string; label: string; type: string; required?: boolean; options?: { value: string; label: string }[] };
type RequestType = { key: string; name: string; category: string; form: { fields: Field[] } };

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

/** Owner, employee Ben and his manager Ana (the manager relationship routes the approval). */
async function team(request: APIRequestContext) {
  const { ws, owner } = await workspaceWithOwner(request);
  const ana = await person(request, owner, ws.id, 'Ana Manager');
  const ben = await person(request, owner, ws.id, 'Ben Employee');
  expect((await owner.c.patch(`/api/v1/members/${ben.id}`, { headers: owner.headers, data: { manager_membership_id: ana.id } })).ok()).toBe(true);
  return { ws, owner, ana, ben };
}

/** The catalogue entry whose name matches `re`, as the employee sees it. */
async function requestType(s: Session, re: RegExp): Promise<RequestType> {
  const res = await s.c.get('/api/v1/request-types', { headers: s.headers });
  expect(res.status()).toBe(200);
  const items = (await res.json()).items as RequestType[];
  const t = items.find((x) => re.test(x.name) || re.test(x.key));
  expect(t, `a request type matching ${re} in ${items.map((x) => x.name).join(', ')}`).toBeTruthy();
  return t!;
}

const inTwoWeeks = () => new Date(Date.now() + 14 * 86_400_000).toISOString().slice(0, 10);

/** Fills every required field (and `extra` values by label) through the form's controls. */
async function fill(page: Page, form: Locator, fields: Field[], extra: Record<string, string> = {}) {
  for (const f of fields) {
    if (!f.required && !(f.label in extra)) continue;
    const escaped = f.label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const name = new RegExp(`^${escaped}`);
    const trigger = new RegExp(escaped); // a select's name is "<chosen value> <label>"

    const value = extra[f.label];
    switch (f.type) {
      case 'enum': {
        const opt = f.options?.find((o) => o.label === value) ?? f.options![0]!;
        if (f.options!.length <= 4) await form.getByRole('radiogroup', { name: trigger }).getByText(opt.label, { exact: true }).click(); // the input is visually hidden
        else { await form.getByRole('button', { name: trigger }).click(); await page.getByRole('option', { name: opt.label, exact: true }).click(); }
        break;
      }
      case 'principal':
        await form.getByRole('button', { name: trigger }).click();
        await page.getByRole('option').first().click();
        break;
      case 'boolean':
        await form.getByRole('checkbox', { name }).check();
        break;
      case 'date':
        await form.getByLabel(name).fill(value ?? inTwoWeeks());
        break;
      case 'money':
      case 'number':
        await form.getByRole('textbox', { name }).fill(value ?? (f.type === 'money' ? '120' : '2'));
        await form.getByRole('textbox', { name }).press('Tab');
        break;
      default:
        await form.getByRole('textbox', { name }).fill(value ?? `E2E ${f.label}`);
    }
  }
}

/** Ben opens the catalogue from the sidebar, picks the type and submits it; returns the request page's display id. */
async function submit(page: Page, t: RequestType, check = false): Promise<string> {
  await nav(page, 'Requests');
  await page.getByRole('navigation', { name: 'Request views' }).getByRole('link', { name: 'New request', exact: true }).click();
  await expect(page.getByRole('heading', { level: 2, name: t.category || 'General' })).toBeVisible();
  if (check) await axe(page);
  await page.getByRole('link', { name: t.name, exact: true }).click();
  await expect(page.getByRole('heading', { level: 1, name: t.name })).toBeVisible();
  const form = page.getByRole('form', { name: t.name });
  await fill(page, form, t.form.fields);
  if (check) await axe(page);
  await form.getByRole('button', { name: 'Submit request' }).click();
  const done = page.getByRole('status').filter({ hasText: /^Success: Submitted / });
  await expect(done).toContainText('Ana Manager will review it.');
  return (/Submitted (\S+)\./.exec((await done.textContent()) ?? '') ?? [])[1] ?? '';
}

async function secondPage(browser: Browser): Promise<Page> {
  return (await browser.newContext()).newPage();
}

test.describe('requests and approvals @r2 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // provisions a workspace and signs two people in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-UI-01 an employee submits a leave request; the manager approves it from Home and My work; the employee sees Approved with the step history', async ({ page, browser, request, controls }) => {
    const { ana, ben } = await team(request);
    const leave = await requestType(ben.session, /leave|time off/i);

    await signIn(page, ben.email, ben.recovery[0]!);
    const ref = await submit(page, leave);
    expect(ref).toMatch(/\S+-\d+/);
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    await expect(page.getByText('In progress', { exact: true })).toBeVisible();
    const benRequest = page.url();
    await controls.drain();

    const mgr = await secondPage(browser);
    await signIn(mgr, ana.email, ana.recovery[0]!);
    await expect(mgr.getByRole('heading', { name: 'Needs you' })).toBeVisible();
    const task = mgr.getByRole('listitem').filter({ hasText: `Approve ${leave.name} from Ben Employee` });
    await expect(task).toBeVisible();
    await expect(mgr.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'My work, 1 open' })).toBeVisible();
    await task.getByRole('button', { name: 'Open' }).click();

    await expect(mgr).toHaveURL(/\/work\/[^/?]+$/);
    const panel = mgr.getByRole('region', { name: 'Your decision' });
    await expect(panel).toBeVisible();
    await expect(panel).toContainText("Why you: You're Ben Employee's manager.");
    await panel.getByRole('textbox', { name: 'Comment' }).fill('Enjoy the break.');
    await panel.getByRole('button', { name: 'Approve' }).click();
    await expect(mgr.getByRole('status').filter({ hasText: `Approved ${ref}. It’s off your list.` })).toBeVisible();
    await expect(mgr.getByText('Nothing is waiting for you')).toBeVisible();
    await expect(mgr.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'My work', exact: true })).toBeVisible();
    await controls.drain();

    await page.goto(benRequest.replace(/\?.*$/, ''));
    await expect(page.getByText('Approved', { exact: true }).first()).toBeVisible();
    const steps = page.getByRole('list', { name: 'Approval steps' });
    await expect(steps).toContainText('Approved by Ana Manager');
    await expect(steps).toContainText('Enjoy the break.');
    await expect(page.getByRole('region', { name: 'Timeline' }).getByRole('listitem').first()).toBeVisible();
    // Why this route (PR-08): a disclosure with the plain reason for each step.
    await page.getByText('Why this route', { exact: true }).click();
    await expect(page.getByText("Ana Manager decides Manager approval as Ben Employee's manager.")).toBeVisible();
    await expect(page.getByRole('button', { name: 'Cancel request' })).toHaveCount(0);
  });

  test('E2E-R2-UI-02 a manager sends an expense back with a comment; the employee updates it and resubmits from their work item', async ({ page, browser, request, controls }) => {
    const { ana, ben } = await team(request);
    const expense = await requestType(ben.session, /expense/i);

    await signIn(page, ben.email, ben.recovery[0]!);
    const ref = await submit(page, expense);
    await controls.drain();

    const mgr = await secondPage(browser);
    await signIn(mgr, ana.email, ana.recovery[0]!);
    await nav(mgr, 'My work');
    await mgr.getByRole('navigation', { name: 'Open work' }).getByRole('link').filter({ hasText: ref }).click();
    const panel = mgr.getByRole('region', { name: 'Your decision' });
    // Sending back needs a reason: without one the comment field says so and nothing is sent.
    await panel.getByRole('button', { name: 'Send back' }).click();
    await expect(panel.getByText(/Say why you are sending it back/)).toBeVisible();
    await expect(panel.getByRole('textbox', { name: 'Comment' })).toHaveAttribute('aria-invalid', 'true');
    await panel.getByRole('textbox', { name: 'Comment' }).fill('Please attach the receipt total.');
    await panel.getByRole('button', { name: 'Send back' }).click();
    await expect(mgr.getByRole('status').filter({ hasText: `Sent back ${ref}.` })).toBeVisible();
    await controls.drain();

    // Ben's request shows it came back to him, with Ana's comment, and his work item lets him resubmit.
    await nav(page, 'Requests');
    await page.getByRole('navigation', { name: 'Request views' }).getByRole('link', { name: 'My requests', exact: true }).click();
    await page.getByRole('row').filter({ hasText: ref }).getByRole('link').first().click();
    await expect(page.getByText('Sent back', { exact: true }).first()).toBeVisible();
    await expect(page.getByRole('list', { name: 'Approval steps' })).toContainText('Please attach the receipt total.');
    const mine = page.getByRole('region', { name: 'Your decision' });
    await expect(mine).toBeVisible();
    const amount = expense.form.fields.find((f) => f.type === 'money');
    if (amount && (await mine.getByRole('textbox', { name: new RegExp(`^${amount.label}`) }).count())) {
      await mine.getByRole('textbox', { name: new RegExp(`^${amount.label}`) }).fill('95');
    }
    await mine.getByRole('textbox', { name: 'Comment' }).fill('Total is 95, receipt attached.');
    await mine.getByRole('button', { name: 'Resubmit' }).click();
    await expect(page.getByRole('status').filter({ hasText: `Resubmitted. ${ref} is off your list.` })).toBeVisible();
    await expect(page.getByText('In progress', { exact: true }).first()).toBeVisible();
    await controls.drain();

    // Back with Ana.
    await mgr.reload();
    await expect(mgr.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'My work, 1 open' })).toBeVisible();
    await expect(mgr.getByRole('navigation', { name: 'Open work' }).getByRole('link').filter({ hasText: ref })).toBeVisible();
  });

  test('E2E-R2-UI-03 a required answer left empty shows an error on its field and nothing is sent', async ({ page, request }) => {
    const { ben } = await team(request);
    const leave = await requestType(ben.session, /leave|time off/i);
    const first = leave.form.fields.find((f) => f.required && ['string', 'text', 'money', 'number'].includes(f.type)) ?? leave.form.fields.find((f) => f.required)!;
    expect(first, 'the leave form has a required field').toBeTruthy();

    await signIn(page, ben.email, ben.recovery[0]!);
    await expect(page.getByRole('heading', { name: 'Needs you' })).toBeVisible();
    await page.goto(`${WEB}/requests/new/${encodeURIComponent(leave.key)}`);
    const form = page.getByRole('form', { name: leave.name });
    await fill(page, form, leave.form.fields.filter((f) => f.name !== first.name));
    await form.getByRole('button', { name: 'Submit request' }).click();
    await expect(form.getByText(first.type === 'boolean' ? 'Tick this to continue.' : `Enter ${first.label.toLowerCase()}.`)).toBeVisible();
    if (['string', 'text', 'money', 'number'].includes(first.type)) {
      await expect(form.getByRole('textbox', { name: new RegExp(`^${first.label}`) })).toHaveAttribute('aria-invalid', 'true');
    }
    await expect(page).toHaveURL(/\/requests\/new\//);
    const mine = await (await ben.session.c.get('/api/v1/requests?scope=mine', { headers: ben.session.headers })).json();
    expect(mine.items).toHaveLength(0);
  });

  test('E2E-R2-UI-04 the catalogue, a request form, the request page and My work pass axe', async ({ page, browser, request, controls }) => {
    const { ana, ben } = await team(request);
    const leave = await requestType(ben.session, /leave|time off/i);

    await signIn(page, ben.email, ben.recovery[0]!);
    await submit(page, leave, true); // axe on the catalogue and the form
    await expect(page.getByRole('list', { name: 'Approval steps' })).toBeVisible();
    await axe(page);
    await controls.drain();

    const mgr = await secondPage(browser);
    await signIn(mgr, ana.email, ana.recovery[0]!);
    await nav(mgr, 'My work');
    await expect(mgr.getByRole('heading', { level: 1, name: 'My work' })).toBeVisible();
    await axe(mgr);
    await mgr.getByRole('navigation', { name: 'Open work' }).getByRole('link').first().click();
    await expect(mgr.getByRole('region', { name: 'Your decision' })).toBeVisible();
    await axe(mgr);
  });

  test('E2E-R2-UI-05 an admin reassigns an unassigned step from Requests oversight; a separation-of-duties refusal shows on the person field', async ({ page, request }) => {
    const { ws, owner, ownerEmail } = await workspaceWithOwner(request);
    const ana = await person(request, owner, ws.id, 'Ana Manager');
    const cara = await person(request, owner, ws.id, 'Cara Nomanager'); // no manager, no process owner: unassigned
    const leave = await requestType(cara.session, /leave|time off/i);
    const day = inTwoWeeks();
    const started = await cara.session.c.post('/api/v1/requests', {
      headers: { ...cara.session.headers, 'idempotency-key': `ui05-${Date.now()}` },
      data: { type: leave.key, input: { leave_type: 'annual', start_date: day, end_date: day } },
    });
    expect(started.status(), await started.text()).toBe(201);
    const { id, display_id } = await started.json();

    await signIn(page, ownerEmail, owner.recovery[0]!);
    await expect(page.getByRole('heading', { name: 'Needs attention' })).toBeVisible();
    await page.getByRole('link', { name: /request step has nobody to decide/ }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Requests oversight' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Unassigned' })).toHaveAttribute('aria-current', 'page');
    await axe(page);
    const row = page.getByRole('row').filter({ hasText: display_id });
    await row.getByRole('button', { name: /^Reassign/ }).click();
    const dialog = page.getByRole('dialog', { name: 'Reassign step' });
    await expect(dialog).toBeVisible();

    // The requester can't take her own step.
    await dialog.getByRole('button', { name: /Person/ }).click();
    await page.getByRole('option', { name: /Cara Nomanager/ }).click();
    await dialog.getByRole('textbox', { name: /^Reason/ }).fill('She knows the dates');
    await dialog.getByRole('button', { name: 'Reassign', exact: true }).click();
    await expect(dialog.getByText('They started this request, so separation of duties keeps them off this step')).toBeVisible();

    await dialog.getByRole('button', { name: /Person/ }).click();
    await page.getByRole('option', { name: /Ana Manager/ }).click();
    await dialog.getByRole('button', { name: 'Reassign', exact: true }).click();
    await expect(page.getByText(`${display_id} is now with Ana Manager.`)).toBeVisible();
    await expect(page.getByRole('row').filter({ hasText: display_id })).toHaveCount(0);

    const work = await (await ana.session.c.get('/api/v1/work?state=open', { headers: ana.session.headers })).json();
    expect((work.items as { request: { id: string } }[]).map((w) => w.request.id)).toContain(id);
    await page.goto(`${WEB}/requests/${id}`);
    await expect(page.getByText(/Reassigned: Manager approval to Ana Manager by .* \(She knows the dates\)/)).toBeVisible();
  });
});
