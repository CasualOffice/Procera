import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import type { Member } from '../lib/data';
import { type Department, type Group, type MemberDirectory, deptPath, problemText, treeOrder } from '../lib/directory';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { TextField } from '../ui/components/TextField';
import { useStepUp } from '../ui/StepUp';

/**
 * Person › Edit profile (docs/design/ia-competitive-analysis.md §8.3): the directory fields
 * `PATCH /members/{id}` accepts. Only changed fields are sent, so a field's source ("from CSV
 * import") changes only when the field does. Manager cycles come back from the server as errors.
 */
export function EditProfile({ member, dir, members, departments, onClose, onSaved }: {
  member: Member; dir: MemberDirectory; members: Member[]; departments: Department[]; onClose: () => void; onSaved: () => void;
}) {
  const [title, setTitle] = useState(dir.title ?? '');
  const [dept, setDept] = useState(dir.department_id ?? '');
  const [manager, setManager] = useState(dir.manager_membership_id ?? '');
  const [location, setLocation] = useState(dir.location ?? '');
  const [error, setError] = useState<string | null>(null);
  const depts = [{ id: '', label: 'No department' }, ...treeOrder(departments).map((d) => ({ id: d.id, label: deptPath(d.id, departments) ?? d.name }))];
  const managers = [{ id: '', label: 'No manager of their own' }, ...members.filter((m) => m.status === 'active' && m.id !== member.id).map((m) => ({ id: m.id, label: m.display_name }))];
  const save = async () => {
    const body: Record<string, string | null> = {};
    if (title.trim() !== (dir.title ?? '')) body.title = title.trim() || null;
    if (dept !== (dir.department_id ?? '')) body.department_id = dept || null;
    if (manager !== (dir.manager_membership_id ?? '')) body.manager_membership_id = manager || null;
    if (location.trim() !== (dir.location ?? '')) body.location = location.trim() || null;
    if (!Object.keys(body).length) { onClose(); return; }
    try {
      await api(`/api/v1/members/${member.id}`, { method: 'PATCH', body });
      onSaved();
    } catch (e) {
      setError(e instanceof ApiError && e.code === 'cycle' ? `Can’t save: ${e.problem.detail ?? 'this would create a reporting loop'}.` : problemText(e, 'Could not save the profile.'));
    }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={`Edit ${member.display_name}’s profile`} description="Changes show at once and are recorded in the audit log."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void save()}>Save profile</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <Form onSubmit={(e) => { e.preventDefault(); void save(); }} style={{ display: 'contents' }}>
        <TextField label="Title" value={title} onChange={setTitle} maxLength={120} autoFocus />
        <Select label="Department" options={depts} selectedKey={dept} onSelectionChange={(k) => setDept(String(k))} />
        <Select label="Manager" description="Without a manager of their own, the head of their department (or the nearest one above it) is their manager."
          options={managers} selectedKey={manager} onSelectionChange={(k) => setManager(String(k))} />
        <TextField label="Location" value={location} onChange={setLocation} maxLength={120} description="Where they work, e.g. “Bangalore” or “Remote”." />
      </Form>
    </FormDialog>
  );
}

/** Person › Add to group: one of the groups they're not in yet. */
export function AddToGroup({ member, groups, current, onClose, onAdded }: { member: Member; groups: Group[]; current: string[]; onClose: () => void; onAdded: (g: Group) => void }) {
  const options = groups.filter((g) => !current.includes(g.id)).map((g) => ({ id: g.id, label: g.name }));
  const [group, setGroup] = useState(options[0]?.id ?? '');
  const [error, setError] = useState<string | null>(null);
  const add = async () => {
    try {
      await api(`/api/v1/groups/${group}/members`, { method: 'POST', body: { membership_id: member.id } });
      onAdded(groups.find((g) => g.id === group)!);
    } catch (e) { setError(problemText(e)); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={`Add ${member.display_name} to a group`} description="Groups cross departments; a person can be in any number of them."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void add()} isDisabled={!group}>Add to group</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      {options.length
        ? <Select label="Group" options={options} selectedKey={group} onSelectionChange={(k) => setGroup(String(k))} />
        : <p className="muted" style={{ margin: 0 }}>{groups.length ? `${member.display_name} is already in every group.` : 'There are no groups yet. Create one under Groups.'}</p>}
    </FormDialog>
  );
}

/**
 * Person › Reactivate (docs/22 A-41): they come back with the roles and groups they had. It
 * restores access, so it asks for a reason and a recent verification.
 */
export function Reactivate({ member, roleNames, onClose, onDone }: { member: Member; roleNames: string[]; onClose: () => void; onDone: (title: string, tone: 'success' | 'danger') => void }) {
  const [reason, setReason] = useState('');
  const [tried, setTried] = useState(false);
  const onError = (e: unknown) => {
    if (e instanceof ApiError && e.code === 'escalation') onDone(`You can’t bring ${member.display_name} back: their roles include permissions you don’t hold. Ask a workspace owner.`, 'danger');
    else onDone(problemText(e), 'danger');
  };
  const { guard, dialog } = useStepUp(onError);
  const confirm = () => {
    setTried(true);
    const why = reason.trim();
    if (!why) return;
    void guard(async () => {
      await api(`/api/v1/members/${member.id}/reactivate`, { method: 'POST', body: { reason: why } });
      onDone(`${member.display_name} can sign in to this workspace again.`, 'success');
    });
  };
  const missing = tried && !reason.trim();
  return (
    <>
      <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={`Reactivate ${member.display_name}?`}
        description={`They can sign in to this workspace again, with the roles they had (${roleNames.join(', ') || 'none'}), their groups and their department.`}
        footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={confirm}>Reactivate</Button></>}>
        <Form validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); confirm(); }}>
          <TextField label="Reason" value={reason} onChange={setReason} isRequired maxLength={500} autoFocus
            description="For the audit log, e.g. “Back from parental leave”."
            isInvalid={missing} errorMessage={missing ? 'Give a reason.' : undefined} />
        </Form>
      </FormDialog>
      {dialog}
    </>
  );
}
