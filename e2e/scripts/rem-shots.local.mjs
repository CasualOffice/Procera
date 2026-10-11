import { chromium } from '@playwright/test';
const out = process.argv[2];
const b = await chromium.launch();
for (const [w, h] of [[390, 844], [1280, 800], [1920, 1080], [2560, 1440]]) {
  const p = await b.newPage({ viewport: { width: w, height: h } });
  await p.goto('http://127.0.0.1:5173/sign-in');
  await p.getByLabel('Work email').fill('olivia@acme.test');
  await p.getByLabel('Password').fill('correct horse battery staple');
  await p.getByRole('button', { name: 'Sign in' }).click();
  await p.waitForURL((u) => !u.pathname.includes('sign-in'), { timeout: 20000 });
  await p.waitForTimeout(1500);
  const root = await p.evaluate(() => getComputedStyle(document.documentElement).fontSize);
  console.log(w, 'root font-size', root);
  await p.screenshot({ path: `${out}/home-${w}.png` });
  await p.close();
}
await b.close();
