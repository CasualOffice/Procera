import { type Page, expect, test } from '@playwright/test';
import { PW, hasDatabase } from '../../fixtures/accounts';
import { relyingParty } from '../../fixtures/passkey';
import { axe } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-30..31 — passkeys in the browser (BPMS-022): Account › Security, sign-in with a passkey,
// and step-up with a passkey. Chromium's virtual authenticator (CDP WebAuthn domain) plays the
// device. Browsers refuse IP addresses as a WebAuthn RP ID, so these journeys open the web app at the
// server's relying-party origin (`localhost` for a loopback public URL).

async function virtualAuthenticator(page: Page) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('WebAuthn.enable');
  const { authenticatorId } = await cdp.send('WebAuthn.addVirtualAuthenticator', {
    options: { protocol: 'ctap2', transport: 'internal', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });
  return { cdp, authenticatorId };
}

async function openSecurity(page: Page) {
  await page.getByRole('button', { name: /^Account:/ }).click();
  await page.getByRole('menuitem', { name: 'Security' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Security' })).toBeVisible();
  await expect(page.getByRole('menu')).toHaveCount(0); // the account menu has finished closing before any axe scan
}

test.describe('passkeys @r1 @ui', () => {
  test.beforeEach(async ({ request, browserName }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
    test.skip(browserName !== 'chromium', 'virtual authenticators are a Chromium DevTools feature');
  });

  test('E2E-R1-UI-30 add a passkey from Account › Security, sign out, sign in with password + passkey', async ({ page, request }) => {
    const { origin } = await relyingParty(request);
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const { cdp, authenticatorId } = await virtualAuthenticator(page);

    await page.goto(`${origin}/sign-in`);
    await page.getByLabel('Work email').fill(ownerEmail);
    await page.getByLabel(/^Password/).fill(PW);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    // TOTP account without a passkey: no passkey button yet.
    await expect(page.getByRole('heading', { level: 1, name: 'Two-step verification' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Use a passkey' })).toHaveCount(0);
    await page.getByLabel('Code').fill(owner.recovery[0]!);
    await page.getByRole('button', { name: 'Verify' }).click();
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();

    await openSecurity(page);
    await expect(page.getByText('No passkeys yet')).toBeVisible();
    await expect(page.getByText('On', { exact: true })).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: 'Add a passkey' }).click();
    const dialog = page.getByRole('dialog', { name: 'Add a passkey' });
    await dialog.getByLabel('Passkey name').fill('Test laptop');
    await axe(page);
    await dialog.getByRole('button', { name: 'Continue' }).click();
    await expect(page.getByRole('status').filter({ hasText: '“Test laptop” is ready' })).toBeVisible();
    const list = page.getByRole('list', { name: 'Your passkeys' });
    await expect(list.getByText('Test laptop')).toBeVisible();
    await expect(list.getByText('Not used yet')).toBeVisible();
    const { credentials } = await cdp.send('WebAuthn.getCredentials', { authenticatorId });
    expect(credentials).toHaveLength(1);
    await axe(page);

    // Sign out, then sign in with the password and the passkey.
    await page.getByRole('button', { name: /^Account:/ }).click();
    await page.getByRole('menuitem', { name: 'Sign out' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Sign in to Procera' })).toBeVisible();
    await page.getByLabel('Work email').fill(ownerEmail);
    await page.getByLabel(/^Password/).fill(PW);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await expect(page.getByText('Use your passkey, or enter the 6-digit code')).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: 'Use a passkey' }).click();
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await openSecurity(page);
    await expect(page.getByRole('list', { name: 'Your passkeys' }).getByText(/Last used/)).toBeVisible();
  });

  test('E2E-R1-UI-31 removing a passkey asks to confirm it’s you; the passkey itself passes step-up', async ({ page, request }) => {
    const { origin } = await relyingParty(request);
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await virtualAuthenticator(page);
    await page.goto(`${origin}/sign-in`);
    await page.getByLabel('Work email').fill(ownerEmail);
    await page.getByLabel(/^Password/).fill(PW);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await page.getByLabel('Code').fill(owner.recovery[0]!);
    await page.getByRole('button', { name: 'Verify' }).click();
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await openSecurity(page);
    await page.getByRole('button', { name: 'Add a passkey' }).click();
    await page.getByRole('dialog', { name: 'Add a passkey' }).getByLabel('Passkey name').fill('Desk key');
    await page.getByRole('dialog', { name: 'Add a passkey' }).getByRole('button', { name: 'Continue' }).click();
    await expect(page.getByRole('list', { name: 'Your passkeys' }).getByText('Desk key')).toBeVisible();

    // The sign-in is now 10 minutes old (account step-up window: 5 minutes); reload to pick it up.
    expect((await request.post('/__test/session/age', { data: { email: ownerEmail, seconds: 600 } })).status()).toBe(204);
    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Security' })).toBeVisible();
    await page.getByRole('button', { name: 'Remove Desk key' }).click();
    const confirm = page.getByRole('alertdialog', { name: 'Remove “Desk key”?' });
    await expect(confirm).toContainText('won’t be able to sign in');
    await axe(page);
    await confirm.getByRole('button', { name: 'Remove passkey' }).click();
    const stepUp = page.getByRole('dialog', { name: 'Confirm it’s you' });
    await expect(stepUp).toContainText('Use your passkey, or enter a code');
    await axe(page);
    await stepUp.getByRole('button', { name: 'Use a passkey' }).click();
    await expect(page.getByRole('status').filter({ hasText: '“Desk key” was removed' })).toBeVisible();
    await expect(page.getByText('No passkeys yet')).toBeVisible();
  });
});
