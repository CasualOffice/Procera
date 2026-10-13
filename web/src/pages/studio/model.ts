/*
 * The Guided Studio's model (BPMS-100, docs/05 §3.5): a process version's content
 * (`{ir, studio}`, docs/08 "Processes") read into a linear list of steps and form fields the owner
 * edits, and written back as valid IR v1 (docs/11). Pure functions only, no React and no I/O, so the
 * rules are easy to read and test.
 *
 * The shape the studio edits is the one the built-ins use (crates/requests/builtin): start → steps →
 * "Approved", where every approval also goes to "Rejected" and sends back to a `revise` task for the
 * requester, which returns to the first step. A step may be "only when" a condition holds, which is
 * a `decision` before it whose default skips it. Anything else (parallel blocks, notifications,
 * custom outcomes) is shown read-only: the studio never rewrites what it can't represent.
 */

type Json = Record<string, unknown>;
export type I18n = Record<string, string>;

export interface IrVariable { name: string; type: unknown; classification?: string; required?: boolean; [k: string]: unknown }
export interface IrCandidate { type: string; ref: string }
export interface IrAssignment { candidates: IrCandidate[]; strategy?: string; excludeInitiator?: boolean; excludeActorsOf?: string[]; fallback?: IrCandidate; [k: string]: unknown }
export interface IrEscalation { at: string; action: 'remind' | 'notify' | 'reassign' | 'outcome'; to?: IrCandidate; outcome?: string; reason?: string }
export interface IrNode {
  id: string; kind: string; name?: I18n; outcomes?: string[]; assignment?: IrAssignment; due?: { after: string; business?: boolean };
  escalations?: IrEscalation[]; branches?: { name: string; when?: string }[]; subject?: string; form?: string; inputForm?: string; [k: string]: unknown;
}
export interface IrEdge { id: string; from: string; to: string; on?: string }
export interface IrRefs { forms?: Record<string, string>; calendars?: Record<string, string>; documents?: Record<string, string>; [k: string]: unknown }
export interface IrDocument {
  schemaVersion: number; process: { key: string; version: number; name: I18n; criticality?: string }; variables: IrVariable[];
  nodes: IrNode[]; edges: IrEdge[]; sla?: Json[]; refs?: IrRefs; meta?: Json;
}

export type Category = 'hr' | 'finance' | 'it' | 'operations' | 'other';
export const CATEGORIES: { id: Category; label: string }[] = [
  { id: 'hr', label: 'HR' }, { id: 'finance', label: 'Finance' }, { id: 'it', label: 'IT' }, { id: 'operations', label: 'Operations' }, { id: 'other', label: 'Other' },
];
export interface StudioRequest { name: string; description: string; category: Category; prefix: string }
export interface Content { ir: IrDocument; studio: { request: StudioRequest; fields: Record<string, { label: string; help?: string }>; [k: string]: unknown } }

/* ------------------------------------------------------------------ fields */

export type FieldKind = 'string' | 'text' | 'number' | 'money' | 'date' | 'boolean' | 'enum' | 'principal' | 'files' | 'other';
export const FIELD_KINDS: { id: Exclude<FieldKind, 'other'>; label: string; hint: string }[] = [
  { id: 'string', label: 'Short text', hint: 'One line, like a model name' },
  { id: 'text', label: 'Long text', hint: 'A paragraph, like a reason' },
  { id: 'number', label: 'Number', hint: 'A quantity' },
  { id: 'money', label: 'Money', hint: 'An amount in one currency' },
  { id: 'date', label: 'Date', hint: 'A calendar day' },
  { id: 'boolean', label: 'Yes / no', hint: 'A tick box' },
  { id: 'enum', label: 'Choice', hint: 'One of a few options' },
  { id: 'principal', label: 'Person', hint: 'Someone in this workspace' },
  { id: 'files', label: 'Files', hint: 'Attachments' },
];
export const CURRENCIES = ['EUR', 'USD', 'GBP', 'INR', 'CHF', 'JPY', 'AUD', 'CAD', 'SGD'];

export interface Field {
  name: string; label: string; help: string; kind: FieldKind; required: boolean;
  /** Choice values (shown as written). */
  options: string[];
  currency: string;
  /** The variable as read, so classification, readers and unknown types survive a round trip. */
  base: IrVariable;
}

function kindOf(type: unknown): { kind: FieldKind; options: string[]; currency: string } {
  const none = { options: [] as string[], currency: 'EUR' };
  if (typeof type === 'string') {
    const map: Record<string, FieldKind> = { string: 'string', text: 'text', integer: 'number', decimal: 'number', date: 'date', boolean: 'boolean', principal: 'principal', file: 'files' };
    return { ...none, kind: map[type] ?? 'other' };
  }
  const t = (type ?? {}) as Json;
  if (Array.isArray(t.enum)) return { ...none, kind: 'enum', options: (t.enum as unknown[]).map(String) };
  if (t.money && typeof t.money === 'object') return { ...none, kind: 'money', currency: String((t.money as Json).currency ?? 'EUR') };
  if (t.list === 'file') return { ...none, kind: 'files' };
  return { ...none, kind: 'other' };
}

function typeOf(f: Field): unknown {
  switch (f.kind) {
    case 'number': return f.base.type === 'integer' || f.base.type === 'decimal' ? f.base.type : 'decimal';
    case 'money': return { money: { currency: f.currency || 'EUR' } };
    case 'enum': return { enum: f.options.map((o) => o.trim()).filter(Boolean) };
    case 'files': return f.base.type === 'file' ? 'file' : { list: 'file' };
    case 'other': return f.base.type;
    default: return f.kind;
  }
}

const RESERVED = new Set(['initiator', 'instance', 'as', 'break', 'const', 'continue', 'else', 'false', 'for', 'function', 'if', 'import', 'in', 'let', 'loop', 'package', 'namespace', 'null', 'return', 'true', 'var', 'void', 'while', 'start', 'revise']);

/** `Manager approval` → `manager_approval`: lower-case letters, digits and underscores, starting with a letter. */
export function slug(text: string, fallback: string): string {
  const s = text.toLowerCase().normalize('NFKD').replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '').slice(0, 40).replace(/_+$/, '');
  if (!s) return fallback;
  return /^[a-z]/.test(s) ? s : `${fallback}_${s}`;
}

/** `base`, or `base_2`, `base_3`… — whichever is not taken. */
export function unique(base: string, taken: Set<string>): string {
  if (!taken.has(base) && !RESERVED.has(base)) return base;
  for (let i = 2; ; i++) if (!taken.has(`${base}_${i}`)) return `${base}_${i}`;
}

/* ------------------------------------------------------------------- steps */

export type StepKind = 'approval' | 'task' | 'acknowledge';
export const STEP_KINDS: { id: StepKind; label: string; irKind: string; hint: string }[] = [
  { id: 'approval', label: 'Approval', irKind: 'human.approval', hint: 'Someone approves, rejects or sends it back' },
  { id: 'task', label: 'Task', irKind: 'human.task', hint: 'Someone does something and marks it done' },
  { id: 'acknowledge', label: 'Acknowledgement', irKind: 'human.acknowledge', hint: 'Someone confirms they have read a document' },
];
const IR_KIND: Record<StepKind, string> = { approval: 'human.approval', task: 'human.task', acknowledge: 'human.acknowledge' };

export type Who =
  | { type: 'manager' } | { type: 'skip_manager' } | { type: 'requester' }
  | { type: 'group'; name: string } | { type: 'role'; key: string } | { type: 'person'; email: string } | { type: 'position'; title: string }
  | { type: 'custom'; candidate: IrCandidate };
export type WhoType = Who['type'];
export const WHO_TYPES: { id: Exclude<WhoType, 'custom'>; label: string }[] = [
  { id: 'manager', label: 'The requester’s manager' },
  { id: 'skip_manager', label: 'Their manager’s manager' },
  { id: 'requester', label: 'The requester' },
  { id: 'group', label: 'Anyone in a group' },
  { id: 'role', label: 'Anyone with a role' },
  { id: 'position', label: 'Whoever holds a position' },
  { id: 'person', label: 'A specific person' },
];

const MANAGER = 'expr:cel: initiator.manager';
const SKIP = 'expr:cel: initiator.manager.manager';
const REQUESTER = 'expr:cel: initiator';
export const OWNER_FALLBACK: IrCandidate = { type: 'role', ref: 'process.owner' };

export function whoFrom(c: IrCandidate): Who {
  if (c.type === 'user' && c.ref === MANAGER) return { type: 'manager' };
  if (c.type === 'user' && c.ref === SKIP) return { type: 'skip_manager' };
  if (c.type === 'user' && c.ref === REQUESTER) return { type: 'requester' };
  if (c.ref.startsWith('expr:')) return { type: 'custom', candidate: c };
  if (c.type === 'group') return { type: 'group', name: c.ref };
  if (c.type === 'role') return { type: 'role', key: c.ref };
  if (c.type === 'user') return { type: 'person', email: c.ref };
  if (c.type === 'position') return { type: 'position', title: c.ref };
  return { type: 'custom', candidate: c };
}

export function candidateOf(w: Who): IrCandidate {
  switch (w.type) {
    case 'manager': return { type: 'user', ref: MANAGER };
    case 'skip_manager': return { type: 'user', ref: SKIP };
    case 'requester': return { type: 'user', ref: REQUESTER };
    case 'group': return { type: 'group', ref: w.name.trim() };
    case 'role': return { type: 'role', ref: w.key };
    case 'person': return { type: 'user', ref: w.email.trim().toLowerCase() };
    case 'position': return { type: 'position', ref: w.title.trim() };
    case 'custom': return w.candidate;
  }
}

export function emptyWho(type: WhoType): Who {
  switch (type) {
    case 'group': return { type, name: '' };
    case 'role': return { type, key: '' };
    case 'person': return { type, email: '' };
    case 'position': return { type, title: '' };
    case 'custom': return { type, candidate: { type: 'user', ref: REQUESTER } };
    default: return { type };
  }
}

/** "Only when …": a number or money field over a value, or a field equal to a value. */
export type Condition = { field: string; op: 'over' | 'equals'; value: string } | { raw: string };

export type Escalate = { days: number; action: 'notify' | 'reassign'; reason?: string };

export interface Step {
  id: string; kind: StepKind; name: string;
  who: Who[];
  /** Due after this many business days; null for no deadline (or `dueRaw` when the template's isn't in days). */
  dueDays: number | null;
  dueRaw: { after: string; business?: boolean } | null;
  /** Reminders, in days after the step arrives. */
  reminders: number[];
  /** "If still open after N days, tell / move it to their manager's manager." */
  escalate: Escalate | null;
  /** Escalations the studio doesn't edit, kept as they are. */
  otherEscalations: IrEscalation[];
  when: Condition | null;
  /** The decision node a template wrote for the condition, so its id and name stay. */
  whenBase?: IrNode;
  /** Acknowledgements: what is confirmed, as a library reference `policy:key@3`, and its title. */
  subject: { ref: string; title: string } | null;
  /** Created in this editing session: its id still follows its name. */
  fresh?: boolean;
  /** A key for the screen that never changes, even while a fresh step's id follows its name. */
  uid: string;
  /** The node as read, so fields the studio doesn't edit (quorum, signature, SoD) survive. */
  base: IrNode;
}

export interface Model {
  request: StudioRequest;
  fields: Field[];
  steps: Step[];
  /** The document read, for what passes through (process, refs, SLA, meta, start and end nodes). */
  base: IrDocument;
  ends: { approved: IrNode; rejected: IrNode; revise: IrNode | null };
  /** Why the studio can't edit this version (it shows it read-only), or null. */
  readOnly: string | null;
  /** Studio members this editor doesn't edit (e.g. an acknowledgement's statement), kept as they are. */
  studioExtra: Record<string, unknown>;
}

const days = (iso: string | undefined): number | null => {
  const m = /^P(?:(\d+)W)?(?:(\d+)D)?$/.exec(iso ?? '');
  if (!m || (!m[1] && !m[2])) return null;
  return Number(m[1] ?? 0) * 7 + Number(m[2] ?? 0);
};
const iso = (n: number) => `P${n}D`;

const numberLit = /^\d{1,15}(\.\d{1,10})?$/;
/** CEL for a condition, null when it isn't complete yet. */
export function conditionExpr(c: Condition, fields: Field[]): string | null {
  if ('raw' in c) return c.raw;
  const f = fields.find((x) => x.name === c.field);
  if (!f) return null;
  const guard = (body: string) => (f.required ? body : `${f.name} != null && ${body}`);
  if (c.op === 'over') {
    if (!numberLit.test(c.value.trim())) return null;
    if (f.kind === 'money') return `expr:cel: ${guard(`${f.name}.amount > ${c.value.trim()}`)}`;
    if (f.kind === 'number') return `expr:cel: ${guard(`${f.name} > ${c.value.trim()}`)}`;
    return null;
  }
  if (f.kind === 'boolean') return `expr:cel: ${f.name} == ${c.value === 'yes' ? 'true' : 'false'}`;
  if (!c.value) return null;
  return `expr:cel: ${f.name} == ${JSON.stringify(c.value)}`;
}

/** Reads back what `conditionExpr` writes; anything else stays as written. */
export function parseCondition(expr: string): Condition {
  const body = expr.replace(/^expr:cel:\s*/, '');
  const unguard = (s: string) => s.replace(/^(\w+) != null && /, '');
  const b = unguard(body);
  let m = /^(\w+)(?:\.amount)? > (\d+(?:\.\d+)?)$/.exec(b);
  if (m) return { field: m[1]!, op: 'over', value: m[2]! };
  m = /^(\w+) == (true|false)$/.exec(b);
  if (m) return { field: m[1]!, op: 'equals', value: m[2] === 'true' ? 'yes' : 'no' };
  m = /^(\w+) == ("(?:[^"\\]|\\.)*")$/.exec(b);
  if (m) { try { return { field: m[1]!, op: 'equals', value: JSON.parse(m[2]!) as string }; } catch { /* kept raw */ } }
  return { raw: expr };
}

/** "Amount over 5000", "Model is MacBook Pro". */
export function describeCondition(c: Condition, fields: Field[]): string {
  if ('raw' in c) return 'a custom condition';
  const f = fields.find((x) => x.name === c.field);
  const label = f?.label ?? c.field;
  if (c.op === 'over') return `${label} is over ${c.value || '…'}${f?.kind === 'money' ? ` ${f.currency}` : ''}`;
  if (f?.kind === 'boolean') return `${label} is ${c.value === 'yes' ? 'yes' : 'no'}`;
  return `${label} is ${c.value || '…'}`;
}

/* -------------------------------------------------------------- reading */

const BLANK_REQUEST: StudioRequest = { name: '', description: '', category: 'other', prefix: '' };

function stepFrom(node: IrNode, kind: StepKind, when: Condition | null, docRefs: IrRefs | undefined): Step {
  const reminders: number[] = [];
  let escalate: Escalate | null = null;
  const other: IrEscalation[] = [];
  for (const e of node.escalations ?? []) {
    const d = days(e.at);
    if (e.action === 'remind' && !e.to && d !== null) reminders.push(d);
    else if ((e.action === 'notify' || e.action === 'reassign') && e.to?.ref === SKIP && d !== null && !escalate) escalate = { days: d, action: e.action, ...(e.reason ? { reason: e.reason } : {}) };
    else other.push(e);
  }
  const dueDays = node.due && node.due.business !== false ? days(node.due.after) : null;
  const subjectRef = node.subject ? (docRefs?.documents?.[node.subject] ?? (docRefs as Json | undefined)?.[node.subject] as string | undefined) : undefined;
  return {
    id: node.id, uid: node.id, kind, name: node.name?.en ?? Object.values(node.name ?? {})[0] ?? node.id,
    who: (node.assignment?.candidates ?? []).map(whoFrom),
    dueDays, dueRaw: node.due && dueDays === null ? node.due : null,
    reminders: reminders.sort((a, b) => a - b), escalate, otherEscalations: other, when,
    subject: node.subject ? { ref: typeof subjectRef === 'string' ? subjectRef : node.subject, title: node.subject } : null,
    base: node,
  };
}

/** The steps from start to "Approved", or why the studio can't show this flow. */
function readFlow(ir: IrDocument): { steps: Step[]; approved?: IrNode; rejected?: IrNode; revise?: IrNode } | { unsupported: string } {
  const byId = new Map(ir.nodes.map((n) => [n.id, n]));
  const out = (id: string) => ir.edges.filter((e) => e.from === id);
  const to = (id: string, on: string) => out(id).find((e) => (e.on ?? 'next') === on)?.to;
  const start = ir.nodes.find((n) => n.kind === 'start');
  if (!start) return { unsupported: 'It has no start.' };
  const steps: Step[] = [];
  const used = new Set([start.id]);
  let approved: IrNode | undefined, rejected: IrNode | undefined, revise: IrNode | undefined;
  let cur = to(start.id, 'next');
  while (cur) {
    let node = byId.get(cur);
    if (!node) return { unsupported: `A connection leads to “${cur}”, which doesn’t exist.` };
    if (node.kind === 'end') { approved = node; used.add(node.id); break; }
    if (used.has(node.id)) return { unsupported: 'Its steps go round in a loop.' };
    used.add(node.id);
    let when: Condition | null = null;
    let whenBase: IrNode | undefined;
    if (node.kind === 'decision') {
      whenBase = node;
      const b = node.branches ?? [];
      const target = b.length === 1 && b[0]!.when ? to(node.id, b[0]!.name) : undefined;
      if (!target || !to(node.id, 'default')) return { unsupported: `“${node.name?.en ?? node.id}” chooses between several paths.` };
      when = parseCondition(b[0]!.when!);
      node = byId.get(target);
      if (!node || used.has(node.id)) return { unsupported: 'A condition leads nowhere the studio can show.' };
      used.add(node.id);
    }
    const kind = STEP_KINDS.find((k) => k.irKind === node!.kind)?.id;
    if (!kind) return { unsupported: `“${node.name?.en ?? node.id}” is a kind of step the studio can’t edit yet (${node.kind}).` };
    let next: string | undefined;
    if (kind === 'approval') {
      if (node.outcomes?.length) return { unsupported: `“${node.name?.en ?? node.id}” has its own outcomes.` };
      next = to(node.id, 'approved');
      const r = to(node.id, 'rejected');
      const back = to(node.id, 'changes_requested');
      const rn = r ? byId.get(r) : undefined;
      if (!rn || rn.kind !== 'end' || (rejected && rejected.id !== rn.id)) return { unsupported: 'A rejection doesn’t end the request.' };
      rejected = rn;
      const bn = back ? byId.get(back) : undefined;
      if (!bn || bn.kind !== 'human.task' || bn.assignment?.candidates?.[0]?.ref !== REQUESTER || (revise && revise.id !== bn.id)) return { unsupported: 'Sending back doesn’t return to the requester.' };
      revise = bn;
    } else if (kind === 'task') {
      const targets = new Set(out(node.id).map((e) => e.to));
      if (targets.size !== 1) return { unsupported: `“${node.name?.en ?? node.id}” leads to different places.` };
      next = [...targets][0];
    } else {
      next = to(node.id, 'acknowledged');
      const exp = to(node.id, 'expired');
      if (exp && exp !== next) return { unsupported: `“${node.name?.en ?? node.id}” goes somewhere else when it expires.` };
    }
    steps.push({ ...stepFrom(node, kind, when, ir.refs), ...(whenBase ? { whenBase } : {}) });
    cur = next;
  }
  for (const x of [rejected, revise]) if (x) used.add(x.id);
  // Ends nothing leads to yet (a blank template's) are adopted, or dropped when the flow is written.
  const spareEnds = ir.nodes.filter((n) => n.kind === 'end' && !used.has(n.id));
  approved ??= spareEnds.find((n) => !/reject/i.test(n.id)) ?? undefined;
  rejected ??= spareEnds.find((n) => /reject/i.test(n.id) && n !== approved);
  const left = ir.nodes.filter((n) => !used.has(n.id) && n.kind !== 'end');
  if (left.length) return { unsupported: `It has parts the studio can’t show yet (${left.map((n) => n.name?.en ?? n.id).join(', ')}).` };
  return { steps, approved, rejected, revise };
}

const endNode = (id: string, name: string): IrNode => ({ id, kind: 'end', name: { en: name }, result: 'completed' });

/** Reads a process version's content into the studio's model. */
export function fromContent(content: Partial<Content> | null | undefined, fallbackName = ''): Model {
  const ir: IrDocument = content?.ir ?? { schemaVersion: 1, process: { key: '', version: 1, name: { en: fallbackName } }, variables: [], nodes: [{ id: 'start', kind: 'start', name: { en: 'Submit request' }, trigger: 'manual' }], edges: [] };
  const studio = content?.studio;
  const labels = studio?.fields ?? {};
  const fields: Field[] = (ir.variables ?? []).map((v) => {
    const k = kindOf(v.type);
    return { name: v.name, label: labels[v.name]?.label ?? v.name.replace(/_/g, ' ').replace(/^./, (c) => c.toUpperCase()), help: labels[v.name]?.help ?? '', kind: k.kind, required: v.required === true, options: k.options, currency: k.currency, base: v };
  });
  const request: StudioRequest = { ...BLANK_REQUEST, name: ir.process.name?.en ?? fallbackName, ...(studio?.request ?? {}) };
  // A built-in's category arrives as its display name ("HR"); the studio's are ids.
  const cat = String(request.category ?? '').toLowerCase();
  request.category = CATEGORIES.find((c) => c.id === cat || c.label.toLowerCase() === cat)?.id ?? (cat === 'people' ? 'hr' : 'other');
  const { request: _r, fields: _f, ...studioExtra } = (studio ?? {}) as Json;
  const flow = readFlow(ir);
  if ('unsupported' in flow) {
    return { request, fields, steps: [], base: ir, ends: { approved: endNode('end_approved', 'Approved'), rejected: endNode('end_rejected', 'Rejected'), revise: null }, readOnly: flow.unsupported, studioExtra };
  }
  return {
    request, fields, steps: flow.steps, base: ir,
    ends: { approved: flow.approved ?? endNode('end_approved', 'Approved'), rejected: flow.rejected ?? endNode('end_rejected', 'Rejected'), revise: flow.revise ?? null },
    readOnly: null, studioExtra,
  };
}

/* -------------------------------------------------------------- writing */

const decisionId = (s: Step) => s.whenBase?.id ?? `${s.id}_when`;

/** The condition's decision node: the template's own (id, name, branch name) while its condition is unchanged. */
function decisionNode(s: Step, expr: string, fields: Field[]): IrNode {
  const b = s.whenBase;
  const same = b?.branches?.length === 1 && b.branches[0]!.when === expr;
  if (b && same) return b;
  return { ...(b ?? {}), id: decisionId(s), kind: 'decision', name: { en: `Only when ${describeCondition(s.when!, fields)}` }, branches: [{ name: b?.branches?.[0]?.name ?? 'applies', when: expr }] };
}

/** The node a step is entered by: its condition when it has one. */
const entryOf = (s: Step, fields: Field[]) => (s.when && conditionExpr(s.when, fields) ? decisionId(s) : s.id);

function stepNode(s: Step, ids: Set<string>, refKey: string | null): IrNode {
  const n: IrNode = { ...s.base, id: s.id, kind: IR_KIND[s.kind], name: { ...(s.base.name ?? {}), en: s.name.trim() || 'Untitled step' } };
  // Fields that belong to another kind go when a step changes kind.
  for (const k of ['outcomes', 'quorum', 'subject', 'excludeInitiator'] as const) delete n[k];
  const a = s.base.assignment;
  const exclude = (a?.excludeActorsOf ?? []).filter((x) => ids.has(x) && x !== s.id);
  n.assignment = {
    ...(a ?? {}),
    candidates: s.who.map(candidateOf),
    strategy: a?.strategy ?? 'claim',
    fallback: a?.fallback ?? OWNER_FALLBACK,
  };
  if (exclude.length) n.assignment.excludeActorsOf = exclude; else delete n.assignment.excludeActorsOf;
  if (s.kind === 'approval') {
    n.assignment.excludeInitiator = a?.excludeInitiator ?? true;
    n.quorum = s.base.quorum ?? 'one';
  } else delete n.assignment.excludeInitiator;
  if (s.kind === 'task') n.outcomes = s.base.kind === 'human.task' && s.base.outcomes?.length ? s.base.outcomes : ['done'];
  if (s.kind === 'acknowledge' && refKey) n.subject = refKey;
  if (s.dueDays !== null) n.due = { after: iso(s.dueDays), business: true };
  else if (s.dueRaw) n.due = s.dueRaw;
  else delete n.due;
  const esc: IrEscalation[] = [
    ...s.reminders.map((d): IrEscalation => ({ at: iso(d), action: 'remind' })),
    ...(s.escalate ? [{ at: iso(s.escalate.days), action: s.escalate.action, to: { type: 'user', ref: SKIP }, reason: s.escalate.reason ?? 'still open' } as IrEscalation] : []),
    ...s.otherEscalations,
  ];
  if (esc.length) n.escalations = esc; else delete n.escalations;
  return n;
}

/** The outcomes of a step and where each goes, for the step builder and the properties panel. */
export function routes(m: Model, s: Step): { outcome: string; label: string; to: string }[] {
  const i = m.steps.findIndex((x) => x.id === s.id);
  const after = m.steps[i + 1];
  const next = after ? `Next: ${after.name || 'Untitled step'}${after.when ? ' (when it applies)' : ''}` : 'Approved';
  if (s.kind === 'approval') return [
    { outcome: 'approved', label: 'Approve', to: next },
    { outcome: 'rejected', label: 'Reject', to: 'Rejected' },
    { outcome: 'changes_requested', label: 'Send back', to: 'Back to the requester' },
  ];
  if (s.kind === 'task') return (s.base.kind === 'human.task' && s.base.outcomes?.length ? s.base.outcomes : ['done']).map((o) => ({ outcome: o, label: o === 'done' ? 'Mark done' : o.replace(/_/g, ' '), to: next }));
  return [{ outcome: 'acknowledged', label: 'Acknowledge', to: next }];
}

/** The content to save: IR v1 for this artifact's key and draft version, plus the studio's wording. */
export function toContent(m: Model, key: string, version: number): Content {
  if (m.readOnly) {
    return { ir: { ...m.base, process: { ...m.base.process, key, version } }, studio: { ...m.studioExtra, request: m.request, fields: studioFields(m.fields) } };
  }
  const ir = m.base;
  const start: IrNode = ir.nodes.find((n) => n.kind === 'start') ?? { id: 'start', kind: 'start', name: { en: 'Submit request' }, trigger: 'manual' };
  const anyApproval = m.steps.some((s) => s.kind === 'approval');
  const ids = new Set(m.steps.map((s) => s.id));
  const nodes: IrNode[] = [start];
  const edges: IrEdge[] = [];
  const edge = (from: string, to: string, on?: string) => edges.push({ id: `e${edges.length + 1}`, from, to, ...(on && on !== 'next' ? { on } : {}) });
  const refs: IrRefs = JSON.parse(JSON.stringify(ir.refs ?? {})) as IrRefs;
  const documents: Record<string, string> = Object.fromEntries(Object.entries(refs.documents ?? {}).filter(([k]) => !k.startsWith('ack_')));

  const entries = m.steps.map((s) => entryOf(s, m.fields));
  const approved = m.ends.approved;
  const next = (i: number) => entries[i + 1] ?? approved.id;
  edge(start.id, entries[0] ?? approved.id);
  m.steps.forEach((s, i) => {
    const expr = s.when ? conditionExpr(s.when, m.fields) : null;
    if (expr) {
      const d = decisionNode(s, expr, m.fields);
      nodes.push(d);
      edge(d.id, s.id, d.branches![0]!.name);
      edge(decisionId(s), next(i), 'default');
    }
    let refKey: string | null = null;
    if (s.kind === 'acknowledge' && s.subject) {
      // A template's own subject keeps its key; one chosen here is filed under `ack_<step>`.
      const existing = Object.entries(refs.documents ?? {}).find(([k, v]) => !k.startsWith('ack_') && v === s.subject!.ref);
      refKey = existing ? existing[0] : `ack_${s.id}`;
      documents[refKey] = s.subject.ref;
    }
    nodes.push(stepNode(s, ids, refKey));
    if (s.kind === 'approval') {
      edge(s.id, next(i), 'approved');
      edge(s.id, m.ends.rejected.id, 'rejected');
      edge(s.id, m.ends.revise?.id ?? 'revise', 'changes_requested');
    } else if (s.kind === 'task') {
      for (const o of (nodes[nodes.length - 1]!.outcomes ?? ['done'])) edge(s.id, next(i), o);
    } else {
      edge(s.id, next(i), 'acknowledged');
      edge(s.id, next(i), 'expired');
    }
  });
  if (anyApproval) {
    const revise: IrNode = m.ends.revise ?? {
      id: 'revise', kind: 'human.task', name: { en: 'Revise request' },
      ...(typeof start.inputForm === 'string' ? { form: start.inputForm } : {}),
      assignment: { candidates: [{ type: 'user', ref: REQUESTER }] }, maxIterations: 5,
    };
    nodes.push(revise);
    for (const o of revise.outcomes?.length ? revise.outcomes : ['submitted']) edge(revise.id, entries[0] ?? approved.id, o);
  }
  nodes.push(approved);
  if (anyApproval) nodes.push(m.ends.rejected);

  if (Object.keys(documents).length) refs.documents = documents; else delete refs.documents;
  // Business-day deadlines need a calendar (IR-E027).
  if (nodes.some((n) => n.due?.business)) refs.calendars = { default: 'calendar:default@1', ...(refs.calendars ?? {}) };
  const live = new Set(nodes.map((n) => n.id));
  const sla = (ir.sla ?? []).filter((x) => x.scope === 'instance' || live.has(String(x.scope).replace(/^node:/, '')))
    .map((x) => (Array.isArray(x.pauseWhile) ? { ...x, pauseWhile: (x.pauseWhile as string[]).filter((p) => live.has(p)) } : x))
    .map((x) => (Array.isArray(x.pauseWhile) && !(x.pauseWhile as string[]).length ? (({ pauseWhile: _, ...rest }) => rest)(x) : x));

  const doc: IrDocument = {
    schemaVersion: 1,
    process: { ...ir.process, key, version, name: { ...(ir.process.name ?? {}), en: m.request.name.trim() || ir.process.name?.en || key } },
    variables: m.fields.map(toVariable),
    nodes, edges,
    ...(sla.length ? { sla } : {}),
    ...(Object.keys(refs).length ? { refs } : {}),
    ...(ir.meta && Object.keys(ir.meta).length ? { meta: ir.meta } : {}),
  };
  return { ir: doc, studio: { ...m.studioExtra, request: m.request, fields: studioFields(m.fields) } };
}

function toVariable(f: Field): IrVariable {
  const v: IrVariable = { ...f.base, name: f.name, type: typeOf(f), classification: f.base.classification ?? 'internal' };
  if (f.required) v.required = true; else delete v.required;
  return v;
}

function studioFields(fields: Field[]): Content['studio']['fields'] {
  return Object.fromEntries(fields.map((f) => [f.name, f.help.trim() ? { label: f.label.trim() || f.name, help: f.help.trim() } : { label: f.label.trim() || f.name }]));
}

/* ------------------------------------------------------------ editing */

const allIds = (m: Model, except?: string) => new Set([
  ...m.base.nodes.filter((n) => n.kind === 'start' || n.kind === 'end').map((n) => n.id),
  m.ends.approved.id, m.ends.rejected.id, m.ends.revise?.id ?? 'revise',
  ...m.steps.filter((s) => s.id !== except).flatMap((s) => [s.id, decisionId(s)]),
]);

const DEFAULTS: Record<StepKind, { name: string; who: Who[]; dueDays: number | null; reminders: number[] }> = {
  approval: { name: 'Approval', who: [{ type: 'manager' }], dueDays: 2, reminders: [1] },
  task: { name: 'Task', who: [{ type: 'manager' }], dueDays: 3, reminders: [] },
  acknowledge: { name: 'Acknowledgement', who: [{ type: 'requester' }], dueDays: 5, reminders: [] },
};

/** Inserts a new step after index `after` (-1 = first). Returns the model and the new step's id. */
export function addStep(m: Model, after: number, kind: StepKind, name?: string): [Model, string] {
  const d = DEFAULTS[kind];
  const label = name?.trim() || d.name;
  const id = unique(slug(label, 'step'), allIds(m));
  const step: Step = {
    id, kind, name: label, who: d.who, dueDays: d.dueDays, dueRaw: null, reminders: d.reminders, escalate: null, otherEscalations: [], when: null,
    subject: null, fresh: true, uid: `new-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`, base: { id, kind: IR_KIND[kind] },
  };
  const steps = [...m.steps];
  steps.splice(after + 1, 0, step);
  return [{ ...m, steps }, id];
}

export function moveStep(m: Model, id: string, by: -1 | 1): Model {
  const i = m.steps.findIndex((s) => s.id === id);
  const j = i + by;
  if (i < 0 || j < 0 || j >= m.steps.length) return m;
  const steps = [...m.steps];
  [steps[i], steps[j]] = [steps[j]!, steps[i]!];
  return { ...m, steps };
}

export function removeStep(m: Model, id: string): Model {
  return { ...m, steps: m.steps.filter((s) => s.id !== id) };
}

/** Changes a step. A step made in this session keeps an id that follows its name. */
export function updateStep(m: Model, id: string, patch: Partial<Omit<Step, 'id' | 'base'>>): [Model, string] {
  let newId = id;
  const steps = m.steps.map((s) => {
    if (s.id !== id) return s;
    const next = { ...s, ...patch };
    if (patch.kind && patch.kind !== s.kind && patch.kind !== 'acknowledge') next.subject = null;
    if (s.fresh && patch.name !== undefined) {
      newId = unique(slug(patch.name, 'step'), allIds(m, id));
      next.id = newId;
      next.base = { ...next.base, id: newId };
    }
    return next;
  });
  return [{ ...m, steps }, newId];
}

export const setWho = (m: Model, id: string, who: Who[]) => updateStep(m, id, { who })[0];
export const setDue = (m: Model, id: string, dueDays: number | null) => updateStep(m, id, { dueDays, dueRaw: null })[0];
export const setReminders = (m: Model, id: string, reminders: number[]) => updateStep(m, id, { reminders: [...new Set(reminders)].sort((a, b) => a - b) })[0];
export const setEscalate = (m: Model, id: string, escalate: Escalate | null) => updateStep(m, id, { escalate })[0];
export const setCondition = (m: Model, id: string, when: Condition | null) => updateStep(m, id, { when })[0];

export function setRequest(m: Model, patch: Partial<StudioRequest>): Model {
  return { ...m, request: { ...m.request, ...patch } };
}

/** Adds a form field; its variable name comes from the label once and then stays. */
export function addField(m: Model, label: string, kind: Exclude<FieldKind, 'other'>, options: string[] = []): [Model, string] {
  const name = unique(slug(label, 'field'), new Set(m.fields.map((f) => f.name)));
  const f: Field = { name, label: label.trim(), help: '', kind, required: false, options: kind === 'enum' ? options : [], currency: 'EUR', base: { name, type: kind, classification: 'internal' } };
  return [{ ...m, fields: [...m.fields, f] }, name];
}

export function updateField(m: Model, name: string, patch: Partial<Omit<Field, 'name' | 'base'>>): Model {
  return { ...m, fields: m.fields.map((f) => (f.name === name ? { ...f, ...patch } : f)) };
}

export function moveField(m: Model, name: string, by: -1 | 1): Model {
  const i = m.fields.findIndex((f) => f.name === name);
  const j = i + by;
  if (i < 0 || j < 0 || j >= m.fields.length) return m;
  const fields = [...m.fields];
  [fields[i], fields[j]] = [fields[j]!, fields[i]!];
  return { ...m, fields };
}

/** Removes a field and any "only when" that read it. */
export function removeField(m: Model, name: string): Model {
  return {
    ...m,
    fields: m.fields.filter((f) => f.name !== name),
    steps: m.steps.map((s) => (s.when && !('raw' in s.when) && s.when.field === name ? { ...s, when: null } : s)),
  };
}

/** Fields an "only when" can read, with the comparison each allows. */
export function conditionFields(m: Model): { field: Field; op: 'over' | 'equals' }[] {
  return m.fields.flatMap((f): { field: Field; op: 'over' | 'equals' }[] => (f.kind === 'number' || f.kind === 'money' ? [{ field: f, op: 'over' }] : f.kind === 'enum' || f.kind === 'boolean' || f.kind === 'string' ? [{ field: f, op: 'equals' }] : []));
}

/* ---------------------------------------------------------- describing */

export function whoText(w: Who, roleName?: (key: string) => string | undefined): string {
  switch (w.type) {
    case 'manager': return 'The requester’s manager';
    case 'skip_manager': return 'Their manager’s manager';
    case 'requester': return 'The requester';
    case 'group': return w.name ? `Anyone in ${w.name}` : 'A group (not chosen yet)';
    case 'role': return w.key ? `Anyone with the role ${roleName?.(w.key) ?? w.key}` : 'A role (not chosen yet)';
    case 'person': return w.email || 'A person (not chosen yet)';
    case 'position': return w.title ? `The ${w.title}` : 'A position (not chosen yet)';
    case 'custom': return 'A custom rule';
  }
}

export function deadlineText(s: Step): string {
  const parts: string[] = [];
  if (s.dueDays !== null) parts.push(`Due in ${s.dueDays} business ${s.dueDays === 1 ? 'day' : 'days'}`);
  else if (s.dueRaw) parts.push('Has a deadline');
  if (s.reminders.length) parts.push(`${s.reminders.length} ${s.reminders.length === 1 ? 'reminder' : 'reminders'}`);
  if (s.escalate) parts.push(`${s.escalate.action === 'reassign' ? 'moves up' : 'escalates'} after ${s.escalate.days} ${s.escalate.days === 1 ? 'day' : 'days'}`);
  return parts.join(' · ') || 'No deadline';
}

/**
 * Which step (or field) a validation problem points at: its node (a step, or a step's condition),
 * else its pointer into the content that was checked (`/nodes/3/…`, `/variables/1/…`).
 */
export function problemTarget(m: Model, p: { node?: string | null; pointer?: string }, checked?: IrDocument): { step?: string; field?: string } {
  const byNode = (id: string | null | undefined) => (id ? m.steps.find((x) => x.id === id || decisionId(x) === id) : undefined);
  const n = /^(?:\/ir)?\/nodes\/(\d+)/.exec(p.pointer ?? '');
  const s = byNode(p.node) ?? byNode(n ? checked?.nodes[Number(n[1])]?.id : undefined);
  if (s) return { step: s.id };
  const v = /^(?:\/ir)?\/variables\/(\d+)/.exec(p.pointer ?? '');
  const name = v ? checked?.variables[Number(v[1])]?.name : undefined;
  if (name && m.fields.some((f) => f.name === name)) return { field: name };
  return {};
}
