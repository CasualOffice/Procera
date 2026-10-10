#!/usr/bin/env node
// JS licence gate (ADR-0014, docs/24 §3.12). Reads every package-lock.json (v2/v3) in the repo
// and fails on any package whose licence is not on the allow list. Offline; no dependencies.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const root = new URL('..', import.meta.url).pathname;

// Permissive licences. MPL-2.0 is file-level copyleft and allowed when used unmodified.
// EPL-2.0 is allowed only for unmodified, separately loaded modules (elkjs) — see SP-07.
const ALLOW = new Set([
  'MIT', 'MIT-0', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', '0BSD', 'CC0-1.0',
  'Unlicense', 'BlueOak-1.0.0', 'Python-2.0', 'CC-BY-4.0', 'MPL-2.0', 'EPL-2.0', 'Zlib',
  'OFL-1.1', // self-hosted fonts (Inter) — SP-07
]);
// Packages that are never allowed regardless of declared licence (watermark / commercial EULA).
const DENY_PACKAGES = new Set(['bpmn-js', 'dmn-js', '@bpmn-io/form-js', 'survey-creator-core', 'survey-creator-react', 'minio', 'redis']);

const lockfiles = [];
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    if (name === 'node_modules' || name === 'target' || name.startsWith('.git')) continue;
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if (name === 'package-lock.json') lockfiles.push(p);
  }
};
walk(root);

const ok = (expr) => {
  if (!expr) return false;
  const e = expr.replace(/[()]/g, '').trim();
  if (e.includes(' OR ')) return e.split(' OR ').some((x) => ok(x.trim())); // pick a permissive option
  if (e.includes(' AND ')) return e.split(' AND ').every((x) => ok(x.trim()));
  return ALLOW.has(e);
};

const problems = [];
let count = 0;
for (const file of lockfiles) {
  const lock = JSON.parse(readFileSync(file, 'utf8'));
  for (const [path, meta] of Object.entries(lock.packages ?? {})) {
    if (path === '') continue; // the project itself
    const name = path.split('node_modules/').pop();
    count++;
    if (DENY_PACKAGES.has(name)) problems.push(`${relative(root, file)}: ${name} is on the deny list`);
    else if (!ok(meta.license)) problems.push(`${relative(root, file)}: ${name}@${meta.version} licence "${meta.license ?? 'UNKNOWN'}" not allowed`);
  }
}
if (problems.length) {
  console.error(`JS licence gate FAILED (${problems.length}):\n` + problems.join('\n'));
  process.exit(1);
}
console.log(`JS licence gate ok: ${count} packages in ${lockfiles.length} lockfile(s)`);
