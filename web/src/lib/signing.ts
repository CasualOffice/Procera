import { bytes } from './api';

/** E-signing (BPMS-107, docs/08 "Signing"). Field boxes are fractions of the page from its top-left. */
export type FieldKind = 'signature' | 'initials' | 'date_signed' | 'name' | 'text' | 'checkbox';
export interface SignField { id?: string; recipient_id: string; kind: FieldKind; page: number; x: number; y: number; w: number; h: number; required: boolean; label: string | null; value?: unknown }
export interface Recipient { id: string; ord: number; membership_id: string; name: string; email: string; role: 'signer' | 'cc'; state: 'pending' | 'signed' | 'declined'; viewed_at: string | null; signed_at: string | null; declined_at: string | null; decline_reason: string | null }
export type EnvelopeState = 'draft' | 'sent' | 'completed' | 'declined' | 'voided' | 'expired';
export interface Envelope {
  id: string; title: string; message: string | null; source_file_id: string; document_hash: string; pages: [number, number][];
  order_mode: 'sequential' | 'parallel'; mode: 'simple' | 'strong'; state: EnvelopeState; created_by: string; sender_name: string;
  created_at: string; sent_at: string | null; expires_at: string | null; completed_at: string | null; completed_file_id: string | null;
  completed_hash: string | null; ended_reason: string | null; revision: number; recipients: Recipient[]; fields: SignField[];
  /** A group send's draft: its one signer stands for each person. */ bulk: boolean; batch_id: string | null; reminder_days: number | null;
}
export interface InboxItem {
  recipient_id: string; envelope_id: string; title: string; sender_name: string; role: 'signer' | 'cc'; state: Recipient['state'];
  envelope_state: EnvelopeState; to_sign: boolean; sent_at: string | null; expires_at: string | null; signed_at: string | null; completed_file_id: string | null;
}
export interface Ceremony {
  envelope: { id: string; title: string; message: string | null; sender_name: string; state: EnvelopeState; document_hash: string; pages: [number, number][]; mode: 'simple' | 'strong'; order_mode: string; expires_at: string | null; completed_file_id: string | null };
  recipient: Recipient; my_turn: boolean; fields: SignField[];
  progress: { name: string; ord: number; state: Recipient['state']; signed_at: string | null }[];
  consent_text: string; meaning: string;
}

export const FIELD_KINDS: { id: FieldKind; label: string; w: number; h: number }[] = [
  { id: 'signature', label: 'Signature', w: 0.3, h: 0.06 },
  { id: 'initials', label: 'Initials', w: 0.1, h: 0.05 },
  { id: 'date_signed', label: 'Date signed', w: 0.2, h: 0.035 },
  { id: 'name', label: 'Name', w: 0.28, h: 0.035 },
  { id: 'text', label: 'Text', w: 0.3, h: 0.035 },
  { id: 'checkbox', label: 'Checkbox', w: 0.035, h: 0.025 },
];
export const kindLabel = (k: FieldKind) => FIELD_KINDS.find((f) => f.id === k)?.label ?? k;

export const STATE_LABEL: Record<EnvelopeState, string> = {
  draft: 'Draft', sent: 'Waiting for signatures', completed: 'Completed', declined: 'Declined', voided: 'Cancelled', expired: 'Expired',
};
export const STATE_COLOR: Record<EnvelopeState, 'gray' | 'blue' | 'green' | 'red' | 'orange'> = {
  draft: 'gray', sent: 'blue', completed: 'green', declined: 'red', voided: 'gray', expired: 'orange',
};

/** Distinct, colour-blind-safe hues for recipients (also labelled by name; colour is never the only cue). */
export const RECIPIENT_HUES = ['#2563EB', '#C2410C', '#047857', '#7C3AED', '#B45309', '#BE185D'];

/** The document as bytes (the original, or the signed copy once completed). */
export async function envelopePdf(id: string, signed = false): Promise<ArrayBuffer> {
  return bytes(`/api/v1/envelopes/${id}/document${signed ? '?signed=true' : ''}`);
}

export interface BatchRow {
  envelope_id: string; recipient_id: string; membership_id: string; name: string; email: string; department: string | null;
  state: EnvelopeState; viewed_at: string | null; signed_at: string | null; decline_reason: string | null; reminders_sent: number;
}
export interface Batch {
  id: string; title: string; template_id: string; total: number; created_by: string; sender_name: string; created_at: string; expires_at: string | null;
  counts: { total: number; signed: number; waiting: number; viewed: number; declined: number; cancelled: number; expired: number };
  rows: BatchRow[];
}
