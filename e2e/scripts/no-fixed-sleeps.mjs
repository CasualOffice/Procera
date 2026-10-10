// Fails if any scenario uses fixed sleeps (docs/23 §7). Use controls.drain()/advanceClock() instead.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const banned = [/waitForTimeout\s*\(/, /setTimeout\s*\(/, /\bsleep\s*\(/];
const offenders = [];
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if (p.endsWith('.ts')) {
      readFileSync(p, 'utf8').split('\n').forEach((line, i) => {
        if (banned.some((re) => re.test(line))) offenders.push(`${p}:${i + 1}: ${line.trim()}`);
      });
    }
  }
};
walk(new URL('../packs', import.meta.url).pathname);
walk(new URL('../fixtures', import.meta.url).pathname);
if (offenders.length) {
  console.error('Fixed sleeps are forbidden in E2E tests:\n' + offenders.join('\n'));
  process.exit(1);
}
console.log('no fixed sleeps found');
