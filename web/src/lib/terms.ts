/**
 * Workspace terminology (CFG-008, FR-W04, docs/20 §2.2): a workspace may rename product terms
 * ("Request" → "Ticket"). The shell loads the effective labels from `GET /api/v1/branding`; until
 * then, and for any term the workspace hasn't renamed, the product defaults apply. Labels are display
 * copy only: API values and fields (`kind: 'sop'`) never change. Mirrors crates/settings/src/terms.rs.
 */
import { useSyncExternalStore } from 'react';

export type Term = 'request' | 'process' | 'step' | 'sop' | 'case' | 'work';
export interface Label { singular: string; plural: string }

export const TERMS: Term[] = ['request', 'process', 'step', 'sop', 'case', 'work'];

export const TERM_DEFAULTS: Record<Term, Label> = {
  request: { singular: 'Request', plural: 'Requests' },
  process: { singular: 'Process', plural: 'Processes' },
  step: { singular: 'Step', plural: 'Steps' },
  sop: { singular: 'SOP', plural: 'SOPs' },
  case: { singular: 'Case', plural: 'Cases' },
  work: { singular: 'Work', plural: 'Work' },
};

let current: Record<Term, Label> = TERM_DEFAULTS;
const listeners = new Set<() => void>();

/** Replaces the workspace labels (missing or malformed terms fall back to the defaults). */
export function setTerms(next?: Partial<Record<string, Partial<Label>>> | null) {
  const merged = { ...TERM_DEFAULTS };
  for (const t of TERMS) {
    const l = next?.[t];
    if (l && typeof l.singular === 'string' && l.singular.trim() && typeof l.plural === 'string' && l.plural.trim()) merged[t] = { singular: l.singular, plural: l.plural };
  }
  current = merged;
  listeners.forEach((f) => f());
}

/**
 * Lowers each word's first letter for use mid-sentence, unless the word is an acronym (its first two
 * letters are capitals: "SOP", "SOPs", "HR").
 */
export function lowerLabel(label: string): string {
  return label.split(' ').map((w) => {
    const [a, b] = [...w].filter((c) => c.toLowerCase() !== c.toUpperCase());
    const acronym = !!a && !!b && a === a.toUpperCase() && b === b.toUpperCase();
    return acronym || !w ? w : w.charAt(0).toLowerCase() + w.slice(1);
  }).join(' ');
}

export interface TermOptions { plural?: boolean; capitalised?: boolean }

/**
 * The workspace's label for `t`. Mid-sentence by default ("ticket", "SOP"); `capitalised` gives the
 * heading form ("Ticket"). Read inside a component that calls `useTerms()` so it re-renders on change.
 */
export function term(t: Term, o: TermOptions = {}): string {
  const l = current[t] ?? TERM_DEFAULTS[t];
  const s = o.plural ? l.plural : l.singular;
  return o.capitalised ? s.charAt(0).toUpperCase() + s.slice(1) : lowerLabel(s);
}

/** Subscribes the component to terminology changes and returns `term`. */
export function useTerms(): typeof term {
  useSyncExternalStore(subscribe, snapshot);
  return term;
}

function subscribe(f: () => void) { listeners.add(f); return () => { listeners.delete(f); }; }
function snapshot() { return current; }

/** A library document kind as a label: SOPs take the workspace term; policies stay "Policy". */
export function kindLabel(kind: string, o: TermOptions = { capitalised: true }): string {
  if (kind === 'sop') return term('sop', o);
  const policy = o.plural ? 'Policies' : 'Policy';
  if (kind === 'policy') return o.capitalised ? policy : policy.toLowerCase();
  return kind;
}
