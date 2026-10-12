import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { type WorkItem, fieldErrors } from '../lib/requests';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { TextField } from '../ui/components/TextField';

/**
 * Reassign one step or several to one person with a reason (BPMS-088, FR-A03 bulk reassign). The
 * dialog previews what moves; each step is sent on its own, so separation of duties is checked per
 * step. Refusals are listed by request and the rest still move. `exclude` hides a person (the
 * current holder) from the picker; `only` limits it (a manager moves steps within their team).
 */
export function ReassignDialog({ items, exclude, only, onClose, onDone }: { items: WorkItem[]; exclude?: string; only?: string[]; onClose: () => void; onDone: (message: string) => void }) {
  const { can } = useSession();
  const open = items.length > 0;
  const key = items.map((i) => i.id).join(',');
  const [members] = useItems<Member>(open && can('member:view') ? '/api/v1/members' : null, [key]);
  const [who, setWho] = useState('');
  const [reason, setReason] = useState('');
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [failure, setFailure] = useState<{ title: string; details?: string[] } | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { setWho(''); setReason(''); setErrors({}); setFailure(null); }, [key]);
  const options = (members ?? []).filter((m) => m.status === 'active' && m.id !== exclude && (!only || only.includes(m.id))).map((m) => ({ id: m.id, label: `${m.display_name} (${m.email})` }));
  const one = items.length === 1 ? items[0] : null;
  const submit = async () => {
    const missing: Record<string, string> = {};
    if (!who) missing.membership_id = items.length > 1 ? 'Choose who should take these steps.' : 'Choose who should take this step.';
    if (!reason.trim()) missing.reason = 'Say why, for the audit log and the people concerned.';
    setErrors(missing); setFailure(null);
    if (Object.keys(missing).length) return;
    setBusy(true);
    const refused: string[] = [];
    let moved = 0;
    let name = '';
    for (const item of items) {
      try {
        const r = await api<{ assignees: string[] }>(`/api/v1/work/${item.id}/reassign`, { method: 'POST', body: { membership_id: who, reason: reason.trim() } });
        name = r.assignees.join(', '); moved += 1;
      } catch (e) {
        const fe = fieldErrors(e);
        if (one && Object.keys(fe).length) { setErrors(fe); setBusy(false); return; }
        const why = e instanceof ApiError && e.code === 'already_decided' ? 'someone decided it already'
          : fe.membership_id ?? (e instanceof Error ? e.message : 'couldn’t be moved');
        refused.push(`${item.request.display_id}: ${why}`);
      }
    }
    setBusy(false);
    if (refused.length === 0) {
      onDone(one ? `${one.request.display_id} is now with ${name}.` : `${moved} steps are now with ${name}.`);
    } else if (moved > 0) {
      setFailure({ title: `${moved} moved to ${name}; ${refused.length} couldn’t be.`, details: refused });
    } else {
      setFailure({ title: refused.length === 1 ? 'The step couldn’t be moved.' : 'None of the steps could be moved.', details: refused });
    }
  };
  return (
    <FormDialog isOpen={open} onOpenChange={(o) => !o && onClose()} title={items.length > 1 ? `Reassign ${items.length} steps` : 'Reassign step'}
      description={one ? `${one.step} of ${one.request.display_id} (${one.request.title}). The new person is notified; the change is recorded in the audit log and the request’s timeline.`
        : 'The new person is notified about each step; every move is recorded in the audit log and the request’s timeline.'}
      footer={<><Button variant="secondary" onPress={onClose}>{failure ? 'Close' : 'Cancel'}</Button><Button onPress={() => void submit()} isDisabled={busy}>{busy ? 'Reassigning…' : 'Reassign'}</Button></>}>
      {failure && <Notice tone="danger" title={failure.title}>{failure.details && <ul className="reassign-refused">{failure.details.map((d) => <li key={d}>{d}</li>)}</ul>}</Notice>}
      <div className="stack">
        {items.length > 1 && (
          <ul className="reassign-preview" aria-label="Steps to reassign">
            {items.map((i) => <li key={i.id}><strong>{i.request.display_id}</strong> {i.step} · <span className="muted">{i.request.title}</span></li>)}
          </ul>
        )}
        <Select label="Person" isRequired options={options} selectedKey={who || null} onSelectionChange={(k) => setWho(String(k ?? ''))}
          placeholder={members === null ? 'Loading people…' : 'Choose a person'} isInvalid={!!errors.membership_id} errorMessage={errors.membership_id}
          description="The person who started a request, and anyone separation of duties excludes, can’t take it." />
        <TextField label="Reason" isRequired value={reason} onChange={setReason} isInvalid={!!errors.reason} errorMessage={errors.reason}
          description="For example “Manager on leave until Monday”." />
      </div>
    </FormDialog>
  );
}
