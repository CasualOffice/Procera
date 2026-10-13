import { Ban, BellRing, Download, Users } from 'lucide-react';
import { useMemo, useState } from 'react';
import { Form, Label, TextArea, TextField as AriaTextField } from 'react-aria-components';
import { api, download } from '../../lib/api';
import { useResource } from '../../lib/data';
import { shortDate } from '../../lib/format';
import { useRoute } from '../../lib/router';
import { useSession } from '../../lib/session';
import { type Batch, type EnvelopeState, STATE_COLOR, STATE_LABEL } from '../../lib/signing';
import { setCrumb } from '../../ui/AppShell';
import { Button } from '../../ui/components/Button';
import { FormDialog } from '../../ui/components/FormDialog';
import { Notice } from '../../ui/components/Notice';
import { LoadError, Skeleton } from '../../ui/States';
import { Tag } from '../../ui/Tag';
import { problemText } from './shared';
import './sender.css';

type Filter = 'all' | 'waiting' | 'signed' | 'other';
const ROW_LABEL: Partial<Record<EnvelopeState, string>> = { sent: 'Waiting', completed: 'Signed' };

/**
 * A group send (BPMS-107, docs/05 §3.22/§3.24): how many have signed, each person's copy and where
 * it stands, reminders for everyone still to sign, and cancelling the copies still waiting.
 */
export function BatchPage({ id }: { id: string }) {
  const { query } = useRoute();
  const { membershipId, can } = useSession();
  const [b, reload, error] = useResource<Batch>(`/api/v1/envelope-batches/${id}`);
  const [filter, setFilter] = useState<Filter>('all');
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string } | null>(
    query.get('sent') ? { tone: 'success', title: 'Sent. Each person is asked to sign their own copy.' } : null,
  );
  const [voiding, setVoiding] = useState(false);
  if (b) setCrumb(b.title);

  const rows = useMemo(() => (b?.rows ?? []).filter((r) =>
    filter === 'all' ? true : filter === 'waiting' ? r.state === 'sent' : filter === 'signed' ? r.state === 'completed' : !['sent', 'completed'].includes(r.state)), [b, filter]);

  if (error) return <LoadError error={error} onRetry={reload} what="this group send" />;
  if (!b) return <Skeleton variant="doc" label="Loading the group send" />;
  const c = b.counts;
  const mine = b.created_by === membershipId;
  const pct = c.total ? Math.round((c.signed / c.total) * 100) : 0;
  const remind = async () => {
    try {
      const r = await api<{ reminded: number }>(`/api/v1/envelope-batches/${id}/remind`, { method: 'POST' });
      setNotice({ tone: 'success', title: r.reminded ? `Reminded ${r.reminded} ${r.reminded === 1 ? 'person' : 'people'}.` : 'Everyone still to sign was reminded recently. You can remind them again an hour later.' });
      reload();
    } catch (e) { setNotice({ tone: 'danger', title: problemText(e, 'Couldn’t send the reminders.') }); }
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{b.title}</h1>
          <p>Sent to {c.total} {c.total === 1 ? 'person' : 'people'} by {b.sender_name} on {shortDate(b.created_at)}{b.expires_at ? ` · signatures close ${shortDate(b.expires_at)}` : ''}</p>
        </div>
        <div className="page-actions">
          {mine && c.waiting > 0 && <Button variant="secondary" onPress={() => void remind()}><BellRing aria-hidden="true" size="0.9375rem" /> Remind everyone waiting</Button>}
          {c.waiting > 0 && (mine || can('document:void')) && <Button variant="secondary" onPress={() => setVoiding(true)}><Ban aria-hidden="true" size="0.9375rem" /> Cancel the rest…</Button>}
        </div>
      </div>
      {notice && <Notice tone={notice.tone} title={notice.title} />}

      <section className="batch-progress card" aria-label="Progress">
        <div className="batch-progress-head">
          <p className="batch-big"><strong>{c.signed}</strong> of {c.total} signed</p>
          <span className="muted">{pct}%</span>
        </div>
        <div className="batch-bar" role="progressbar" aria-label="Signed" aria-valuemin={0} aria-valuemax={c.total} aria-valuenow={c.signed}>
          <span style={{ width: `${pct}%` }} />
        </div>
        <ul className="batch-counts">
          <li><span className="dot dot-blue" aria-hidden="true" />{c.waiting} waiting{c.viewed ? ` (${c.viewed} opened it)` : ''}</li>
          {c.declined > 0 && <li><span className="dot dot-red" aria-hidden="true" />{c.declined} declined</li>}
          {c.expired > 0 && <li><span className="dot dot-orange" aria-hidden="true" />{c.expired} expired</li>}
          {c.cancelled > 0 && <li><span className="dot dot-gray" aria-hidden="true" />{c.cancelled} cancelled</li>}
        </ul>
      </section>

      <div className="toolbar">
        <div className="segmented" role="group" aria-label="Show">
          {([['all', `Everyone (${c.total})`], ['waiting', `Waiting (${c.waiting})`], ['signed', `Signed (${c.signed})`], ['other', `Other (${c.total - c.waiting - c.signed})`]] as const).map(([k, l]) => (
            <button key={k} type="button" aria-pressed={filter === k} onClick={() => setFilter(k)}>{l}</button>
          ))}
        </div>
      </div>
      <div className="table-wrap">
        <table className="table" aria-label="People in this send">
          <thead><tr><th>Person</th><th className="col-optional">Department</th><th>Status</th><th className="col-optional">Reminders</th><th aria-label="Signed copy" /></tr></thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.envelope_id}>
                <td><span className="cell-stack"><span>{r.name}</span><span className="cell-sub">{r.email}</span></span></td>
                <td className="col-optional cell-muted">{r.department ?? '—'}</td>
                <td>
                  <Tag color={STATE_COLOR[r.state]} dot>{ROW_LABEL[r.state] ?? STATE_LABEL[r.state]}</Tag>
                  <span className="cell-sub batch-when">
                    {r.state === 'completed' && r.signed_at ? shortDate(r.signed_at) : r.state === 'sent' && r.viewed_at ? `Opened ${shortDate(r.viewed_at)}` : r.decline_reason ?? ''}
                  </span>
                </td>
                <td className="col-optional cell-muted">{r.reminders_sent || '—'}</td>
                <td className="cell-actions">
                  {r.state === 'completed' && (
                    <Button variant="ghost" className="btn-sm" aria-label={`Download ${r.name}’s signed copy`}
                      onPress={() => void download(`/api/v1/envelopes/${r.envelope_id}/document?signed=true`, `${b.title} - ${r.name} (signed).pdf`)}>
                      <Download aria-hidden="true" size="0.875rem" />
                    </Button>
                  )}
                </td>
              </tr>
            ))}
            {rows.length === 0 && <tr><td colSpan={5} className="cell-muted"><Users aria-hidden="true" size="0.875rem" /> Nobody here.</td></tr>}
          </tbody>
        </table>
      </div>
      {voiding && <CancelDialog id={id} title={b.title} waiting={c.waiting} onClose={() => setVoiding(false)}
        onDone={(n) => { setVoiding(false); setNotice({ tone: 'success', title: `Cancelled ${n} ${n === 1 ? 'copy' : 'copies'} still waiting. Signed copies are kept.` }); reload(); }} />}
    </>
  );
}

function CancelDialog({ id, title, waiting, onClose, onDone }: { id: string; title: string; waiting: number; onClose: () => void; onDone: (n: number) => void }) {
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const go = async () => {
    setBusy(true); setErr(null);
    try { onDone((await api<{ cancelled: number }>(`/api/v1/envelope-batches/${id}/void`, { method: 'POST', body: { reason: reason.trim() } })).cancelled); }
    catch (e) { setErr(problemText(e, 'Couldn’t cancel. Try again.')); setBusy(false); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && !busy && onClose()} title={`Cancel the ${waiting} unsigned ${waiting === 1 ? 'copy' : 'copies'} of “${title}”?`} width={500}
      description="People who haven’t signed are told it no longer needs their signature. Signed copies stay."
      footer={<><Button variant="secondary" onPress={onClose} isDisabled={busy}>Keep them</Button><Button variant="danger" onPress={() => void go()} isDisabled={busy || !reason.trim()}>Cancel signing</Button></>}>
      {err && <Notice tone="danger" title={err} />}
      <Form onSubmit={(e) => { e.preventDefault(); void go(); }}>
        <AriaTextField className="field field-wide" value={reason} onChange={setReason} maxLength={1000} isRequired autoFocus>
          <Label className="field-label">Reason</Label>
          <TextArea className="field-input sign-textarea" rows={3} />
        </AriaTextField>
      </Form>
    </FormDialog>
  );
}
