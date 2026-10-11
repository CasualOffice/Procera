// Seeds a local demo (scripts/demo.sh): a system admin, the "Acme" workspace with an owner, people,
// departments and a few library documents. Prints the sign-in details. Needs an `e2e` build.
import { createHmac } from 'node:crypto';

const API = process.env.API ?? 'http://127.0.0.1:18500';
const PW = 'correct horse battery staple';
const CSRF = { 'x-requested-with': 'procera' };

function b32(s) {
  const a = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = '';
  for (const c of s.replace(/=+$/, '').toUpperCase()) bits += a.indexOf(c).toString(2).padStart(5, '0');
  const out = [];
  for (let i = 0; i + 8 <= bits.length; i += 8) out.push(parseInt(bits.slice(i, i + 8), 2));
  return Buffer.from(out);
}
function totp(secret, at, offset = 0) {
  const c = Buffer.alloc(8);
  c.writeBigUInt64BE(BigInt(Math.floor(at / 30000) + offset));
  const h = createHmac('sha1', b32(secret)).update(c).digest();
  const o = h[19] & 15;
  return String((((h[o] & 127) << 24) | (h[o + 1] << 16) | (h[o + 2] << 8) | h[o + 3]) % 1e6).padStart(6, '0');
}

async function call(method, path, { token, body, headers } = {}) {
  const res = await fetch(API + path, {
    method,
    headers: { accept: 'application/json', ...(body ? { 'content-type': 'application/json' } : {}), ...(token ? { authorization: `Bearer ${token}` } : {}), ...headers },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  const json = text ? JSON.parse(text) : undefined;
  if (!res.ok) throw new Error(`${method} ${path} → ${res.status} ${text}`);
  return json;
}
const now = async () => Date.parse((await call('GET', '/version')).server_time);

/** Signs in with MFA (enrolling TOTP first) and returns { token, recovery }. */
async function mfaLogin(email) {
  const pw = await call('POST', '/api/v1/auth/login', { body: { email, password: PW }, headers: CSRF });
  const enr = await call('POST', '/api/v1/me/mfa/totp', { token: pw.access_token });
  const t = await now();
  const { recovery_codes } = await call('POST', '/api/v1/me/mfa/totp/confirm', { token: pw.access_token, body: { code: totp(enr.secret, t) } });
  const step = await call('POST', '/api/v1/auth/login', { body: { email, password: PW }, headers: CSRF });
  const done = await call('POST', '/api/v1/auth/mfa', { body: { mfa_token: step.mfa_token, code: totp(enr.secret, t, 1) }, headers: CSRF });
  return { token: done.access_token, recovery: recovery_codes, secret: enr.secret };
}

const { token: boot } = await call('POST', '/__test/bootstrap-token');
await call('POST', '/sysapi/v1/bootstrap', { body: { token: boot, email: 'root@acme.test', display_name: 'Riley Root', password: PW } });
const sys = await mfaLogin('root@acme.test');

const ws = await call('POST', '/sysapi/v1/workspaces', { token: sys.token, body: { name: 'Acme', slug: 'acme', profile: 'standard', owner_email: 'olivia@acme.test' } });
// Demo convenience: nobody needs an authenticator app in the demo workspace. The platform floor is
// lowered on this instance only and the workspace setting cleared — production defaults are untouched.
await call('PUT', '/sysapi/v1/settings/auth.mfa.required_roles', { token: sys.token, body: { value: [], reason: 'local demo: password-only sign-in' } });
await call('POST', '/__test/settings', { body: { workspace: ws.id, key: 'auth.mfa.required_roles', value: [] } });
const ownerInvite = (await call('GET', `/__test/invitations/token?workspace=${ws.id}&email=olivia%40acme.test`)).token;
await call('POST', '/api/v1/invitations/register', { body: { token: ownerInvite, display_name: 'Olivia Owner', password: PW } });
const owner = await call('POST', '/api/v1/auth/login', { body: { email: 'olivia@acme.test', password: PW }, headers: CSRF });
owner.token = owner.access_token;
const sw = await call('POST', '/api/v1/session/workspace', { token: owner.token, body: { workspace_id: ws.id } });
const o = sw.access_token;

// People and directory.
const people = [
  ['ben@acme.test', 'Ben Baker', 'reviewer', 'Head of Compliance'],
  ['cara@acme.test', 'Cara Chen', 'member', 'Support lead'],
  ['dev@acme.test', 'Dev Patel', 'member', 'IT engineer'],
  ['ema@acme.test', 'Ema Rossi', 'workspace.admin', 'People partner'],
  ['finn@acme.test', 'Finn Walsh', 'member', 'Finance analyst'],
  ['gia@acme.test', 'Gia Nguyen', 'member', 'Office manager'],
];
const ids = {};
for (const [email, name, role] of people) {
  await call('POST', '/__test/users', { body: { email, display_name: name, password: PW } });
  await call('POST', '/api/v1/members/invitations', { token: o, body: { email, roles: [role] } });
  const t = (await call('GET', `/__test/invitations/token?workspace=${ws.id}&email=${encodeURIComponent(email)}`)).token;
  const login = await call('POST', '/api/v1/auth/login', { body: { email, password: PW }, headers: CSRF });
  ids[email] = (await call('POST', '/api/v1/invitations/accept', { token: login.access_token, body: { token: t } })).membership_id;
}
const ops = await call('POST', '/api/v1/departments', { token: o, body: { name: 'Operations' } });
const ppl = await call('POST', '/api/v1/departments', { token: o, body: { name: 'People & Culture' } });
const fin = await call('POST', '/api/v1/departments', { token: o, body: { name: 'Finance' } });
const dept = { 'ben@acme.test': ops, 'cara@acme.test': ops, 'dev@acme.test': ops, 'ema@acme.test': ppl, 'finn@acme.test': fin, 'gia@acme.test': ops };
for (const [email, , , title] of people) await call('PATCH', `/api/v1/members/${ids[email]}`, { token: o, body: { department_id: dept[email].id, title } });
await call('POST', '/api/v1/members/invitations', { token: o, body: { email: 'nina@acme.test', roles: ['member'] } });

// Library.
const p = (text) => ({ type: 'paragraph', text });
const h = (text, level = 2) => ({ type: 'heading', level, text });
const li = (...items) => ({ type: 'list', items });
async function create(kind, key, title, blocks, criticality = 'standard') {
  return call('POST', '/api/v1/artifacts', { token: o, body: { kind, key, title, criticality, content: { blocks } } });
}
async function publish(kind, key, title, blocks) {
  const { artifact, version } = await create(kind, key, title, blocks);
  await call('POST', `/api/v1/versions/${version.id}/submit`, { token: o });
  await call('POST', `/api/v1/versions/${version.id}/publish`, { token: o, body: {} });
  return artifact.id;
}
const coc = await publish('policy', 'code-of-conduct', 'Code of Conduct', [
  p('Our Code of Conduct describes how we work with each other, our customers and our partners. It applies to everyone at Acme.'),
  h('Respect and inclusion'), p('We treat each other with respect. Harassment, discrimination and bullying are never acceptable, in the office or online.'),
  h('Conflicts of interest'), p('Tell your manager about any outside interest that could influence your decisions at work.'),
  li('Gifts over €50 must be declared', 'Family relationships in a reporting line must be disclosed', 'Side businesses need written approval'),
  h('Speaking up'), p('Report concerns to People & Culture, or anonymously through the ethics line. We never tolerate retaliation.'),
]);
await publish('policy', 'acceptable-use', 'Acceptable Use of IT', [
  p('Company devices and accounts are for work. Keep them secure and use them responsibly.'),
  h('Your responsibilities'), li('Lock your screen when you step away', 'Never share passwords or MFA codes', 'Report lost devices within one hour'),
  h('Monitoring'), p('Security tools may log activity on company systems to protect our customers and staff.'),
]);
await publish('sop', 'laptop-onboarding', 'Laptop onboarding', [
  p('How IT prepares a laptop for a new starter.'),
  h('Before day one'), li('Order the laptop five working days before the start date', 'Enrol the device in management', 'Install the standard image and security agent'),
  h('On day one'), li('Hand over the laptop and confirm MFA enrolment', 'Walk through the Acceptable Use policy'),
]);
await publish('sop', 'expense-claims', 'Submitting expense claims', [
  p('Claim business expenses within 30 days with an itemised receipt.'), h('Steps'), li('Photograph the receipt', 'Choose the cost centre', 'Submit for manager approval'),
]);
const travel = await create('policy', 'travel', 'Travel and expenses', [p('Book economy for flights under six hours. Use preferred hotels where available.'), h('Approvals'), p('Trips over €2,000 need director approval before booking.')], 'controlled');
await call('POST', `/api/v1/versions/${travel.version.id}/submit`, { token: o });
await create('sop', 'incident-response', 'Security incident response', [p('Draft — steps for triaging a suspected security incident.')]);

// Campaign: everyone acknowledges the Code of Conduct.
const cocVersion = (await call('GET', `/api/v1/artifacts/${coc}`, { token: o })).artifact.current_published_version_id;
const camp = await call('POST', '/api/v1/campaigns', { token: o, body: { version_id: cocVersion, title: 'Annual Code of Conduct attestation', audience: { all: true } } });
await call('POST', `/api/v1/campaigns/${camp.id}/activate`, { token: o });

// Announcements (targeted by role, so no workspace-wide approval is needed).
const everyone = { roles: ['member', 'reviewer', 'workspace.admin', 'workspace.owner'] };
const a1 = await call('POST', '/api/v1/announcements', { token: o, body: { title: 'Office closed on Friday for maintenance', content: { blocks: [p('The building will be closed all day Friday while the electrical system is upgraded. Please work from home.'), p('Laptops and badges are not affected.')] }, audience: everyone, priority: 'critical', pinned: true, require_ack: true } });
await call('POST', `/api/v1/announcements/${a1.id}/publish`, { token: o });
const a2 = await call('POST', '/api/v1/announcements', { token: o, body: { title: 'Welcome Ema, our new People partner', content: { blocks: [p('Ema joins us from Lisbon and will look after onboarding and wellbeing. Say hello!')] }, audience: everyone } });
await call('POST', `/api/v1/announcements/${a2.id}/publish`, { token: o });
await call('POST', '/__test/drain');

console.log(JSON.stringify({ owner: 'olivia@acme.test', password: PW, note: 'Password only — the demo workspace does not require two-step verification.', system_admin: 'root@acme.test', system_recovery_codes: sys.recovery, others: people.map((x) => x[0]), reviewer: 'ben@acme.test' }, null, 2));
