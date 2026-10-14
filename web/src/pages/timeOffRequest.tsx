import { CalendarCheck, CircleAlert } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Form, Label, TextArea, TextField as AriaTextField } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { fieldErrors } from '../lib/requests';
import { Link } from '../lib/router';
import { Button } from '../ui/components/Button';
import { Checkbox } from '../ui/components/Checkbox';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { type Balance, daysText } from './timeOffShared';

export interface LeaveRequest {
  request_id: string; display_id: string; leave_type: { id: string; key: string; name: string; colour: string; sensitive: boolean };
  start_date: string; start_half: string; end_date: string; end_half: string; days: number; state: 'pending' | 'approved' | 'rejected' | 'cancelled'; created_at: string;
}
interface Line { date: string; why: { kind: string; name?: string }; halves: number }
interface Quote { days: number; lines: Line[]; available: number | null; after: number | null; problems: { field: string; code: string; message: string }[] }

const day = (d: string, o: Intl.DateTimeFormatOptions = { weekday: 'short', day: 'numeric', month: 'short' }) => new Date(`${d}T00:00:00Z`).toLocaleDateString(undefined, { ...o, timeZone: 'UTC' });
const iso = (d: Date) => d.toISOString().slice(0, 10);
const WHY: Record<string, string> = { weekly_off: 'weekend', holiday: 'holiday', rostered_off: 'day off', working: 'not an optional holiday' };

/** "Mon 5 Oct – Wed 7 Oct", "Mon 5 Oct (afternoon)". */
export function rangeText(r: Pick<LeaveRequest, 'start_date' | 'start_half' | 'end_date' | 'end_half'>) {
  const a = `${day(r.start_date)}${r.start_half === 'second_half' ? ' (afternoon)' : ''}`;
  if (r.start_date === r.end_date) return r.end_half === 'first_half' ? `${day(r.start_date)} (morning)` : a;
  return `${a} – ${day(r.end_date)}${r.end_half === 'first_half' ? ' (morning)' : ''}`;
}

const STATE: Record<LeaveRequest['state'], [string, string]> = {
  pending: ['Waiting for approval', 'tag-yellow'], approved: ['Approved', 'tag-green'], rejected: ['Not approved', 'tag-red'], cancelled: ['Cancelled', 'tag-gray'],
};

export function MyRequests({ items }: { items: LeaveRequest[] }) {
  if (!items.length) return null;
  return (
    <section className="card to-reqs" aria-labelledby="to-reqs-h">
      <div className="card-head"><h2 id="to-reqs-h">My requests</h2></div>
      <ul className="to-req-list">
        {items.map((r) => (
          <li key={r.request_id} className={`bal-${r.leave_type.colour}`}>
            <span className="bal-dot" aria-hidden="true" />
            <span className="to-req-main">
              <Link to={`/requests/${r.request_id}`} className="to-req-title">{r.leave_type.name}: {rangeText(r)}</Link>
              <span className="to-req-sub">{daysText(r.days)} · {r.display_id}</span>
            </span>
            <span className={`tag ${STATE[r.state][1]}`}>{STATE[r.state][0]}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Request time off (BPMS-224): pick a type and dates; Procera counts the days as you go. */
export function RequestTimeOff({ isOpen, onClose, balances, onDone }: { isOpen: boolean; onClose: () => void; balances: Balance[]; onDone: (msg: string) => void }) {
  const types = balances.map((b) => b.leave_type);
  const today = iso(new Date());
  const [type, setType] = useState(types[0]?.key ?? '');
  const [start, setStart] = useState(today);
  const [end, setEnd] = useState(today);
  const [afternoon, setAfternoon] = useState(false);
  const [morning, setMorning] = useState(false);
  const [reason, setReason] = useState('');
  const [quote, setQuote] = useState<Quote | null>(null);
  const [errs, setErrs] = useState<Record<string, string>>({});
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const single = start === end;
  const body = useMemo(() => ({
    leave_type: type, start_date: start, end_date: end < start ? start : end,
    ...(afternoon ? { start_half: 'second_half' } : {}), ...(morning && !(single && afternoon) ? { end_half: 'first_half' } : {}),
  }), [type, start, end, afternoon, morning, single]);
  useEffect(() => {
    if (!isOpen || !type || !start) return;
    let live = true;
    const t = setTimeout(() => {
      api<Quote>('/api/v1/time-off/quote', { method: 'POST', body }).then((q) => { if (live) setQuote(q); }).catch(() => { if (live) setQuote(null); });
    }, 200);
    return () => { live = false; clearTimeout(t); };
  }, [body, isOpen, type, start]);
  const submit = async () => {
    setBusy(true); setErr(null); setErrs({});
    try {
      await api('/api/v1/requests', { method: 'POST', headers: { 'idempotency-key': crypto.randomUUID() }, body: { type: 'hr.leave-request', input: { ...body, ...(reason.trim() ? { reason } : {}) } } });
      onDone(`Requested ${daysText(quote?.days ?? 0)} of ${types.find((x) => x.key === type)?.name ?? 'leave'}. Your manager has been asked.`);
      setReason(''); onClose();
    } catch (e) {
      const fe = fieldErrors(e);
      if (Object.keys(fe).length) setErrs(fe); else setErr(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Couldn’t send it.');
    } finally { setBusy(false); }
  };
  const t = balances.find((b) => b.leave_type.key === type);
  const skipped = (quote?.lines ?? []).filter((l) => l.halves === 0);
  const problems = quote?.problems ?? [];
  return (
    <FormDialog isOpen={isOpen} onOpenChange={(o) => { if (!o) onClose(); }} title="Request time off" description="Your manager is asked to approve it. The days are held from your balance until then." width={600}
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()} isDisabled={busy || !type || problems.length > 0}>{busy ? 'Sending…' : 'Send request'}</Button></>}>
      <Form className="stack" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
        {err && <Notice tone="danger" title={err} />}
        <Select label="Type of leave" selectedKey={type} onSelectionChange={(k) => setType(String(k))} options={types.map((x) => ({ id: x.key, label: x.name }))} errorMessage={errs.leave_type} isInvalid={!!errs.leave_type} />
        <div className="to-dates">
          <div className="field">
            <label className="field-label" htmlFor="to-start">First day</label>
            <input id="to-start" type="date" className="field-input" value={start} onChange={(e) => { setStart(e.target.value); if (end < e.target.value) setEnd(e.target.value); }} />
            <Checkbox isSelected={afternoon} onChange={setAfternoon}>Start in the afternoon</Checkbox>
          </div>
          <div className="field">
            <label className="field-label" htmlFor="to-end">Last day</label>
            <input id="to-end" type="date" className="field-input" value={end} min={start} onChange={(e) => setEnd(e.target.value)} />
            <Checkbox isSelected={morning} onChange={setMorning} isDisabled={single && afternoon}>End at midday</Checkbox>
          </div>
        </div>
        <div className="to-quote" aria-live="polite">
          {quote && (
            <>
              <p className="to-quote-main"><CalendarCheck aria-hidden="true" size="1rem" /> <strong>{daysText(quote.days)}</strong>
                {t?.leave_type.limited && quote.available !== null && quote.after !== null && <span className="muted"> · {daysText(quote.available)} available, {daysText(quote.after)} after</span>}
              </p>
              {skipped.length > 0 && <p className="to-quote-skip muted">Not counted: {skipped.slice(0, 6).map((l) => `${day(l.date)} (${l.why.name ?? WHY[l.why.kind] ?? l.why.kind})`).join(', ')}{skipped.length > 6 ? ` and ${skipped.length - 6} more` : ''}</p>}
              {problems.length > 0 && (
                <ul className="to-problems" aria-label="Problems">
                  {problems.map((p) => <li key={p.code}><CircleAlert aria-hidden="true" size="0.875rem" />{p.message}</li>)}
                </ul>
              )}
            </>
          )}
        </div>
        <AriaTextField className="field" value={reason} onChange={setReason}>
          <Label className="field-label">Note for your manager <span className="muted">(optional)</span></Label>
          <TextArea className="field-input" rows={2} />
        </AriaTextField>
        {Object.entries(errs).filter(([k]) => k !== 'leave_type').map(([k, v]) => <Notice key={k} tone="danger" title={v} />)}
      </Form>
    </FormDialog>
  );
}
