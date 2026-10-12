import { Lock } from 'lucide-react';
import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../../lib/api';
import { useItems } from '../../lib/data';
import { sysCan } from '../../lib/system';
import { Button } from '../../ui/components/Button';
import { FormDialog } from '../../ui/components/FormDialog';
import { Notice } from '../../ui/components/Notice';
import { TextField } from '../../ui/components/TextField';
import { LoadError, Skeleton } from '../../ui/States';
import { useStepUp } from '../../ui/StepUp';
import { useSystem } from '../../ui/SystemShell';
import { Control, type Kind, show } from '../Settings';
import { CATEGORIES, anchorOf, isHidden, labelOf, placeOf } from '../settingsMeta';

type Comparator = 'smaller' | 'larger' | 'superset' | 'subset' | 'true';
/** One platform floor from `GET /sysapi/v1/settings`; `value` is null when no floor is set yet. */
export interface Floor { key: string; value: unknown; is_floor: boolean; revision: number; comparator: Comparator | null; kind: Kind; default: unknown }

const isSet = (f: Floor) => f.is_floor && f.value !== null;

/** What the floor means for every workspace, in plain words. */
export function meaning(f: Floor): string {
  if (!isSet(f)) return 'No floor. Workspaces can use any allowed value.';
  const v = show(f.kind, f.value);
  switch (f.comparator) {
    case 'larger': return `Workspaces can’t set less than ${v}.`;
    case 'smaller': return `Workspaces can’t set more than ${v}.`;
    case 'true': return f.value === true ? 'On in every workspace. Workspaces can’t turn it off.' : 'Workspaces decide for themselves.';
    case 'superset': return `Every workspace requires it for at least: ${v}.`;
    case 'subset': return `Workspaces can offer only: ${v}.`;
    default: return v;
  }
}

/**
 * System · Policy floors (docs/20 §1 layer 2, docs/12 §2): the minimums that bind every workspace,
 * grouped like the workspace Settings categories. A workspace can be stricter, never looser.
 * System admins edit them with a reason and a recent verification; auditors and operators read.
 */
export function SystemFloors() {
  const me = useSystem();
  const [items, reload, error] = useItems<Floor>('/sysapi/v1/settings');
  const [editing, setEditing] = useState<Floor | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const canEdit = sysCan(me.roles, 'platform.config');
  const floors = (items ?? []).filter((f) => f.comparator !== null && !isHidden(f.key));

  return (
    <>
      <div className="page-head"><div><h1>Policy floors</h1><p>Minimums every workspace must meet. A workspace can be stricter, never looser. Raising a floor applies right away; values a workspace saved earlier are kept but the floor wins.</p></div></div>
      {!canEdit && <Notice tone="info" title="Read only">Your system role can see the floors but not change them.</Notice>}
      {saved && <Notice tone="success" title={saved} />}
      {error ? <LoadError error={error} onRetry={reload} what="policy floors" /> : items === null ? <Skeleton variant="table" rows={6} label="Loading policy floors" /> : CATEGORIES.map((c) => {
        const group = floors.filter((f) => placeOf(f.key).cat === c.id);
        if (group.length === 0) return null;
        return (
          <section key={c.id} className="quota-section floor-section" aria-labelledby={`floors-${c.id}`}>
            <h2 id={`floors-${c.id}`}>{c.label}</h2>
            <div className="table-wrap">
              <table className="table" aria-label={`${c.label} floors`}>
                <thead><tr><th>Setting</th><th>Floor</th>{canEdit && <th><span className="visually-hidden">Actions</span></th>}</tr></thead>
                <tbody>
                  {group.map((f) => {
                    const [label, hint] = labelOf(f.key);
                    return (
                      <tr key={f.key} id={anchorOf(f.key)}>
                        <td>
                          <span className="floor-name">{label}</span>
                          <span className="floor-meaning">{meaning(f)}</span>
                          {hint && <span className="floor-hint">{hint}</span>}
                        </td>
                        <td className="floor-value">{isSet(f) ? show(f.kind, f.value) : <span className="cell-muted">Not set</span>}</td>
                        {canEdit && <td className="num"><Button variant="ghost" onPress={() => { setSaved(null); setEditing(f); }} aria-label={`Edit ${label} floor`}>Edit</Button></td>}
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </section>
        );
      })}
      {editing && <FloorDialog f={editing} onClose={() => setEditing(null)} onSaved={() => { setSaved(`${labelOf(editing.key)[0]}: floor saved`); setEditing(null); reload(); }} />}
    </>
  );
}

function FloorDialog({ f, onClose, onSaved }: { f: Floor; onClose: () => void; onSaved: () => void }) {
  const [label, hint] = labelOf(f.key);
  const [draft, setDraft] = useState<unknown>(isSet(f) ? f.value : f.default);
  const [reason, setReason] = useState('');
  const [error, setError] = useState<string | null>(null);
  const { guard, dialog } = useStepUp((e) => setError(
    e instanceof ApiError && e.code === 'precondition_failed' ? 'Someone else changed this floor. Close and reopen it to see the new value.'
      : e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not save. Try again.',
  ));
  const id = `floor-${f.key.replace(/\./g, '-')}`;
  const changed = !isSet(f) || JSON.stringify(draft) !== JSON.stringify(f.value);
  const submit = () => {
    if (!reason.trim() || !changed) return;
    setError(null);
    void guard(async () => {
      await api(`/sysapi/v1/settings/${f.key}`, { method: 'PUT', headers: { 'if-match': `"${f.revision}"` }, body: { value: draft, reason: reason.trim() } });
      onSaved();
    });
  };
  return (
    <>
      <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={`${label} floor`} width={520}
        description="Applies to every workspace right away, including people who are signed in. Workspaces with a looser value use the floor."
        footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={submit} isDisabled={!reason.trim() || !changed}>Save floor</Button></>}>
        {error && <Notice tone="danger" title={error} />}
        <Form className="quota-form" onSubmit={(e) => { e.preventDefault(); submit(); }}>
          <div className="field">
            <span className="field-label" id={`${id}-l`}>{label}</span>
            {hint && <span className="field-description" id={`${id}-h`}>{hint}</span>}
            <Control kind={f.kind} value={draft} onChange={setDraft} labelledBy={`${id}-l`} describedBy={hint ? `${id}-h` : undefined} />
            <span className="field-description">{meaning({ ...f, value: draft, is_floor: true })}</span>
          </div>
          <TextField label="Reason for the change" value={reason} onChange={setReason} description="Required. Kept in the system audit log." />
          <p className="floor-stepup"><Lock aria-hidden="true" size="0.8125rem" /> You may be asked to confirm it’s you.</p>
        </Form>
      </FormDialog>
      {dialog}
    </>
  );
}
