import { ApiError, api } from '../../lib/api';
import { toIso } from '../../lib/format';
import { type Envelope, type Recipient, RECIPIENT_HUES } from '../../lib/signing';

/** The server's words for a refusal (its detail names what's wrong), or a fallback. */
export function problemText(e: unknown, fallback: string): string {
  if (e instanceof ApiError) return e.problem.detail && e.status !== 412 ? e.problem.detail : e.problem.title;
  return fallback;
}

/** "12 Oct 2026, 10:42" for a timestamp (ISO or the tuple form some payloads still use). */
export function dateTime(v: unknown): string {
  const iso = toIso(v);
  return iso ? new Date(iso).toLocaleString(undefined, { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' }) : '';
}

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return '?';
  return (parts.length === 1 ? parts[0]!.slice(0, 2) : parts[0]![0]! + parts[parts.length - 1]![0]!).toUpperCase();
}

/** A signer's colour: by their position among the signers (CC recipients have no fields). */
export function hueOf(recipients: Recipient[], id: string): string {
  const i = recipients.filter((r) => r.role === 'signer').findIndex((r) => r.id === id);
  return RECIPIENT_HUES[(i < 0 ? 0 : i) % RECIPIENT_HUES.length]!;
}

export const signers = (e: Envelope) => e.recipients.filter((r) => r.role === 'signer').sort((a, b) => a.ord - b.ord);

/** "1 of 2 signed". */
export function progressText(e: Envelope): string {
  const s = signers(e);
  return s.length ? `${s.filter((r) => r.state === 'signed').length} of ${s.length} signed` : 'No signers yet';
}

/** A draft change with the envelope's revision as If-Match (412 when someone else changed it). */
export function putDraft(id: string, what: 'recipients' | 'fields', body: unknown, revision: number): Promise<Envelope> {
  return api<Envelope>(`/api/v1/envelopes/${id}/${what}`, { method: 'PUT', headers: { 'If-Match': `"${revision}"` }, body });
}
