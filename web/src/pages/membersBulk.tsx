import { useState } from 'react';
import { api } from '../lib/api';
import { type Department, type Group, deptPath, problemText, treeOrder } from '../lib/directory';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';

/** A selected Members row as the bulk actions see it. */
export interface BulkRow { id: string; name: string; active: boolean; invite: boolean }

const people = (n: number) => `${n} ${n === 1 ? 'person' : 'people'}`;

/** What the selection holds that the action can't touch (deactivated people, invitations). */
function skipped(rows: BulkRow[]): string | null {
  const off = rows.filter((r) => !r.invite && !r.active).length;
  const inv = rows.filter((r) => r.invite).length;
  const parts = [off && `${people(off)} deactivated`, inv && `${inv} ${inv === 1 ? 'invitation' : 'invitations'}`].filter(Boolean);
  return parts.length ? `Skipped: ${parts.join(' and ')} (only active members can be changed).` : null;
}

/**
 * Runs `call` for each active member in the selection, one after another, and reports how many
 * succeeded and the first failure. Each call is its own audited change.
 */
async function each(rows: BulkRow[], call: (id: string) => Promise<unknown>): Promise<{ done: number; failed: string | null }> {
  let done = 0;
  for (const r of rows.filter((x) => x.active && !x.invite)) {
    try { await call(r.id); done += 1; } catch (e) { return { done, failed: `${r.name}: ${problemText(e)}` }; }
  }
  return { done, failed: null };
}

export function BulkSetDepartment({ rows, departments, onClose, onDone }: { rows: BulkRow[]; departments: Department[]; onClose: () => void; onDone: (title: string, tone: 'success' | 'danger') => void }) {
  const [dept, setDept] = useState('');
  const [busy, setBusy] = useState(false);
  const targets = rows.filter((r) => r.active && !r.invite);
  const options = [{ id: '', label: 'No department' }, ...treeOrder(departments).map((d) => ({ id: d.id, label: deptPath(d.id, departments) ?? d.name }))];
  const where = dept ? deptPath(dept, departments) : 'no department';
  const run = async () => {
    setBusy(true);
    const { done, failed } = await each(rows, (id) => api(`/api/v1/members/${id}`, { method: 'PATCH', body: { department_id: dept || null } }));
    onDone(failed ? `Set the department of ${people(done)}; stopped at ${failed}` : `${people(done)} now in ${where}.`, failed ? 'danger' : 'success');
  };
  const skip = skipped(rows);
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Set department" description="Each person belongs to one department, so this moves them from wherever they are now."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void run()} isDisabled={!targets.length || busy}>Set department for {people(targets.length)}</Button></>}>
      <Select label="Department" options={options} selectedKey={dept} onSelectionChange={(k) => setDept(String(k))} />
      <p className="dir-hint">Moves {targets.map((r) => r.name).join(', ') || 'nobody'} to {where}. Each change is recorded in the audit log.</p>
      {skip && <Notice tone="info" title={skip} />}
    </FormDialog>
  );
}

export function BulkAddToGroup({ rows, groups, onClose, onDone }: { rows: BulkRow[]; groups: Group[]; onClose: () => void; onDone: (title: string, tone: 'success' | 'danger') => void }) {
  const [group, setGroup] = useState(groups[0]?.id ?? '');
  const [busy, setBusy] = useState(false);
  const targets = rows.filter((r) => r.active && !r.invite);
  const g = groups.find((x) => x.id === group);
  const run = async () => {
    setBusy(true);
    let already = 0;
    const { done, failed } = await each(rows, async (id) => {
      try { await api(`/api/v1/groups/${group}/members`, { method: 'POST', body: { membership_id: id } }); } catch (e) {
        // Someone already in the group is fine: the outcome is the same.
        if (problemText(e).includes('already in the group')) { already += 1; return; }
        throw e;
      }
    });
    const note = already ? ` (${already} already there)` : '';
    onDone(failed ? `Added ${people(done)}${note}; stopped at ${failed}` : `Added ${people(done)} to ${g?.name}${note}.`, failed ? 'danger' : 'success');
  };
  const skip = skipped(rows);
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Add to group" description="People keep their department; a person can be in any number of groups."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void run()} isDisabled={!targets.length || !group || busy}>Add {people(targets.length)}</Button></>}>
      {groups.length ? <Select label="Group" options={groups.map((x) => ({ id: x.id, label: x.name }))} selectedKey={group} onSelectionChange={(k) => setGroup(String(k))} />
        : <p className="muted" style={{ margin: 0 }}>There are no groups yet. Create one under Groups first.</p>}
      {g && <p className="dir-hint">Adds {targets.map((r) => r.name).join(', ') || 'nobody'} to {g.name}. Roles granted to the group apply to them at once.</p>}
      {skip && <Notice tone="info" title={skip} />}
    </FormDialog>
  );
}
