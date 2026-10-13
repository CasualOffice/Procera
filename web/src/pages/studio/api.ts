import { ApiError, api } from '../../lib/api';
import type { Content } from './model';

/* The processes API (docs/08 "Processes (0.3, BPMS-100/084)"), as the studio reads it. */

export type ProcessState = 'draft' | 'in_review' | 'approved' | 'published' | 'deprecated' | 'retired' | string;
export interface ProcessRow {
  id: string; key: string; title: string; state: ProcessState;
  draft: { version_id: string; version_no: number; lifecycle: string } | null;
  published: { version_id: string; version_no: number; published_at: string } | null;
}
export interface ProcessDetail {
  id: string; key: string; title: string; state: ProcessState;
  draft: { version_id: string; version_no: number; lifecycle: string; revision: number; content: Content } | null;
  published: { version_id: string; version_no: number; published_at: string; content: Content } | null;
  can: { edit: boolean; publish: boolean };
}
export interface ProcessTemplate { key: string; name: string; description: string }
export interface Problem { severity: 'error' | 'warning'; code: string; pointer: string; node: string | null; message: string }

export const KEY_RE = /^[a-z0-9][a-z0-9._-]{1,63}$/;

/** "Laptop request" → "laptop-request". */
export function keyFrom(title: string): string {
  return title.toLowerCase().normalize('NFKD').replace(/[^a-z0-9.]+/g, '-').replace(/^[-.]+|[-.]+$/g, '').slice(0, 64).replace(/[-.]+$/, '');
}

export const createProcess = (body: { key: string; title: string; template: string }) =>
  api<{ id: string }>('/api/v1/processes', { method: 'POST', headers: { 'idempotency-key': crypto.randomUUID() }, body });

/** Saves the draft. `revision` is the draft's, or null when this opens a new draft from the published version. */
export const saveDraft = (id: string, content: Content, revision: number | null) =>
  api<{ version_id: string; revision: number }>(`/api/v1/processes/${id}/draft`, { method: 'PUT', headers: revision === null ? {} : { 'if-match': `"${revision}"` }, body: { content } });

export const validate = (content: Content) =>
  api<{ ok: boolean; problems: Problem[] }>('/api/v1/processes/validate', { method: 'POST', body: { content } });

export const publish = (id: string) =>
  api<{ state: 'in_review' | 'published'; version_no: number }>(`/api/v1/processes/${id}/publish`, { method: 'POST', headers: { 'idempotency-key': crypto.randomUUID() } });

/** The problems a 422 carries (`{problems}` on the problem body), or null. */
export function problemsOf(e: unknown): Problem[] | null {
  if (!(e instanceof ApiError) || e.status !== 422) return null;
  const p = (e.problem as unknown as { problems?: Problem[] }).problems;
  return Array.isArray(p) ? p : null;
}
