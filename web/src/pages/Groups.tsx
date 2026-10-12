import { Plus, Trash2, UserRoundMinus, UserRoundPlus, UsersRound } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Form } from 'react-aria-components';
import { api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { type Group, nameProblem, problemText } from '../lib/directory';
import { Link, navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { setCrumb } from '../ui/AppShell';
import { Button } from '../ui/components/Button';
import { DataGrid } from '../ui/components/DataGrid';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { Empty, LoadError, Skeleton } from '../ui/States';
import '../ui/directory.css';

const people = (n: number) => `${n} ${n === 1 ? 'person' : 'people'}`;

/**
 * Admin → People → Groups (BPMS-023, docs/design/ia-competitive-analysis.md §5.4): flat sets of
 * people across departments (fire wardens, auditors), used for access and audiences.
 */
export function Groups() {
  const { can } = useSession();
  const manage = can('directory:manage');
  const [groups, reload, error] = useItems<Group>('/api/v1/groups');
  const [creating, setCreating] = useState(false);
  const newGroup = manage && <Button onPress={() => setCreating(true)}><Plus aria-hidden="true" size="1rem" /> New group</Button>;
  return (
    <>
      <div className="page-head">
        <div><h1>Groups</h1><p>{groups?.length ? `${groups.length} ${groups.length === 1 ? 'group' : 'groups'} · a person can be in many` : 'Sets of people across departments'}</p></div>
        {!!groups?.length && <div className="actions">{newGroup}</div>}
      </div>
      {error ? <LoadError error={error} onRetry={reload} what="groups" /> : groups === null ? <Skeleton variant="table" rows={4} label="Loading groups" /> : !groups.length ? (
        <Empty art="groups" title="No groups yet" action={newGroup}>
          Groups are flexible sets of people across departments, for example Fire wardens or Auditors. Use them for access and audiences. Each person has one department but can be in any number of groups.
        </Empty>
      ) : (
        <DataGrid<Group> caption="Groups" showCaption={false} data={groups} initialSort={[{ id: 'name', desc: false }]} rowHref={(g) => `/admin/groups/${g.id}`}
          columns={[
            { id: 'name', header: 'Name', value: (g) => g.name, cell: (g) => <span className="cell-entity"><span className="row-icon"><UsersRound aria-hidden="true" size="0.875rem" /></span><Link to={`/admin/groups/${g.id}`}>{g.name}</Link></span> },
            { id: 'purpose', header: 'Purpose', value: (g) => g.description ?? '', cell: (g) => g.description ?? <span className="cell-muted">None</span> },
            { id: 'members', header: 'Members', type: 'number', value: (g) => g.member_count, cell: (g) => <span className="cell-muted">{people(g.member_count)}</span> },
          ]} />
      )}
      {creating && <NewGroup onClose={() => setCreating(false)} onCreated={(g) => navigate(`/admin/groups/${g.id}?created=1`)} />}
    </>
  );
}

function NewGroup({ onClose, onCreated }: { onClose: () => void; onCreated: (g: Group) => void }) {
  const [name, setName] = useState('');
  const [purpose, setPurpose] = useState('');
  const [touched, setTouched] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const problem = nameProblem(name, 'group');
  const show = touched || name.length > 0;
  const submit = async () => {
    setTouched(true);
    if (problem) return;
    try {
      onCreated(await api<Group>('/api/v1/groups', { method: 'POST', body: { name: name.trim(), description: purpose.trim() || null } }));
    } catch (e) { setError(problemText(e, 'Could not create the group.')); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="New group" description="A set of people across departments. You add people next."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()}>Create group</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <Form validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); void submit(); }} style={{ display: 'contents' }}>
        <TextField label="Name" value={name} onChange={(v) => { setName(v); setError(null); }} isRequired autoFocus maxLength={120}
          isInvalid={show && !!problem} errorMessage={show ? problem ?? undefined : undefined} />
        <TextField label="Purpose (optional)" description="One line on what the group is for, e.g. “Trained first aiders on every floor”." value={purpose} onChange={setPurpose} maxLength={500} />
      </Form>
    </FormDialog>
  );
}

/** One group (`/admin/groups/{id}`): who is in it, add and remove people, delete. */
export function GroupDetail({ id, created = false }: { id: string; created?: boolean }) {
  const { can } = useSession();
  const manage = can('directory:manage');
  const [groups, reloadGroups, error] = useItems<Group>('/api/v1/groups');
  const [ids, reloadIds] = useItems<string>(`/api/v1/groups/${id}/members`);
  const [members] = useItems<Member>('/api/v1/members');
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<Member | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string } | null>(created ? { tone: 'success', title: 'Group created. Add the people who belong in it.' } : null);
  const g = groups?.find((x) => x.id === id) ?? null;
  useEffect(() => { if (g) setCrumb(g.name); return () => setCrumb(null); }, [g]);
  const inGroup = useMemo(() => (ids ?? []).map((m) => members?.find((x) => x.id === m)).filter((m): m is Member => !!m), [ids, members]);
  const reload = () => { reloadGroups(); reloadIds(); };
  const remove = async (m: Member) => {
    setRemoving(null);
    try {
      await api(`/api/v1/groups/${id}/members/${m.id}`, { method: 'DELETE' });
      setNotice({ tone: 'success', title: `${m.display_name} removed from ${g?.name}.` });
      reload();
    } catch (e) { setNotice({ tone: 'danger', title: problemText(e) }); }
  };
  const destroy = async () => {
    try {
      await api(`/api/v1/groups/${id}`, { method: 'DELETE' });
      navigate('/admin/groups');
    } catch (e) { setDeleting(false); setNotice({ tone: 'danger', title: problemText(e) }); }
  };

  if (error) return <LoadError error={error} onRetry={reload} what="this group" />;
  if (groups === null || ids === null || members === null) return <Skeleton variant="list" rows={4} label="Loading group" />;
  if (!g) return <Empty art="generic" title="This group doesn’t exist" action={<Link className="btn btn-secondary btn-md" to="/admin/groups">All groups</Link>}>It may have been deleted, or the link is wrong.</Empty>;
  return (
    <div className="group-members">
      <div className="page-head">
        <div><h1>{g.name}</h1><p>{g.description ?? 'No purpose written'} · {people(inGroup.filter((m) => m.status === 'active').length)}</p></div>
        {manage && (
          <div className="actions">
            <Button variant="ghost" onPress={() => setDeleting(true)}><Trash2 aria-hidden="true" size="1rem" /> Delete group</Button>
            <Button onPress={() => setAdding(true)}><UserRoundPlus aria-hidden="true" size="1rem" /> Add members</Button>
          </div>
        )}
      </div>
      {notice && <Notice tone={notice.tone} title={notice.title} />}
      {inGroup.length ? (
        <ul className="card rows" aria-label={`Members of ${g.name}`}>
          {inGroup.map((m) => (
            <li key={m.id}>
              <Avatar name={m.display_name} size="1.75rem" />
              <div className="row-main">
                <p className="row-title"><Link to={`/members/${m.id}`}>{m.display_name}</Link>{m.status !== 'active' && <span className="muted"> · deactivated</span>}</p>
                <p className="row-sub">{m.title ? `${m.title} · ` : ''}{m.email}</p>
              </div>
              {manage && <Button variant="secondary" className="btn-sm" aria-label={`Remove ${m.display_name}`} onPress={() => setRemoving(m)}><UserRoundMinus aria-hidden="true" size="0.875rem" /> Remove</Button>}
            </li>
          ))}
        </ul>
      ) : (
        <Empty compact art="groups" title="Nobody is in this group yet" action={manage && <Button onPress={() => setAdding(true)}><UserRoundPlus aria-hidden="true" size="1rem" /> Add members</Button>}>
          Add people from any department.
        </Empty>
      )}
      {adding && <AddMembers group={g} members={members} already={new Set(ids)} onClose={() => setAdding(false)}
        onDone={(added, failed) => { setAdding(false); setNotice(failed ? { tone: 'danger', title: `Added ${people(added)}; ${failed}` } : { tone: 'success', title: `Added ${people(added)} to ${g.name}.` }); reload(); }} />}
      <FormDialog isOpen={removing !== null} onOpenChange={(o) => !o && setRemoving(null)} title={`Remove ${removing?.display_name ?? ''} from ${g.name}?`}
        description="They stay in the workspace and keep their department. Roles that come from this group stop applying to them."
        footer={<><Button variant="secondary" autoFocus onPress={() => setRemoving(null)}>Keep</Button><Button variant="danger" onPress={() => removing && void remove(removing)}>Remove</Button></>}>
        <p className="muted" style={{ margin: 0 }}>You can add them back at any time. The audit log records the change.</p>
      </FormDialog>
      <FormDialog isOpen={deleting} onOpenChange={setDeleting} title={`Delete ${g.name}?`}
        description={`The ${people(inGroup.length)} in it stay in the workspace. Roles granted to the group, and its entries in responsibility (RACI) lists, are removed with it.`}
        footer={<><Button variant="secondary" autoFocus onPress={() => setDeleting(false)}>Keep</Button><Button variant="danger" onPress={() => void destroy()}><Trash2 aria-hidden="true" size="0.875rem" /> Delete group</Button></>}>
        <p className="muted" style={{ margin: 0 }}>This can’t be undone. The audit log keeps a record.</p>
      </FormDialog>
    </div>
  );
}

/** Multi-select people picker: active members not yet in the group, searchable by name or email. */
function AddMembers({ group, members, already, onClose, onDone }: { group: Group; members: Member[]; already: Set<string>; onClose: () => void; onDone: (added: number, failed: string | null) => void }) {
  const [q, setQ] = useState('');
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const candidates = members.filter((m) => m.status === 'active' && !already.has(m.id));
  const shown = candidates.filter((m) => `${m.display_name} ${m.email}`.toLowerCase().includes(q.trim().toLowerCase()));
  const toggle = (id: string) => setChosen((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const submit = async () => {
    setBusy(true);
    let added = 0;
    for (const id of chosen) {
      try {
        await api(`/api/v1/groups/${group.id}/members`, { method: 'POST', body: { membership_id: id } });
        added += 1;
      } catch (e) {
        const who = members.find((m) => m.id === id)?.display_name ?? 'someone';
        onDone(added, `${who} couldn’t be added: ${problemText(e)}`);
        return;
      }
    }
    onDone(added, null);
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={`Add members to ${group.name}`} description="Choose one or more people. They keep their department."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()} isDisabled={!chosen.size || busy}>{chosen.size ? `Add ${people(chosen.size)}` : 'Add'}</Button></>}>
      {candidates.length ? (
        <>
          <TextField label="Find people" value={q} onChange={setQ} autoFocus />
          <ul className="pick-list" aria-label="People">
            {shown.map((m) => (
              <li key={m.id}>
                <label>
                  <input type="checkbox" checked={chosen.has(m.id)} onChange={() => toggle(m.id)} />
                  <Avatar name={m.display_name} size="1.5rem" />
                  <span>{m.display_name}<small>{m.email}</small></span>
                </label>
              </li>
            ))}
            {!shown.length && <li><p className="muted" style={{ margin: 0, padding: '0.75rem' }}>Nobody matches “{q}”.</p></li>}
          </ul>
        </>
      ) : <p className="muted" style={{ margin: 0 }}>Everyone in the workspace is already in this group.</p>}
    </FormDialog>
  );
}
