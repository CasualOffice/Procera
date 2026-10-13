import { type APIRequestContext, type Browser, type Page, expect } from '@playwright/test';
import { PW } from './accounts';
import { WEB, nav } from './ui';

// Journey helpers (E2E-R1-FLOW-*, R1 gate): people arrive from the emails the product sent, each in
// their own browser.

/** The newest email of `kind` to `to`, after delivery runs; returns the first link in its text. */
export async function emailLink(request: APIRequestContext, ws: string, to: string, kind: string, path: RegExp): Promise<string> {
  let link = '';
  await expect.poll(async () => {
    await request.post('/__test/drain');
    const mail = await (await request.get(`/__test/mail?workspace=${ws}&to=${encodeURIComponent(to)}`)).json();
    const m = (mail.items as { kind: string; text: string }[]).find((x) => x.kind === kind);
    link = m?.text.match(/https?:\/\/\S+/)?.[0] ?? '';
    return path.test(link);
  }, { message: `a ${kind} email to ${to}` }).toBe(true);
  // The test build mails links on the web origin; keep only the path so the browser stays on WEB.
  const u = new URL(link);
  return `${WEB}${u.pathname}${u.search}`;
}

export async function personPage(browser: Browser): Promise<Page> {
  return (await browser.newContext()).newPage();
}

/** Sign-in form, password only (members without an admin role have no second step). */
export async function signInForm(page: Page, email: string) {
  await expect(page.getByRole('heading', { level: 1, name: 'Sign in to Procera' })).toBeVisible();
  await page.getByLabel('Work email').fill(email);
  await page.getByLabel(/^Password/).fill(PW);
  await page.getByRole('button', { name: 'Sign in' }).click();
}

/** The owner invites `email` from Members (Member role unless another is named). */
export async function inviteFromMembers(page: Page, email: string, role?: string) {
  await nav(page, 'Members');
  await page.getByRole('button', { name: 'Invite people' }).click();
  const dialog = page.getByRole('dialog', { name: 'Invite people' });
  await dialog.getByLabel('Email address').fill(email);
  if (role) await dialog.getByRole('button', { name: new RegExp(`^${role}`) }).click();
  await dialog.getByRole('button', { name: 'Send invitation' }).click();
  await expect(page.getByRole('row', { name: new RegExp(email) }).getByText('Invited')).toBeVisible();
}

/** A new person opens the invitation email, creates an account and lands on Home in the workspace. */
export async function joinAsNewPerson(browser: Browser, request: APIRequestContext, ws: string, email: string, name: string): Promise<Page> {
  const link = await emailLink(request, ws, email, 'invitation', /\/invite\?token=/);
  const page = await personPage(browser);
  await page.goto(link);
  await expect(page.getByRole('heading', { level: 1, name: 'Join Acme' })).toBeVisible();
  await expect(page.getByLabel('Work email')).toHaveValue(email);
  await page.getByLabel('Your name').fill(name);
  await page.getByLabel(/^Password/).fill(PW);
  await page.getByLabel('Confirm password').fill(PW);
  await page.getByRole('button', { name: 'Create account and join' }).click();
  await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
  return page;
}

