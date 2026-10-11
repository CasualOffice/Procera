// Self-review for the built site: screenshots every page at 1440 and 390 px wide in light
// and dark, and checks internal links, image alt text and horizontal overflow.
//
//   node site/scripts/review.mjs [outDir]
//
// Playwright is not a site dependency. Point PLAYWRIGHT at an install, e.g. the e2e one:
//   PLAYWRIGHT=e2e/node_modules/playwright node site/scripts/review.mjs
import { createRequire } from 'node:module';
import { existsSync, mkdirSync, readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const dist = path.resolve(here, '..', 'dist');
const out = path.resolve(process.argv[2] || path.join(here, '..', 'review'));
const require = createRequire(import.meta.url);
const { chromium } = require(path.resolve(process.env.PLAYWRIGHT || path.join(here, '..', '..', 'e2e', 'node_modules', 'playwright')));

if (!existsSync(dist)) { console.error('Build first: python3 site/build.py'); process.exit(1); }
mkdirSync(out, { recursive: true });
const pages = readdirSync(dist).filter((f) => f.endsWith('.html'));
const problems = [];

// Static checks: every internal href/src resolves, every <img> has non-empty alt.
for (const p of pages) {
  const html = readFileSync(path.join(dist, p), 'utf8');
  for (const m of html.matchAll(/(?:href|src|srcset)="([^"#]*)(#[^"]*)?"/g)) {
    const url = m[1];
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
}

const browser = await chromium.launch();
for (const scheme of ['light', 'dark']) {
  for (const width of [1440, 390]) {
    const ctx = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: scheme, deviceScaleFactor: 1 });
    const page = await ctx.newPage();
    page.on('pageerror', (e) => problems.push(`JS error: ${e.message}`));
    for (const p of pages) {
      await page.goto(pathToFileURL(path.join(dist, p)).href, { waitUntil: 'load' });
      // Lazy images: scroll through so they load before the full-page shot.
      await page.evaluate(async () => {
        for (let y = 0; y < document.body.scrollHeight; y += 700) { window.scrollTo(0, y); await new Promise((r) => setTimeout(r, 40)); }
        window.scrollTo(0, 0);
      });
      await page.waitForLoadState('networkidle').catch(() => {});
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      if (overflow > 1) problems.push(`${p} @${width} ${scheme}: horizontal overflow ${overflow}px`);
      await page.screenshot({ path: path.join(out, `${p.replace('.html', '')}-${width}-${scheme}.png`), fullPage: true });
    }
    await ctx.close();
  }
}
await browser.close();
console.log(problems.length ? problems.join('\n') : 'review: no broken links, missing alt text or overflow');
console.log(`review: screenshots in ${out}`);
process.exit(problems.length ? 1 : 0);
