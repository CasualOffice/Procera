// Self-review for the built site, served over HTTP like GitHub Pages serves it.
//
//   SITE_URL=http://127.0.0.1:8765 python3 site/build.py        # so the 404 page's absolute links resolve
//   python3 -m http.server -d site/dist 8765 &
//   PLAYWRIGHT=e2e/node_modules/playwright node site/scripts/review.mjs [outDir] [baseUrl]
//
// Screenshots every page at 1440 and 390 px in light and dark, plus reduced motion, plus a few
// mid-animation frames of the hero story. Checks: internal links and anchors resolve, every <img>
// and role="img" has a text alternative, no horizontal overflow, no console errors or failed
// requests, persona tabs work from the keyboard, and the hash-chain demo toggles.
import { createRequire } from 'node:module';
import { existsSync, mkdirSync, readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const dist = path.resolve(here, '..', 'dist');
const out = path.resolve(process.argv[2] || path.join(here, '..', 'review'));
const base = (process.argv[3] || process.env.SITE_BASE || 'http://127.0.0.1:8765').replace(/\/$/, '');
const require = createRequire(import.meta.url);
const { chromium } = require(path.resolve(process.env.PLAYWRIGHT || path.join(here, '..', '..', 'e2e', 'node_modules', 'playwright')));

if (!existsSync(dist)) { console.error('Build first: python3 site/build.py'); process.exit(1); }
mkdirSync(out, { recursive: true });
const pages = readdirSync(dist).filter((f) => f.endsWith('.html'));
const problems = [];
const slug = (p) => p.replace('.html', '');

// ---------------------------------------------------------------- static checks
for (const p of pages) {
  const html = readFileSync(path.join(dist, p), 'utf8');
  for (const m of html.matchAll(/(?:href|src|srcset)="([^"#]*)(#[^"]*)?"/g)) {
    let url = m[1];
    if (url.startsWith(base + '/')) url = url.slice(base.length + 1);
    if (!url || /^(https?:|mailto:|data:)/.test(url)) continue;
    if (!existsSync(path.join(dist, url))) problems.push(`${p}: broken link ${url}`);
    const hash = m[2];
    if (hash && url.endsWith('.html')) {
      const target = readFileSync(path.join(dist, url), 'utf8');
      if (!target.includes(`id="${hash.slice(1)}"`)) problems.push(`${p}: missing anchor ${url}${hash}`);
    }
  }
  for (const m of html.matchAll(/href="#([^"]+)"/g)) {
    if (!html.includes(`id="${m[1]}"`)) problems.push(`${p}: missing anchor #${m[1]}`);
  }
  for (const m of html.matchAll(/<img\b[^>]*>/g)) {
    if (!/\balt="[^"]+"/.test(m[0])) problems.push(`${p}: img without alt ${m[0].slice(0, 80)}`);
  }
  for (const m of html.matchAll(/<svg\b[^>]*role="img"[^>]*>/g)) {
    if (!/aria-label="[^"]+"/.test(m[0])) problems.push(`${p}: svg role=img without aria-label`);
  }
  if (/\bMelp\b/i.test(html)) problems.push(`${p}: mentions Melp`);
  if (/\b(compliant|certified|conformant)\b/i.test(html.replace(/&#x27;/g, "'").replace(/&quot;/g, '"').replace(/doesn\S{1,6}t make anyone compliant|does not make an organisation compliant|it isn't certified|holds no certifications|never "compliant" or "certified"/g, ''))) {
    problems.push(`${p}: uses compliant/certified wording`);
  }
}

// ---------------------------------------------------------------- browser checks
const browser = await chromium.launch();
async function visit(page, p) {
  await page.goto(`${base}/${p === 'index.html' ? '' : p}`, { waitUntil: 'load' });
  await page.evaluate(async () => {
    for (let y = 0; y < document.body.scrollHeight; y += 500) { window.scrollTo({ top: y, behavior: 'instant' }); await new Promise((r) => setTimeout(r, 60)); }
    window.scrollTo({ top: 0, behavior: 'instant' });
  });
  await page.waitForLoadState('networkidle').catch(() => {});
  await page.waitForTimeout(400);
}
function watch(page, label) {
  page.on('pageerror', (e) => problems.push(`${label}: JS error ${e.message}`));
  page.on('console', (m) => { if (m.type() === 'error' && !/404 \(File not found\)|status of 404/.test(m.text())) problems.push(`${label}: console ${m.text()}`); });
  page.on('requestfailed', (r) => problems.push(`${label}: request failed ${r.url()}`));
}

const modes = [
  ...['light', 'dark'].flatMap((scheme) => [1440, 390].map((width) => ({ scheme, width, motion: 'no-preference' }))),
  { scheme: 'light', width: 1440, motion: 'reduce' }, { scheme: 'dark', width: 390, motion: 'reduce' },
];
for (const m of modes) {
  const ctx = await browser.newContext({ viewport: { width: m.width, height: 900 }, colorScheme: m.scheme, reducedMotion: m.motion, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  const tag = `${m.width}-${m.scheme}${m.motion === 'reduce' ? '-reduced' : ''}`;
  watch(page, tag);
  for (const p of pages) {
    await visit(page, p);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    if (overflow > 1) problems.push(`${p} @${tag}: horizontal overflow ${overflow}px`);
    await page.screenshot({ path: path.join(out, `${slug(p)}-${tag}.png`), fullPage: true });
  }
  // 360 px reflow check on every page (no screenshots)
  if (m.width === 390 && m.motion !== 'reduce') {
    await page.setViewportSize({ width: 360, height: 800 });
    for (const p of pages) {
      await visit(page, p);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      if (overflow > 1) problems.push(`${p} @360 ${m.scheme}: horizontal overflow ${overflow}px`);
    }
  }
  await ctx.close();
}

// ---------------------------------------------------------------- interactions and animation frames
{
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  watch(page, 'interactions');
  await page.goto(`${base}/`, { waitUntil: 'networkidle' });
  for (const [i, wait] of [[1, 2200], [2, 2600], [3, 3400], [4, 4200], [5, 3000]]) {
    await page.waitForTimeout(wait);
    await page.locator('.hero').screenshot({ path: path.join(out, `hero-frame-${i}.png`) });
  }
  // Keyboard tabs: focus the first tab, arrow right, expect the second tab selected and its panel shown.
  await page.locator('#tab-hr').focus();
  await page.keyboard.press('ArrowRight');
  const ok = await page.evaluate(() => document.activeElement.id === 'tab-manager'
    && document.getElementById('tab-manager').getAttribute('aria-selected') === 'true'
    && !document.getElementById('panel-manager').hidden && document.getElementById('panel-hr').hidden);
  if (!ok) problems.push('persona tabs: ArrowRight did not move selection');
  await page.keyboard.press('End');
  await page.locator('#personas').screenshot({ path: path.join(out, 'personas-compliance.png') });
  // Hash-chain demo
  await page.click('[data-tamper]');
  const bad = await page.locator('[data-status]').textContent();
  if (!/fails/.test(bad)) problems.push('chain demo: tamper did not fail verification');
  await page.locator('[data-chain]').screenshot({ path: path.join(out, 'chain-tampered.png') });
  await page.click('[data-restore]');
  // Tour: scroll the third step into the middle and capture the frame.
  await page.locator('.tour-step[data-i="2"]').scrollIntoViewIfNeeded();
  await page.evaluate(() => window.scrollBy(0, 200));
  await page.waitForTimeout(900);
  await page.screenshot({ path: path.join(out, 'tour-step-3.png') });
  await ctx.close();
}
await browser.close();
console.log(problems.length ? problems.join('\n') : 'review: no broken links, missing alt text, overflow or console errors');
console.log(`review: screenshots in ${out}`);
process.exit(problems.length ? 1 : 0);
