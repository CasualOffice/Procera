import { Check, CircleAlert } from 'lucide-react';
import { useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type WorkItem, workChanged } from '../lib/requests';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';

/*
 * Bulk decisions in My work (BPMS-216, FR-R03 "bulk with preview"): approve or reject several
 * plain approvals at once. Only approval steps with nothing to fill in can be picked, the dialog
 * lists every item before anything happens, and each one is decided on its own with its revision
 * as If-Match — so an item decided elsewhere meanwhile is reported, never overwritten. Rejecting
 * needs a reason, as it does one at a time.
 */

/** Approvals with nothing to fill in, which can be decided together. */
export const bulkable = (w: WorkItem) => (w.kind ?? 'approval') === 'approval' && !(w.fields ?? []).length && w.outcomes.includes('approved');

type Result = { item: WorkItem; ok: boolean; message?: string };

export function BulkDecideDialog({ items, outcome, onClose, onDone }: { items: WorkItem[]; outcome: 'approved' | 'rejected'; onClose: () => void; onDone: (done: number) => void }) {
  const [comment, setComment] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<Result[] | null>(null);
  const approving = outcome === 'approved';
  const run = async () => {
    if (!approving && !comment.trim()) { setError('Say why, so each requester knows.'); return; }
    setBusy(true); setError(null);
    const out: Result[] = [];
    for (const w of items) {
      try {
        await api(`/api/v1/work/${w.id}/decisions`, {
          method: 'POST', headers: { 'If-Match': `"${w.revision}"` },
          body: { outcome, ...(comment.trim() ? { comment: comment.trim() } : {}) },
        });
        out.push({ item: w, ok: true });
      } catch (e) {
        const message = e instanceof ApiError && (e.status === 409 || e.status === 412) ? 'Someone decided it already, or it changed.'
          : e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Couldn’t decide it.';
        out.push({ item: w, ok: false, message });
      }
      setResults([...out]);
    }
    setBusy(false);
    workChanged();
    if (out.every((r) => r.ok)) onDone(out.length);
  };
  const done = results?.filter((r) => r.ok).length ?? 0;
  const failed = results?.filter((r) => !r.ok) ?? [];
  const finished = results !== null && !busy;
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && !busy && (finished ? onDone(done) : onClose())}
      title={`${approving ? 'Approve' : 'Reject'} ${items.length} ${items.length === 1 ? 'request' : 'requests'}`}
      description={approving ? 'Each request is approved on its own and moves to its next step. Check the list first.' : 'Each requester is told, with your reason.'}
      footer={finished
        ? <Button onPress={() => onDone(done)}>Done</Button>
        : <><Button variant="secondary" onPress={onClose} isDisabled={busy}>Cancel</Button><Button variant={approving ? 'primary' : 'danger'} onPress={() => void run()} isDisabled={busy}>{busy ? `${approving ? 'Approving' : 'Rejecting'} ${results?.length ?? 0} of ${items.length}…` : `${approving ? 'Approve' : 'Reject'} ${items.length}`}</Button></>}>
      {finished && failed.length > 0 && <Notice tone="warning" title={`${done} decided; ${failed.length} couldn’t be.`} />}
      <ul className="bulk-preview" aria-label={`Requests to ${approving ? 'approve' : 'reject'}`}>
        {items.map((w) => {
          const r = results?.find((x) => x.item.id === w.id);
          return (
            <li key={w.id} className={r ? (r.ok ? 'is-ok' : 'is-failed') : undefined}>
              <span className="bulk-state" aria-hidden="true">{r ? (r.ok ? <Check size="0.875rem" /> : <CircleAlert size="0.875rem" />) : null}</span>
              <span className="bulk-main">
                <strong>{w.request.display_id}</strong> {w.request.title}
                <span className="bulk-sub">{w.request.initiator.name} · {w.step}{r && !r.ok ? ` · ${r.message}` : ''}</span>
              </span>
              {r && <span className="visually-hidden">{r.ok ? (approving ? 'Approved' : 'Rejected') : `Not decided: ${r.message}`}</span>}
            </li>
          );
        })}
      </ul>
      {!finished && (
        <div className="field">
          <label className="field-label" htmlFor="bulk-comment">{approving ? 'Comment' : 'Reason'}{!approving && <span aria-hidden="true" className="field-required"> *</span>}</label>
          <textarea id="bulk-comment" rows={2} value={comment} onChange={(e) => { setComment(e.target.value); setError(null); }} disabled={busy}
            aria-invalid={!!error} aria-describedby={error ? 'bulk-comment-e' : 'bulk-comment-h'} />
          {error ? <span id="bulk-comment-e" className="field-error">{error}</span> : <span id="bulk-comment-h" className="field-description">{approving ? 'Optional. Added to every request’s timeline.' : 'Required. Every requester sees it.'}</span>}
        </div>
      )}
    </FormDialog>
  );
}

