import { Ban, Check, Minus } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Form, Label, TextArea, TextField as AriaTextField, FieldError } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { type Member, useItems, useResource } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { navigate, useRoute } from '../lib/router';
import { type RequestDetail as Detail, type Step, type WorkItem, STATUS_TAG, displayValue, dueInfo, outcomePast, people } from '../lib/requests';
import { useSession } from '../lib/session';
import { setCrumb } from '../ui/AppShell';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { LoadError, Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';
import { DecisionPanel } from './workDecision';
import './requests.css';

const when = (iso?: string | null) => (iso ? new Date(iso).toLocaleString(undefined, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' }) : '');

/**
 * A request (BPMS-103, `/requests/{id}`): who asked for what, a vertical stepper of the approval steps
 * (done, current, still to come), the submitted answers with the form's labels, the timeline, Cancel
 * for the requester, and the decision panel inline when one of my work items is on this request.
 */
export function RequestDetail({ id }: { id: string }) {
  const { can } = useSession();
  const { query } = useRoute();
  const [r, reload, error] = useResource<Detail>(`/api/v1/requests/${id}`);
  const [work, reloadWork] = useItems<WorkItem>(can('work:view_assigned') ? '/api/v1/work?state=open' : null);
  const [members] = useItems<Member>(r?.form.fields.some((f) => f.type === 'principal') ? '/api/v1/members' : null, [r?.id]);
  const [notice, setNotice] = useState<string | null>(null);
  const [cancelling, setCancelling] = useState(false);
  useEffect(() => { if (r) setCrumb(r.display_id); return () => setCrumb(null); }, [r]);
  const mine = (work ?? []).find((w) => w.request.id === id) ?? null;
  const names = useMemo(() => new Map((members ?? []).map((m) => [m.id, m.display_name])), [members]);

  if (error) return <LoadError error={error} onRetry={reload} what="this request" />;
  if (!r) return <Skeleton variant="doc" label="Loading request" />;
  const status = STATUS_TAG[r.status] ?? { label: r.status, color: 'gray' as const };
  const active = r.steps.find((s) => s.state === 'active');
  const submitted = query.get('submitted') === '1';
  const reviewers = people(active?.assignees ?? []);
  const sla = r.status === 'in_progress' ? dueInfo(r.sla_due_at) : null;

  return (
    <div className="req">
      {submitted && !notice && <Notice tone="success" title={`Submitted ${r.display_id}.${reviewers ? ` ${reviewers} will review it.` : ''}`}>You’ll get a notification when there’s a decision.</Notice>}
      {notice && <Notice tone="success" title={notice} />}
      <header className="req-head">
        <div className="req-head-main">
          <p className="req-eyebrow"><span className="req-id">{r.display_id}</span> · {r.type_name}</p>
          <h1>{r.title}</h1>
          <div className="req-meta">
            <Tag color={status.color} dot>{status.label}</Tag>
            {sla && <Tag color={sla.color} dot={sla.overdue}>{sla.overdue ? 'Past its deadline' : sla.label}</Tag>}
            <span className="req-by"><Avatar name={r.initiator.name} size="1.25rem" /> Requested by <strong>{r.initiator.name}</strong>{r.created_at && <> · <time dateTime={r.created_at}>{shortDate(r.created_at)}</time></>}</span>
          </div>
        </div>
        {r.can_cancel && <Button variant="secondary" onPress={() => setCancelling(true)}><Ban aria-hidden="true" size="0.9375rem" /> Cancel request</Button>}
      </header>

      <div className="req-grid">
        <div className="req-main">
          {mine && <DecisionPanel key={mine.id} item={mine} detail={r} onDecided={(o) => { setNotice(`${outcomePast(o)}. ${r.display_id} is off your list.`); reload(); reloadWork(); }} />}
          <section aria-labelledby="req-details" className="req-section">
            <h2 id="req-details">Details</h2>
            {r.form.fields.length === 0 ? <p className="muted">No answers were needed for this request.</p> : (
              <dl className="req-values">
                {r.form.fields.map((f) => (
                  <div key={f.name} className={f.type === 'text' ? 'req-value req-value-wide' : 'req-value'}>
                    <dt>{f.label}</dt>
                    <dd>{displayValue(f, r.input[f.name], names)}</dd>
                  </div>
                ))}
              </dl>
            )}
          </section>
          <section aria-labelledby="req-timeline" className="req-section">
            <h2 id="req-timeline">Timeline</h2>
            {r.timeline.length === 0 ? <p className="muted">Nothing has happened yet.</p> : (
              <ol className="req-timeline">
                {[...r.timeline].reverse().map((t, i) => (
                  <li key={`${t.at}-${i}`}>
                    <span className="req-timeline-dot" aria-hidden="true" />
                    <span className="req-timeline-text">{t.actor && <strong>{t.actor} </strong>}{t.text}</span>
                    <time dateTime={t.at} title={when(t.at)}>{timeAgo(t.at)}</time>
                  </li>
                ))}
              </ol>
            )}
          </section>
        </div>
        <aside className="req-aside" aria-labelledby="req-steps-title">
          <h2 id="req-steps-title">Approval steps</h2>
          <Stepper steps={r.steps} />
        </aside>
      </div>

      <CancelDialog isOpen={cancelling} onClose={() => setCancelling(false)} id={r.id} onDone={() => { setCancelling(false); setNotice(`${r.display_id} was cancelled.`); if (submitted) navigate(`/requests/${r.id}`, { replace: true }); reload(); }} />
    </div>
  );
}

const STATE_WORD: Record<Step['state'], string> = { done: 'Done', active: 'Current step', waiting: 'Still to come', skipped: 'Skipped', cancelled: 'Cancelled' };

/** The steps top to bottom. State is in words (visually hidden) and in the marker's shape, never colour alone. */
export function Stepper({ steps }: { steps: Step[] }) {
  if (steps.length === 0) return <p className="muted">This request has no approval steps.</p>;
  return (
    <ol className="steps" aria-label="Approval steps">
      {steps.map((s, i) => {
        const due = s.state === 'active' ? dueInfo(s.due_at) : null;
        return (
          <li key={`${s.name}-${i}`} className={`step step-${s.state}`} aria-current={s.state === 'active' ? 'step' : undefined}>
            <span className="step-mark" aria-hidden="true">{s.state === 'done' ? <Check size="0.75rem" strokeWidth={3} /> : s.state === 'skipped' || s.state === 'cancelled' ? <Minus size="0.75rem" strokeWidth={3} /> : null}</span>
            <div className="step-body">
              <p className="step-name"><span className="visually-hidden">{STATE_WORD[s.state]}: </span>{s.name}</p>
              {s.state === 'done' ? (
                <p className="step-meta">{s.outcome ? outcomePast(s.outcome) : 'Done'}{s.decided_by ? ` by ${s.decided_by}` : ''}{s.decided_at && <> · <time dateTime={s.decided_at}>{when(s.decided_at)}</time></>}</p>
              ) : s.state === 'active' ? (
                <p className="step-meta">{s.assignees.length ? `Waiting for ${people(s.assignees)}` : 'Waiting to be picked up'}{due && <> <Tag color={due.color} dot={due.overdue}>{due.label}</Tag></>}</p>
              ) : s.state === 'waiting' ? (
                <p className="step-meta">{s.assignees.length ? `Then ${people(s.assignees)}` : 'Not started'}</p>
              ) : <p className="step-meta">{STATE_WORD[s.state]}</p>}
              {s.comment && <blockquote className="step-comment">{s.comment}</blockquote>}
            </div>
          </li>
        );
      })}
    </ol>
  );
}

function CancelDialog({ isOpen, onClose, id, onDone }: { isOpen: boolean; onClose: () => void; id: string; onDone: () => void }) {
  const [reason, setReason] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (isOpen) { setReason(''); setError(null); setProblem(null); } }, [isOpen]);
  const go = async () => {
    if (!reason.trim()) { setError('Say why, so the approvers know.'); return; }
    setBusy(true);
    try { await api(`/api/v1/requests/${id}/cancel`, { method: 'POST', headers: { 'idempotency-key': crypto.randomUUID() }, body: { reason: reason.trim() } }); onDone(); }
    catch (e) { setProblem(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'The request could not be cancelled. Try again.'); }
    finally { setBusy(false); }
  };
  return (
    <FormDialog isOpen={isOpen} onOpenChange={(o) => !o && onClose()} title="Cancel this request?" description="It stops where it is. Approvers are told, and the request stays in the record as cancelled."
      footer={<><Button variant="secondary" onPress={onClose}>Keep it</Button><Button variant="danger" isDisabled={busy} onPress={() => void go()}><Ban aria-hidden="true" size="0.9375rem" /> {busy ? 'Cancelling…' : 'Cancel request'}</Button></>}>
      {problem && <Notice tone="danger" title={problem} />}
      <Form validationBehavior="aria" validationErrors={error ? { reason: error } : {}} onSubmit={(e) => { e.preventDefault(); void go(); }}>
        <AriaTextField name="reason" className="field" isRequired value={reason} onChange={(v) => { setReason(v); setError(null); }} autoFocus>
          <Label className="field-label">Reason<span aria-hidden="true" className="field-required"> *</span></Label>
          <TextArea className="field-input req-textarea" rows={3} />
          <FieldError className="field-error" />
        </AriaTextField>
      </Form>
    </FormDialog>
  );
}
