import { Check, CircleAlert, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Form, Label, TextArea, TextField as AriaTextField } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { shortDate } from '../lib/format';
import { Link, useRoute } from '../lib/router';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { Skeleton } from '../ui/States';

interface Action {
  state: 'open' | 'used' | 'expired' | 'closed'; deciding_as: string; workspace: string; step: string;
  request: { id: string; display_id: string; title: string; type_name: string | null; initiator: string; submitted_at: string; due_at: string | null };
  answers: { label: string; type: string; value: unknown; currency?: string | null }[]; expires_at: string;
}

function show(a: Action['answers'][number]): string {
  const v = a.value as unknown;
  if (v && typeof v === 'object' && 'amount' in (v as Record<string, unknown>)) {
    const m = v as { amount: string; currency?: string };
    return `${Number(m.amount).toLocaleString(undefined, { maximumFractionDigits: 2 })} ${m.currency ?? a.currency ?? ''}`.trim();
  }
  if (typeof v === 'boolean') return v ? 'Yes' : 'No';
  if (a.type === 'date' && typeof v === 'string') return shortDate(v);
  if (Array.isArray(v)) return `${v.length} ${v.length === 1 ? 'file' : 'files'}`;
  if (a.type === 'enum' && typeof v === 'string') return (v.charAt(0).toUpperCase() + v.slice(1)).replace(/_/g, ' ');
  return String(v ?? '—');
}

/**
 * Decide from email (BPMS-145, FR-N03, docs/05 §8): the single-use link from an approval email lands
 * here. Nothing is decided until the person confirms; the page says who they're deciding as. Works
 * signed out: the link is the credential, once.
 */
export function EmailApprove({ token }: { token: string }) {
  const { query } = useRoute();
  const [a, setA] = useState<Action | null>(null);
  const [missing, setMissing] = useState(false);
  const [outcome, setOutcome] = useState<'approved' | 'rejected'>(query.get('outcome') === 'rejected' ? 'rejected' : 'approved');
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);

  useEffect(() => {
    api<Action>(`/api/v1/email-actions/${encodeURIComponent(token)}`).then(setA).catch(() => setMissing(true));
  }, [token]);

  const submit = async () => {
    setBusy(true); setErr(null);
    try {
      await api(`/api/v1/email-actions/${encodeURIComponent(token)}`, { method: 'POST', body: { outcome, comment: reason.trim() || null } });
      setDone(outcome);
    } catch (e) { setErr(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'That didn’t work. Try again or open it in Procera.'); }
    finally { setBusy(false); }
  };

  if (missing) return <><h1>This link doesn’t work</h1><Notice tone="warning" title="It may be mistyped, or it was replaced by a newer email.">Open Procera to see what’s waiting for you.</Notice><Link className="btn btn-primary btn-md link-btn btn-block" to="/work">Open My work</Link></>;
  if (!a) return <Skeleton variant="doc" label="Loading" />;
  const open = `/requests/${a.request.id}`;
  if (done) {
    return (
      <>
        <p className="eapprove-mark" aria-hidden="true">{done === 'approved' ? <Check size="1.5rem" /> : <X size="1.5rem" />}</p>
        <h1>{done === 'approved' ? 'Approved' : 'Rejected'} {a.request.display_id}</h1>
        <p className="lede">{a.request.initiator} is told. Nothing else to do.</p>
        <Link className="btn btn-secondary btn-md link-btn btn-block" to={open}>Open it in Procera</Link>
      </>
    );
  }
  return (
    <>
      <p className="eapprove-ws">{a.workspace}</p>
      <h1>{a.step}: {a.request.display_id}</h1>
      <p className="lede">{a.request.initiator} asked for {a.request.title}{a.request.type_name ? ` (${a.request.type_name.toLowerCase()})` : ''} on {shortDate(a.request.submitted_at)}.</p>
      {a.answers.length > 0 && (
        <dl className="eapprove-answers">
          {a.answers.map((x) => <div key={x.label}><dt>{x.label}</dt><dd>{show(x)}</dd></div>)}
        </dl>
      )}
      {a.state !== 'open' ? (
        <>
          <Notice tone="warning" title={a.state === 'used' ? 'This link has already been used.' : a.state === 'expired' ? 'This link has expired.' : 'This step has already been decided.'}>Open the request in Procera to see where it stands.</Notice>
          <Link className="btn btn-primary btn-md link-btn btn-block" to={open}>Open it in Procera</Link>
        </>
      ) : (
        <Form className="stack" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
          <div className="segmented eapprove-choice" role="radiogroup" aria-label="Your decision">
            <button type="button" role="radio" aria-checked={outcome === 'approved'} onClick={() => setOutcome('approved')}><Check aria-hidden="true" size="0.9375rem" /> Approve</button>
            <button type="button" role="radio" aria-checked={outcome === 'rejected'} onClick={() => setOutcome('rejected')}><X aria-hidden="true" size="0.9375rem" /> Reject</button>
          </div>
          {outcome === 'rejected' && (
            <AriaTextField className="field field-wide" value={reason} onChange={setReason} maxLength={2000} isRequired autoFocus>
              <Label className="field-label">Why are you rejecting it?</Label>
              <TextArea className="field-input" rows={3} />
            </AriaTextField>
          )}
          {err && <Notice tone="danger" title={err} />}
          <p className="eapprove-as"><CircleAlert aria-hidden="true" size="0.875rem" /><span>You’re deciding as <strong>{a.deciding_as}</strong>. This link works once.</span></p>
          <Button type="submit" className="btn-block" variant={outcome === 'approved' ? 'primary' : 'danger'} isDisabled={busy || (outcome === 'rejected' && !reason.trim())}>
            {busy ? 'Saving…' : outcome === 'approved' ? `Approve ${a.request.display_id}` : `Reject ${a.request.display_id}`}
          </Button>
          <Link className="eapprove-open" to={open}>Open it in Procera instead</Link>
        </Form>
      )}
    </>
  );
}
