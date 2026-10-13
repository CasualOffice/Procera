import { useMemo, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { type WorkItem, fieldErrors, workChanged } from '../lib/requests';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { TextField } from '../ui/components/TextField';

/**
 * Hand a step you hold to a colleague (BPMS-212): one person and an optional note. The server
 * re-checks separation of duties; a refusal shows on the person field.
 */
export function DelegateDialog({ item, onClose, onDone }: { item: WorkItem; onClose: () => void; onDone: () => void }) {
  const { membershipId } = useSession();
  const [members] = useItems<Member>('/api/v1/members');
  const [who, setWho] = useState('');
  const [note, setNote] = useState('');
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const options = useMemo(() => (members ?? [])
    .filter((m) => m.status === 'active' && m.id !== membershipId && m.id !== item.request.initiator.id)
    .map((m) => ({ id: m.id, label: m.title ? `${m.display_name} · ${m.title}` : m.display_name })), [members, membershipId, item.request.initiator.id]);
  const submit = async () => {
    if (!who) { setErrors({ membership_id: 'Choose who takes this step.' }); return; }
    setBusy(true); setErrors({}); setFailure(null);
    try {
      await api(`/api/v1/work/${item.id}/delegate`, { method: 'POST', body: { membership_id: who, ...(note.trim() ? { note: note.trim() } : {}) } });
      workChanged();
      onDone();
    } catch (e) {
      const f = fieldErrors(e);
      if (Object.keys(f).length) setErrors(f);
      else setFailure(e instanceof ApiError && e.code === 'already_decided' ? 'Someone decided this step already.' : e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Couldn’t delegate it. Try again.');
    } finally { setBusy(false); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Delegate this step"
      description={`${item.step} of ${item.request.display_id}. It leaves your list and goes to them; they’re told, and the request’s timeline records it.`}
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()} isDisabled={busy}>{busy ? 'Delegating…' : 'Delegate'}</Button></>}>
      {failure && <Notice tone="danger" title={failure} />}
      <div className="stack">
        <Select label="Delegate to" isRequired options={options} selectedKey={who || null} onSelectionChange={(k) => setWho(String(k ?? ''))}
          placeholder={members === null ? 'Loading people…' : 'Choose a person'} isDisabled={members === null} isInvalid={!!errors.membership_id} errorMessage={errors.membership_id}
          description="Separation of duties still applies: someone who decided an earlier step can’t take it." />
        <TextField label="Note" value={note} onChange={setNote} isInvalid={!!errors.note} errorMessage={errors.note} placeholder="For example “I’m at the offsite until Friday”" description="Optional. They see it in the timeline." />
      </div>
    </FormDialog>
  );
}
