import { useEffect, useState } from 'react';
import { ApiError, api } from './api';
import type { TagColor } from '../ui/Tag';

/*
 * Requests and approvals (0.2, BPMS-103/105): the request catalogue, requests, steps and work items,
 * as the API returns them (docs/08). Shared by the Requests area, My work, the request page and Home.
 */

export type FieldType = 'string' | 'text' | 'enum' | 'money' | 'date' | 'number' | 'boolean' | 'principal' | 'file';
export interface FormField { name: string; label: string; type: FieldType; required?: boolean; options?: { value: string; label: string }[]; help?: string; currency?: string; multiple?: boolean }
export interface RequestType { key: string; name: string; description: string; category: string; icon?: string; form: { fields: FormField[] } }
export type RequestStatus = 'in_progress' | 'approved' | 'rejected' | 'cancelled' | 'changes_requested';
export interface Person { id: string; name: string }
export interface RequestRow { id: string; display_id: string; type: string; type_name: string; title: string; status: RequestStatus; current_step: string | null; assignees: string[]; created_at: string; updated_at: string; initiator: Person }
export type StepState = 'waiting' | 'active' | 'done' | 'skipped' | 'cancelled';
/** Someone out of office (BPMS-086): their last day away (null once back) and who decides for them. */
export interface Away { id: string; name: string; until: string | null; delegate: Person | null }
export interface Step { name: string; state: StepState; assignees: string[]; decided_by?: string | null; on_behalf_of?: string | null; away?: Away[]; outcome?: string | null; comment?: string | null; decided_at?: string | null; due_at?: string | null }
export interface RequestDetail { id: string; display_id: string; type: string; type_name: string; title: string; status: RequestStatus; initiator: Person; created_at?: string; input: Record<string, unknown>; form: { fields: FormField[] }; steps: Step[]; timeline: { at: string; kind?: string; actor?: string | null; text: string; files?: Attachment[] }[]; attachments?: Attachment[]; policy?: PolicyRef | null; sla_due_at?: string | null; can_cancel: boolean; why?: WhyLine[]; release?: { version: number; checksum: string | null } }
export interface WorkItem { id: string; revision: number; request: { id: string; display_id: string; type_name: string; title: string; initiator: Person }; step: string; outcomes: string[]; due_at?: string | null; created_at: string; fields?: FormField[]; decided_at?: string | null; outcome?: string | null; overdue?: boolean; assignees?: string[]; on_behalf_of?: Away | null; why_you?: string | null; attachments?: Attachment[]; policy?: PolicyRef | null }
/** One "why this route" line (PR-08): a routing decision or why a step went to its people. */
/** A file on a request: from a form field (`field`) or attached with a decision (`source` names the step). */
export interface Attachment { id: string; name: string; size: number; state: string; sniffed_type: string | null; source: string; field: string | null }
/** The published policy or SOP version linked to a request type. */
export interface PolicyRef { artifact_id: string; version_id: string; kind: 'policy' | 'sop'; title: string; version: string }
/** "Who approves this" (POST /requests/preview). */
export interface PreviewStep { node: string; name: string; first: boolean; who: string; unassigned: boolean; condition: string | null; applies: boolean | null }
export interface RoutePreview { summary: string; unassigned: boolean; steps: PreviewStep[] }
export interface WhyLine { at: string; step: string; text: string; facts: { kind?: string } & Record<string, unknown> }

/** "Away until Fri 16 Oct" (or just "Away"), for the Away chip's text. */
export const awayUntil = (a: Away) => (a.until ? `Away until ${new Date(`${a.until}T00:00:00`).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' })}` : 'Away');

export const STATUS_TAG: Record<RequestStatus, { label: string; color: TagColor }> = {
  in_progress: { label: 'In progress', color: 'blue' },
  changes_requested: { label: 'Sent back', color: 'orange' },
  approved: { label: 'Approved', color: 'green' },
  rejected: { label: 'Rejected', color: 'red' },
  cancelled: { label: 'Cancelled', color: 'gray' },
};

/** How each decision reads on its button and in the history. Unknown outcomes are humanised. */
const OUTCOMES: Record<string, { verb: string; past: string; tone: 'primary' | 'secondary' | 'reject' }> = {
  approved: { verb: 'Approve', past: 'Approved', tone: 'primary' },
  approve: { verb: 'Approve', past: 'Approved', tone: 'primary' },
  changes_requested: { verb: 'Send back', past: 'Sent back', tone: 'secondary' },
  send_back: { verb: 'Send back', past: 'Sent back', tone: 'secondary' },
  rejected: { verb: 'Reject', past: 'Rejected', tone: 'reject' },
  reject: { verb: 'Reject', past: 'Rejected', tone: 'reject' },
  submitted: { verb: 'Resubmit', past: 'Resubmitted', tone: 'primary' },
  resubmitted: { verb: 'Resubmit', past: 'Resubmitted', tone: 'primary' },
  resubmit: { verb: 'Resubmit', past: 'Resubmitted', tone: 'primary' },
  completed: { verb: 'Mark done', past: 'Done', tone: 'primary' },
  complete: { verb: 'Mark done', past: 'Done', tone: 'primary' },
};
const human = (s: string) => s.replace(/_/g, ' ').replace(/^./, (c) => c.toUpperCase());
export const outcomeVerb = (o: string) => OUTCOMES[o]?.verb ?? human(o);
export const outcomePast = (o: string) => OUTCOMES[o]?.past ?? human(o);
export const outcomeTone = (o: string) => OUTCOMES[o]?.tone ?? 'secondary';
/** Rejecting and sending back always say why (the server enforces it too). */
export const needsComment = (o: string) => ['rejected', 'reject', 'changes_requested', 'send_back'].includes(o);

/** "Ana", "Ana and Ben", "Ana, Ben and 2 others". */
export function people(names: string[]): string {
  if (names.length === 0) return '';
  if (names.length <= 2) return names.join(' and ');
  const rest = names.length - 2;
  return `${names[0]}, ${names[1]} and ${rest} ${rest === 1 ? 'other' : 'others'}`;
}

const DAY = 86_400_000;
/** Due wording for a work item or step: overdue, today, in N days. */
export function dueInfo(iso: string | null | undefined, notOverdue = false): { label: string; color: TagColor; overdue: boolean } | null {
  if (!iso) return null;
  const ms = Date.parse(iso) - Date.now();
  // The server's clock decides overdue when it has said so (it may differ from this device's).
  if (ms < 0 && notOverdue) return { label: 'Due today', color: 'orange', overdue: false };
  if (ms < 0) return { label: 'Overdue', color: 'red', overdue: true };
  const days = Math.ceil(ms / DAY);
  return { label: days <= 1 ? 'Due today' : `Due in ${days} days`, color: days <= 2 ? 'orange' : 'gray', overdue: false };
}

/** A submitted value as text, using the form's labels for choices and names for people. */
export function displayValue(f: FormField, v: unknown, names: Map<string, string>): string {
  if (v === null || v === undefined || v === '') return '—';
  switch (f.type) {
    case 'boolean': return v ? 'Yes' : 'No';
    case 'enum': return f.options?.find((o) => o.value === v)?.label ?? String(v);
    case 'principal': return names.get(String(v)) ?? 'Someone in this workspace';
    case 'date': { const d = new Date(`${String(v).slice(0, 10)}T00:00:00`); return Number.isNaN(d.getTime()) ? String(v) : d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' }); }
    case 'money': {
      const m = typeof v === 'object' ? (v as { amount?: string; currency?: string }) : { amount: String(v), currency: f.currency };
      const n = Number(m.amount);
      if (Number.isNaN(n)) return String(m.amount ?? '');
      return m.currency ? new Intl.NumberFormat(undefined, { style: 'currency', currency: m.currency }).format(n) : n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
    }
    case 'number': return Number(v).toLocaleString();
    case 'file': { const list = Array.isArray(v) ? v : [v]; return list.length ? list.map((x) => (x as { name?: string }).name ?? 'A file').join(', ') : '—'; }
    default: return String(v);
  }
}

/**
 * Field errors from a 422 problem (docs/08 §2: `errors[{pointer, message}]`, pointer `/fields/amount`
 * or `/input/amount`). A field-keyed object is accepted too. Returns `{fieldName: message}`.
 */
export function fieldErrors(e: unknown): Record<string, string> {
  if (!(e instanceof ApiError) || e.status !== 422) return {};
  const raw = (e.problem as unknown as { errors?: unknown }).errors;
  const out: Record<string, string> = {};
  if (Array.isArray(raw)) {
    for (const x of raw as { pointer?: string; field?: string; name?: string; message?: string; detail?: string }[]) {
      const key = (x.field ?? x.name ?? x.pointer ?? '').split('/').filter(Boolean).pop();
      if (key) out[key] = x.message ?? x.detail ?? 'Check this field.';
    }
  } else if (raw && typeof raw === 'object') {
    for (const [k, v] of Object.entries(raw as Record<string, unknown>)) out[k] = Array.isArray(v) ? String(v[0]) : String(v);
  }
  return out;
}

/** Open work items assigned to me, for the sidebar badge; re-read when `key` changes (navigation). */
export function useOpenWork(enabled: boolean, key: unknown): number {
  const [n, setN] = useState(0);
  useEffect(() => {
    if (!enabled) { setN(0); return; }
    let live = true;
    api<{ items: unknown[] }>('/api/v1/work?state=open').then((r) => { if (live) setN(r.items.length); }).catch(() => undefined);
    const on = () => { api<{ items: unknown[] }>('/api/v1/work?state=open').then((r) => { if (live) setN(r.items.length); }).catch(() => undefined); };
    window.addEventListener('procera:work-changed', on);
    return () => { live = false; window.removeEventListener('procera:work-changed', on); };
  }, [enabled, key]);
  return n;
}

/** Tells the shell (badge) and Home that a work item was decided. */
export const workChanged = () => window.dispatchEvent(new Event('procera:work-changed'));

/**
 * The server reports steps with work-item states (offered, completed, cancelled) and the start
 * form as `fields`; the pages speak in waiting/active/done and `form.fields`.
 */
export function normalizeDetail(d: RequestDetail & { fields?: RequestDetail['form']['fields'] }): RequestDetail {
  const STATE: Record<string, StepState> = { offered: 'active', claimed: 'active', completed: 'done', cancelled: 'cancelled', expired: 'cancelled' };
  return {
    ...d,
    form: d.form ?? { fields: d.fields ?? [] },
    steps: (d.steps ?? []).map((s) => ({ ...s, state: STATE[s.state as string] ?? s.state })),
  };
}
