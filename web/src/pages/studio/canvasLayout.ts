/*
 * The flow canvas's graph (BPMS-100, docs/05 §3.5): the studio's model, or any IR the studio can't
 * edit, as positioned nodes and routed edges. Pure functions, no React: the canvas only draws this.
 *
 * Units are rem, so the drawing scales with the app's root size like the rest of the UI; the canvas
 * multiplies by the root font size when it hands positions to React Flow. Node sizes are fixed per
 * kind (text that doesn't fit is truncated, with the full text in the accessible name), which keeps
 * the layout a simple top-to-bottom pass with no measuring.
 */
import { type IrDocument, type IrNode, type Model, type StepKind, STEP_KINDS, deadlineText, describeCondition, routes, whoFrom, whoText } from './model';

export type Pt = [number, number];
export type CvKind = 'start' | 'step' | 'decision' | 'end' | 'revise' | 'gateway' | 'notify' | 'other';

export interface CvNode {
  id: string; kind: CvKind; label: string; sub?: string;
  /** The editable step behind the node (a step, or the "only when" in front of it). */
  stepId?: string;
  stepKind?: StepKind; who?: string; deadline?: string;
  /** How an end ends; a gateway's direction. */
  tone?: 'done' | 'stopped' | 'split' | 'join';
  /** Top-left corner and size, in rem. */
  x: number; y: number; w: number; h: number;
}
export interface CvEdge {
  id: string; from: string; to: string; label?: string;
  /** An orthogonal polyline, rem. */
  points: Pt[];
  /** The label's top corner: its left edge (start) or its right edge (end). */
  labelAt?: Pt; labelAlign?: 'start' | 'end';
}
export interface CvGraph { nodes: CvNode[]; edges: CvEdge[] }

const SIZE: Record<CvKind, [number, number]> = {
  start: [18, 3.75], step: [18, 7], decision: [18, 2.75], end: [12, 3], revise: [13, 4], gateway: [14, 2.75], notify: [16, 4.5], other: [16, 4.5],
};
/** Vertical room between rows: an edge label and its bend fit in it. */
const GAP = 4;
const LABEL_H = 1.35;

const sized = (n: Omit<CvNode, 'x' | 'y' | 'w' | 'h'>): CvNode => ({ ...n, x: 0, y: 0, w: SIZE[n.kind][0], h: SIZE[n.kind][1] });
const cx = (n: CvNode) => n.x + n.w / 2;
const cy = (n: CvNode) => n.y + n.h / 2;
const bottom = (n: CvNode) => n.y + n.h;

/** A straight drop from the bottom of `a` into the top of `b` (same column), labelled under `a`. */
function drop(id: string, a: CvNode, b: CvNode, label?: string): CvEdge {
  const x = cx(a);
  return { id, from: a.id, to: b.id, points: [[x, bottom(a)], [x, b.y]], ...(label ? { label, labelAt: [x + 0.5, bottom(a) + 0.4] as Pt, labelAlign: 'start' as const } : {}) };
}

/**
 * The editable model: "Request submitted", each step (with its "only when" diamond), "Approved"
 * in one column; "Back to the requester" on the left and "Rejected" on the right, joined to every
 * approval by a bus, so no edge crosses a card.
 */
export function modelGraph(m: Model, roleName: (key: string) => string | undefined): CvGraph {
  const main: CvNode[] = [];
  const edges: CvEdge[] = [];
  const questions = m.fields.length ? ` · ${m.fields.length} ${m.fields.length === 1 ? 'question' : 'questions'}` : '';
  main.push(sized({ id: 'start', kind: 'start', label: 'Request submitted', sub: `${m.request.name.trim() || 'The request'} form${questions}` }));
  for (const s of m.steps) {
    if (s.when) main.push(sized({ id: `when:${s.uid}`, kind: 'decision', label: `Only when ${describeCondition(s.when, m.fields)}`, stepId: s.id }));
    main.push(sized({
      id: `step:${s.uid}`, kind: 'step', label: s.name.trim() || 'Untitled step', stepId: s.id, stepKind: s.kind,
      who: s.who.length ? s.who.map((w) => whoText(w, roleName)).join(', or ') : 'Nobody chosen yet', deadline: deadlineText(s),
    }));
  }
  const approved = sized({ id: 'end:approved', kind: 'end', label: 'Approved', tone: 'done' });
  main.push(approved);
  let y = 0;
  for (const n of main) { n.x = -n.w / 2; n.y = y; y += n.h + GAP; }

  const at = (id: string) => main.findIndex((n) => n.id === id);
  const first = main[1]!;
  edges.push(drop('e:start', main[0]!, first));
  m.steps.forEach((s) => {
    const i = at(`step:${s.uid}`);
    const node = main[i]!;
    const next = main[i + 1]!;
    const labels = [...new Set(routes(m, s).filter((r) => r.outcome !== 'rejected' && r.outcome !== 'changes_requested').map((r) => r.label))];
    edges.push(drop(`e:${s.uid}:next`, node, next, labels.join(' / ')));
    if (s.when) {
      const d = main[i - 1]!;
      edges.push(drop(`e:${s.uid}:applies`, d, node, 'Applies'));
      // "Otherwise" leaves the diamond's right point and rejoins above the step after this one.
      const lane = cx(d) + SIZE.step[0] / 2 + 1.75;
      const right = cx(d) + 1.25;
      const join = next.y - GAP / 2;
      edges.push({
        id: `e:${s.uid}:otherwise`, from: d.id, to: next.id, label: 'Otherwise', labelAt: [right + 0.5, cy(d) - LABEL_H], labelAlign: 'start',
        points: [[right, cy(d)], [lane, cy(d)], [lane, join], [cx(next), join], [cx(next), next.y]],
      });
    }
  });

  const nodes = [...main];
  if (m.steps.some((s) => s.kind === 'approval')) {
    const start = main[0]!;
    const revise = sized({ id: 'end:revise', kind: 'revise', label: 'Back to the requester', sub: 'They change it and resubmit' });
    revise.x = -22 - revise.w / 2;
    revise.y = cy(start) - revise.h / 2;
    const rejected = sized({ id: 'end:rejected', kind: 'end', label: 'Rejected', tone: 'stopped' });
    rejected.x = 20.5 - rejected.w / 2;
    rejected.y = approved.y;
    nodes.push(rejected, revise);
    const half = SIZE.step[0] / 2;
    for (const s of m.steps.filter((x) => x.kind === 'approval')) {
      const n = main[at(`step:${s.uid}`)]!;
      const ey = n.y + n.h * 0.6;
      edges.push({ id: `e:${s.uid}:reject`, from: n.id, to: rejected.id, label: 'Reject', labelAt: [cx(n) + half + 0.5, ey - LABEL_H], labelAlign: 'start',
        points: [[cx(n) + half, ey], [cx(rejected), ey], [cx(rejected), rejected.y]] });
      edges.push({ id: `e:${s.uid}:back`, from: n.id, to: revise.id, label: 'Send back', labelAt: [cx(n) - half - 0.5, ey - LABEL_H], labelAlign: 'end',
        points: [[cx(n) - half, ey], [cx(revise), ey], [cx(revise), bottom(revise)]] });
    }
    const lane = cx(first) - half - 3.25;
    const join = first.y - GAP / 2;
    edges.push({ id: 'e:resubmit', from: revise.id, to: first.id, label: 'Resubmit', labelAt: [revise.x + revise.w + 0.4, cy(revise) - LABEL_H], labelAlign: 'start',
      points: [[revise.x + revise.w, cy(revise)], [lane, cy(revise)], [lane, join], [cx(first), join], [cx(first), first.y]] });
  }
  return { nodes, edges };
}

/* ------------------------------------------------------------ any IR */

const OUTCOME: Record<string, string> = {
  approved: 'Approve', rejected: 'Reject', changes_requested: 'Send back', default: 'Otherwise', acknowledged: 'Acknowledge', expired: 'Expires', done: 'Mark done', submitted: 'Resubmit',
};
const human = (s: string) => s.replace(/_/g, ' ').replace(/^./, (c) => c.toUpperCase());
const nameOf = (n: IrNode) => n.name?.en ?? Object.values(n.name ?? {})[0] ?? human(n.id);

function irNode(n: IrNode, roleName: (key: string) => string | undefined): CvNode {
  const label = nameOf(n);
  const kind = STEP_KINDS.find((k) => k.irKind === n.kind);
  if (n.kind === 'start') return sized({ id: n.id, kind: 'start', label, sub: 'Where it starts' });
  if (n.kind === 'end') return sized({ id: n.id, kind: 'end', label, tone: /reject|cancel|declin|fail/i.test(`${n.id} ${label} ${String(n.result ?? '')}`) ? 'stopped' : 'done' });
  if (n.kind === 'decision') return sized({ id: n.id, kind: 'decision', label });
  if (n.kind === 'parallel.split' || n.kind === 'parallel.join') return sized({ id: n.id, kind: 'gateway', label, tone: n.kind === 'parallel.split' ? 'split' : 'join' });
  if (n.kind === 'notify') return sized({ id: n.id, kind: 'notify', label, sub: 'Sends a notification' });
  if (kind) {
    const days = /^P(\d+)D$/.exec(n.due?.after ?? '');
    const who = (n.assignment?.candidates ?? []).map((c) => whoText(whoFrom(c), roleName)).join(', or ');
    return sized({ id: n.id, kind: 'step', label, stepKind: kind.id, who: who || 'Nobody chosen', deadline: days ? `Due in ${days[1]} ${n.due?.business === false ? '' : 'business '}${days[1] === '1' ? 'day' : 'days'}` : n.due ? 'Has a deadline' : 'No deadline' });
  }
  return sized({ id: n.id, kind: 'other', label, sub: n.kind });
}

/**
 * Any IR, read-only (parallel blocks, notifications…): a layered drawing. Each node's row is its
 * longest path from the start (loops back are drawn on the left), rows are ordered by where their
 * parents sit, and edges that skip a row run down a lane on the right.
 */
export function irGraph(ir: IrDocument, roleName: (key: string) => string | undefined): CvGraph {
  const byId = new Map(ir.nodes.map((n) => [n.id, n]));
  // One edge per pair of nodes, with its outcomes joined ("Acknowledge / Expires").
  const merged: { id: string; from: string; to: string; ons: string[] }[] = [];
  for (const e of ir.edges) {
    if (!byId.has(e.from) || !byId.has(e.to)) continue;
    const same = merged.find((x) => x.from === e.from && x.to === e.to);
    if (same) same.ons.push(e.on ?? 'next'); else merged.push({ id: e.id, from: e.from, to: e.to, ons: [e.on ?? 'next'] });
  }
  const out = (id: string) => merged.filter((e) => e.from === id);

  // Depth-first from the start: discovery order, and the edges that loop back.
  const order: string[] = [];
  const state = new Map<string, 1 | 2>();
  const back = new Set<string>();
  const visit = (id: string) => {
    state.set(id, 1);
    order.push(id);
    for (const e of out(id)) {
      const s = state.get(e.to);
      if (s === 1) back.add(e.id);
      else if (!s) visit(e.to);
    }
    state.set(id, 2);
  };
  const start = ir.nodes.find((n) => n.kind === 'start') ?? ir.nodes[0];
  if (start) visit(start.id);
  for (const n of ir.nodes) if (!state.has(n.id)) visit(n.id);
  const forward = merged.filter((e) => !back.has(e.id));

  // Longest path over the forward edges, in topological order (ties by discovery).
  const layer = new Map<string, number>(order.map((id) => [id, 0]));
  const indeg = new Map<string, number>(order.map((id) => [id, 0]));
  for (const e of forward) indeg.set(e.to, (indeg.get(e.to) ?? 0) + 1);
  const queue = order.filter((id) => indeg.get(id) === 0);
  while (queue.length) {
    const id = queue.shift()!;
    for (const e of forward.filter((x) => x.from === id)) {
      layer.set(e.to, Math.max(layer.get(e.to) ?? 0, (layer.get(id) ?? 0) + 1));
      indeg.set(e.to, (indeg.get(e.to) ?? 1) - 1);
      if (indeg.get(e.to) === 0) queue.push(e.to);
    }
  }

  const nodes = new Map(order.map((id) => [id, irNode(byId.get(id)!, roleName)]));
  const rows: string[][] = [];
  for (const id of order) (rows[layer.get(id)!] ??= []).push(id);
  const slot = new Map<string, number>();
  /** Each node's row band (top and bottom), so edges bend only in the gaps between rows. */
  const band = new Map<string, { top: number; bottom: number }>();
  let y = 0;
  for (const row of rows.filter(Boolean)) {
    const bary = (id: string) => {
      const ps = forward.filter((e) => e.to === id).map((e) => slot.get(e.from)).filter((v): v is number => v !== undefined);
      return ps.length ? ps.reduce((a, b) => a + b, 0) / ps.length : Number.POSITIVE_INFINITY;
    };
    const sorted = [...row].sort((a, b) => bary(a) - bary(b) || order.indexOf(a) - order.indexOf(b));
    const width = sorted.reduce((w, id) => w + nodes.get(id)!.w, 0) + 2.5 * (sorted.length - 1);
    const height = Math.max(...sorted.map((id) => nodes.get(id)!.h));
    let x = -width / 2;
    for (const id of sorted) {
      const n = nodes.get(id)!;
      n.x = x; n.y = y + (height - n.h) / 2;
      slot.set(id, cx(n)); // the next row orders by its parents' centres
      band.set(id, { top: y, bottom: y + height });
      x += n.w + 2.5;
    }
    y += height + GAP;
  }

  const all = [...nodes.values()];
  let lanes = 0;
  const edges: CvEdge[] = merged.map((e) => {
    const a = nodes.get(e.from)!;
    const b = nodes.get(e.to)!;
    const ba = band.get(a.id)!;
    const bb = band.get(b.id)!;
    const isBack = back.has(e.id) || bb.top <= ba.top;
    const split = byId.get(e.from)?.kind === 'parallel.split';
    const label = split ? undefined : [...new Set(e.ons.filter((o) => o !== 'next').map((o) => (o === 'submitted' ? (isBack ? 'Resubmit' : 'Submit') : OUTCOME[o] ?? human(o))))].join(' / ') || undefined;
    const base = { id: e.id, from: a.id, to: b.id, ...(label ? { label } : {}) };
    const below = ba.bottom + GAP * 0.3;
    if (isBack) {
      // Down out of the source, up a lane on the left of everything in between, into the target from above.
      const span = all.filter((n) => band.get(n.id)!.bottom >= bb.top && band.get(n.id)!.top <= ba.bottom);
      const lane = Math.min(...span.map((n) => n.x)) - 1.75 - lanes++ * 0.75;
      const sx = cx(a) - a.w * 0.3;
      const tx = b.x + b.w * 0.2;
      const above = bb.top - GAP * 0.7;
      return { ...base, points: [[sx, bottom(a)], [sx, below], [lane, below], [lane, above], [tx, above], [tx, b.y]], labelAt: [lane - 0.4, below + 0.2], labelAlign: 'end' };
    }
    const join = bb.top - GAP / 2;
    const between = all.filter((n) => band.get(n.id)!.top > ba.bottom && band.get(n.id)!.bottom < bb.top);
    if (between.length) {
      // Skips a row: round the rows in between on the nearer side.
      const right = cx(a) + cx(b) >= 0;
      const k = lanes++;
      const lane = right ? Math.max(...between.map((n) => n.x + n.w)) + 1.75 + k : Math.min(...between.map((n) => n.x)) - 1.75 - k;
      // Neighbouring lanes' labels sit at different heights so they don't overlap.
      return { ...base, points: [[cx(a), bottom(a)], [cx(a), below], [lane, below], [lane, join], [cx(b), join], [cx(b), b.y]],
        labelAt: [right ? lane + 0.4 : lane - 0.4, below + 0.2 + (k % 3) * (LABEL_H + 0.25)], labelAlign: right ? 'start' : 'end' };
    }
    const fanOut = out(e.from).filter((x) => !back.has(x.id)).length > 1;
    const pts: Pt[] = cx(a) === cx(b) ? [[cx(a), bottom(a)], [cx(b), b.y]] : [[cx(a), bottom(a)], [cx(a), join], [cx(b), join], [cx(b), b.y]];
    return { ...base, points: pts, labelAt: fanOut ? [cx(b) + 0.5, join + 0.3] : [cx(a) + 0.5, bottom(a) + 0.4], labelAlign: 'start' };
  });
  return { nodes: all.sort((p, q) => p.y - q.y || p.x - q.x), edges };
}

/* ------------------------------------------------------------ keyboard */

export type Direction = 'up' | 'down' | 'left' | 'right';

/**
 * The node an arrow key moves to: the nearest connected node that way, else the nearest node that
 * way at all. Distance across the arrow counts double, so ↓ prefers what is below over what is beside.
 */
export function neighbour(g: CvGraph, from: string, dir: Direction): string | undefined {
  const a = g.nodes.find((n) => n.id === from);
  if (!a) return undefined;
  const linked = new Set(g.edges.flatMap((e) => (e.from === from ? [e.to] : e.to === from ? [e.from] : [])));
  const score = (b: CvNode): number | null => {
    const dx = cx(b) - cx(a);
    const dy = cy(b) - cy(a);
    const along = dir === 'down' ? dy : dir === 'up' ? -dy : dir === 'right' ? dx : -dx;
    const across = dir === 'down' || dir === 'up' ? Math.abs(dx) : Math.abs(dy);
    return along > 1 ? along + 2 * across : null;
  };
  const best = (pool: CvNode[]) => {
    let pick: CvNode | undefined;
    let low = Number.POSITIVE_INFINITY;
    for (const b of pool) {
      const s = b.id === from ? null : score(b);
      if (s !== null && s < low) { low = s; pick = b; }
    }
    return pick?.id;
  };
  return best(g.nodes.filter((n) => linked.has(n.id))) ?? best(g.nodes);
}
