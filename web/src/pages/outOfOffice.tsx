import { CalendarOff } from 'lucide-react';
import { useMemo, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type Member, useItems, useResource } from '../lib/data';
import { fieldErrors } from '../lib/requests';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { Tag } from '../ui/Tag';

interface Person { id: string; name: string }
export interface OutOfOffice { member: Person; delegate: Person; starts_on: string; ends_on: string; note: string | null; active: boolean; set_by: Person }

/** "Fri 16 Oct" for an ISO date, read as a calendar day (no time zone shift). */
export const dayText = (iso: string) => new Date(`${iso}T00:00:00`).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
const today = () => new Date().toISOString().slice(0, 10);

/**
 * Out of office (BPMS-086, docs/12 §9): dates and a delegate who decides requests meanwhile. Shown on
 * Account › Profile for yourself (`/api/v1/me/out-of-office`) and on the Person page for admins and
 * managers (`/api/v1/members/{id}/out-of-office`). Hidden when the caller can't read it.
 */
export function OutOfOfficeSection({ memberId, name }: { memberId?: string; name?: string }) {
  const base = memberId ? `/api/v1/members/${memberId}/out-of-office` : '/api/v1/me/out-of-office';
  const [data, reload, error] = useResource<{ out_of_office: OutOfOffice | null; can_set: boolean }>(base);
  const [editing, setEditing] = useState(false);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string } | null>(null);
  if (error || !data) return null;
  const o = data.out_of_office;
  const you = !memberId;
  const end = async () => {
    try { await api(base, { method: 'DELETE' }); setNotice({ tone: 'success', title: 'Out of office ended.' }); reload(); }
    catch (e) { setNotice({ tone: 'danger', title: e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not end it. Try again.' }); }
  };
  return (
    <section className="account-section" aria-labelledby="ooo-title">
      <div className="section-head"><h2 id="ooo-title">Out of office</h2></div>
      {notice && <Notice tone={notice.tone} title={notice.title} />}
      <div className="card form-card">
        {o ? (
          <>
            <p className="ooo-state">
              <Tag color={o.active ? 'orange' : 'gray'} dot={o.active}>{o.active ? 'Away' : 'Scheduled'}</Tag>{' '}
              {you ? 'You’re' : `${name ?? o.member.name} is`} away from <strong>{dayText(o.starts_on)}</strong> to <strong>{dayText(o.ends_on)}</strong>. <strong>{o.delegate.name}</strong> decides {you ? 'your' : 'their'} requests meanwhile.
            </p>
            {o.note && <p className="muted">“{o.note}”</p>}
            {o.set_by.id !== o.member.id && <p className="muted">Set by {o.set_by.name}.</p>}
          </>
        ) : (
          <p className="muted"><CalendarOff aria-hidden="true" size="0.9375rem" /> {you ? 'Going away? Choose someone to decide your requests while you’re gone. New requests go to them, and they can decide the ones already waiting for you.' : `${name ?? 'They'} has no out of office set.`}</p>
        )}
        {data.can_set ? (
          <div className="form-actions">
            <Button variant="secondary" onPress={() => setEditing(true)}>{o ? 'Change out of office' : 'Set out of office'}</Button>
            {o && <Button variant="ghost" onPress={() => void end()}>End out of office</Button>}
          </div>
        ) : you && <p className="muted">Ask an admin or your manager to set it for you.</p>}
      </div>
      {editing && <OutOfOfficeDialog base={base} current={o} you={you} self={memberId} onClose={() => setEditing(false)} onSaved={() => { setEditing(false); setNotice({ tone: 'success', title: 'Out of office saved.' }); reload(); }} />}
    </section>
  );
}

function OutOfOfficeDialog({ base, current, you, self, onClose, onSaved }: { base: string; current: OutOfOffice | null; you: boolean; self?: string; onClose: () => void; onSaved: () => void }) {
  const [members] = useItems<Member>('/api/v1/members');
  const [from, setFrom] = useState(current?.starts_on ?? today());
  const [to, setTo] = useState(current?.ends_on ?? today());
  const [delegate, setDelegate] = useState<string | null>(current?.delegate.id ?? null);
  const [note, setNote] = useState(current?.note ?? '');
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { membershipId } = useSession();
  const exclude = self ?? membershipId;
  const options = useMemo(() => (members ?? []).filter((m) => m.status === 'active' && m.id !== exclude).map((m) => ({ id: m.id, label: m.display_name })), [members, exclude]);
  const save = async () => {
    if (!delegate) { setErrors({ delegate: 'Choose who decides meanwhile.' }); return; }
    setBusy(true); setProblem(null); setErrors({});
    try {
      await api(base, { method: 'PUT', body: { starts_on: from, ends_on: to, delegate, ...(note.trim() ? { note: note.trim() } : {}) } });
      onSaved();
    } catch (e) {
      const f = fieldErrors(e);
      if (Object.keys(f).length) setErrors(f);
      else setProblem(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not save. Try again.');
    } finally { setBusy(false); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Out of office" description={you ? 'While you’re away, new requests go to your delegate, and they can decide the ones already waiting for you.' : 'While they’re away, new requests go to the delegate, who can also decide the ones already waiting.'}
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void save()} isDisabled={busy}>{busy ? 'Saving…' : 'Save out of office'}</Button></>}>
      {problem && <Notice tone="danger" title={problem} />}
      <div className="field">
        <label className="field-label" htmlFor="ooo-from">First day away</label>
        <input id="ooo-from" type="date" className="field-input" value={from} onChange={(e) => setFrom(e.target.value)} aria-invalid={!!errors.starts_on} aria-describedby={errors.starts_on ? 'ooo-from-e' : undefined} />
        {errors.starts_on && <span id="ooo-from-e" className="field-error">{errors.starts_on}</span>}
      </div>
      <div className="field">
        <label className="field-label" htmlFor="ooo-to">Last day away</label>
        <input id="ooo-to" type="date" className="field-input" value={to} min={from} onChange={(e) => setTo(e.target.value)} aria-invalid={!!errors.ends_on} aria-describedby={errors.ends_on ? 'ooo-to-e' : undefined} />
        {errors.ends_on && <span id="ooo-to-e" className="field-error">{errors.ends_on}</span>}
      </div>
      <Select label="Delegate" description="They decide requests meanwhile. Separation of duties still applies to them." options={options} selectedKey={delegate} onSelectionChange={(k) => setDelegate(k ? String(k) : null)}
        isInvalid={!!errors.delegate} errorMessage={errors.delegate} isRequired />
      <div className="field">
        <label className="field-label" htmlFor="ooo-note">Note <span className="muted">(optional)</span></label>
        <input id="ooo-note" className="field-input" maxLength={500} value={note} onChange={(e) => setNote(e.target.value)} />
        <span className="field-description">For example, who to contact for urgent questions.</span>
      </div>
    </FormDialog>
  );
}
