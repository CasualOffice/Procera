import { CornerDownRight, FileUp, FolderTree, Move, Pencil, Plus, Trash2, UserRoundCog } from 'lucide-react';
import { useMemo, useState } from 'react';
import { Form } from 'react-aria-components';
import { api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { type Department, deptPath, nameProblem, problemText, treeOrder } from '../lib/directory';
import { Link } from '../lib/router';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { TextField } from '../ui/components/TextField';
import { PeopleTabs } from '../ui/PeopleTabs';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { DirectoryImport } from './DirectoryImport';
import '../ui/directory.css';

const people = (n: number) => `${n} ${n === 1 ? 'person' : 'people'}`;
type Dialog = 'add' | 'move' | 'head' | 'delete' | null;

/**
 * Admin → People → Departments (BPMS-023, docs/design/ia-competitive-analysis.md §5.4): the org
 * chart as a tree with member counts, and one department's head, people and sub-departments.
 * Everyone with `directory:view` can look; `directory:manage` adds, renames, moves and deletes.
 */
export function Departments() {
  const { can } = useSession();
  const manage = can('directory:manage');
  const [items, reload, error] = useItems<Department>('/api/v1/departments');
  const [members] = useItems<Member>('/api/v1/members');
  const [selected, setSelected] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [importing, setImporting] = useState(false);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string } | null>(null);
  const ordered = useMemo(() => treeOrder(items ?? []), [items]);
  // The selection may name a department the list hasn't reloaded yet (just added); until then, and
  // after a delete, the first department is shown.
  const current = ordered.find((d) => d.id === selected) ?? ordered[0] ?? null;
  const done = (title: string, select?: string) => { setNotice({ tone: 'success', title }); setDialog(null); if (select) setSelected(select); reload(); };

  const head = (
    <div className="page-head">
      <div>
        <h1>Departments</h1>
        <p>{items?.length ? `${items.length} ${items.length === 1 ? 'department' : 'departments'} · each person belongs to one` : 'Your org chart, one department per person'}</p>
      </div>
      {manage && !!items?.length && (
        <div className="actions">
          <Button variant="secondary" onPress={() => setImporting(true)}><FileUp aria-hidden="true" size="1rem" /> Import from CSV</Button>
          <Button onPress={() => setDialog('add')}><Plus aria-hidden="true" size="1rem" /> Add department</Button>
        </div>
      )}
    </div>
  );

  let body: React.ReactNode;
  if (error) body = <LoadError error={error} onRetry={reload} what="departments" />;
  else if (items === null) body = <Skeleton variant="list" rows={5} label="Loading departments" />;
  else if (!items.length) {
    body = (
      <Empty art="departments" title="No departments yet"
        action={manage && <><Button onPress={() => setDialog('add')}><Plus aria-hidden="true" size="1rem" /> Add department</Button><Button variant="secondary" onPress={() => setImporting(true)}><FileUp aria-hidden="true" size="1rem" /> Import from CSV</Button></>}>
        Departments mirror your org chart. Each person belongs to one. Use them to scope roles and target campaigns.
      </Empty>
    );
  } else {
    body = (
      <div className="dir-split">
        <div className="dept-tree">
          <ul aria-label="Department tree">
            {ordered.map((d) => (
              <li key={d.id}>
                <button type="button" aria-current={d.id === current?.id ? 'true' : undefined} onClick={() => setSelected(d.id)} style={{ paddingLeft: `${0.625 + (d.path.length - 1) * 1.125}rem` }}>
                  {d.path.length > 1 ? <CornerDownRight aria-hidden="true" size="0.875rem" /> : <FolderTree aria-hidden="true" size="0.875rem" />}
                  <span className="dept-name">{d.name}</span>
                  <span className="dept-count">{d.member_count}<span className="visually-hidden"> {d.member_count === 1 ? 'person' : 'people'}</span></span>
                </button>
              </li>
            ))}
          </ul>
        </div>
        {current && <DepartmentDetail key={current.id} d={current} all={ordered} members={members ?? []} manage={manage} onSelect={setSelected} onOpen={setDialog} onRenamed={(n) => done(`Renamed to ${n}.`)} />}
      </div>
    );
  }

  return (
    <>
      <PeopleTabs current="departments" />
      {head}
      {notice && <Notice tone={notice.tone} title={notice.title} />}
      {body}
      <DirectoryImport isOpen={importing} onClose={() => setImporting(false)} onApplied={() => done('Directory updated.')} />
      {dialog === 'add' && <AddDepartment all={ordered} members={members ?? []} parentHint={current?.id ?? null} onClose={() => setDialog(null)} onAdded={(d, warn) => { if (warn) { setNotice({ tone: 'danger', title: warn }); setDialog(null); setSelected(d.id); reload(); } else done(`${d.name} added.`, d.id); }} />}
      {dialog === 'move' && current && <MoveDepartment d={current} all={ordered} onClose={() => setDialog(null)} onMoved={(to) => done(`${current.name} moved ${to}.`)} />}
      {dialog === 'head' && current && <ChangeHead d={current} members={members ?? []} onClose={() => setDialog(null)} onSaved={() => done(`Head of ${current.name} updated.`)} />}
      {dialog === 'delete' && current && <DeleteDepartment d={current} all={ordered} onClose={() => setDialog(null)} onDeleted={() => done(`${current.name} deleted.`, current.parent_id ?? undefined)} />}
    </>
  );
}

function DepartmentDetail({ d, all, members, manage, onSelect, onOpen, onRenamed }: {
  d: Department; all: Department[]; members: Member[]; manage: boolean;
  onSelect: (id: string) => void; onOpen: (d: Dialog) => void; onRenamed: (name: string) => void;
}) {
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(d.name);
  const [error, setError] = useState<string | null>(null);
  const children = all.filter((x) => x.parent_id === d.id);
  const head = members.find((m) => m.id === d.head_membership_id);
  const sibling = all.some((x) => x.id !== d.id && x.parent_id === d.parent_id && x.name.toLowerCase() === name.trim().toLowerCase());
  const problem = nameProblem(name, 'department') ?? (sibling ? 'Another department in the same place has this name.' : null);
  const save = async () => {
    if (problem) return;
    if (name.trim() === d.name) { setRenaming(false); return; }
    try {
      await api(`/api/v1/departments/${d.id}`, { method: 'PATCH', body: { name: name.trim() } });
      setRenaming(false);
      onRenamed(name.trim());
    } catch (e) { setError(problemText(e)); }
  };
  const crumbs = d.path.length > 1 ? deptPath(d.parent_id, all) : null;
  return (
    <section className="dir-detail" aria-labelledby="dept-title">
      <div className="dir-detail-head">
        {renaming ? (
          <Form className="inline-rename" validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); void save(); }}>
            <TextField label="Department name" onKeyDown={(e) => { if (e.key === 'Escape') { setRenaming(false); setName(d.name); } }} value={name} onChange={(v) => { setName(v); setError(null); }} autoFocus isRequired isInvalid={!!problem} errorMessage={problem ?? undefined} />
            <Button type="submit" isDisabled={!!problem}>Save name</Button>
            <Button variant="secondary" onPress={() => { setRenaming(false); setName(d.name); }}>Cancel</Button>
          </Form>
        ) : (
          <div>
            <h2 id="dept-title">{d.name}</h2>
            <p className="crumbs">{crumbs ? `Inside ${crumbs}` : 'Top level'}</p>
          </div>
        )}
        {manage && !renaming && (
          <div className="actions">
            <Button variant="secondary" className="btn-sm" onPress={() => setRenaming(true)}><Pencil aria-hidden="true" size="0.875rem" /> Rename</Button>
            <Button variant="secondary" className="btn-sm" onPress={() => onOpen('move')}><Move aria-hidden="true" size="0.875rem" /> Move to…</Button>
            <Button variant="ghost" className="btn-sm" onPress={() => onOpen('delete')}><Trash2 aria-hidden="true" size="0.875rem" /> Delete department</Button>
          </div>
        )}
      </div>
      {error && <Notice tone="danger" title={error} />}
      {renaming && <p className="dir-hint" style={{ marginBottom: '1rem' }}>The new name shows everywhere the department appears. The audit log records the change.</p>}
      <dl className="props">
        <dt>Head</dt>
        <dd>
          {head ? <Link to={`/members/${head.id}`}>{head.display_name}</Link> : <span className="muted">No head</span>}
          {manage && <Button variant="ghost" className="btn-sm" onPress={() => onOpen('head')}><UserRoundCog aria-hidden="true" size="0.875rem" /> Change head</Button>}
        </dd>
        <dt>People</dt>
        <dd>{people(d.member_count)}</dd>
        <dt>Sub-departments</dt>
        <dd>
          {children.length ? (
            <ul className="chip-list" aria-label={`Sub-departments of ${d.name}`}>
              {children.map((c) => <li key={c.id}><button type="button" className="chip-btn" onClick={() => onSelect(c.id)}>{c.name} <span className="muted">· {c.member_count}</span></button></li>)}
            </ul>
          ) : <span className="muted">None</span>}
        </dd>
      </dl>
      <p className="dir-hint">The head is the manager of everyone here who has no manager of their own; if this department has no head, the nearest head above it is used. Set a person’s department on their page.</p>
    </section>
  );
}

function AddDepartment({ all, members, parentHint, onClose, onAdded }: {
  all: Department[]; members: Member[]; parentHint: string | null; onClose: () => void; onAdded: (d: Department, warning?: string) => void;
}) {
  const [name, setName] = useState('');
  const [parent, setParent] = useState(parentHint ?? '');
  const [head, setHead] = useState('');
  const [touched, setTouched] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sibling = all.some((x) => (x.parent_id ?? '') === parent && x.name.toLowerCase() === name.trim().toLowerCase());
  const problem = nameProblem(name, 'department') ?? (sibling ? 'Another department in the same place has this name.' : null);
  const parents = [{ id: '', label: 'Top level' }, ...all.map((d) => ({ id: d.id, label: deptPath(d.id, all) ?? d.name }))];
  const heads = [{ id: '', label: 'No head' }, ...members.filter((m) => m.status === 'active').map((m) => ({ id: m.id, label: m.display_name }))];
  const submit = async () => {
    setTouched(true);
    if (problem) return;
    setError(null);
    let d: Department;
    try {
      d = await api<Department>('/api/v1/departments', { method: 'POST', body: { name: name.trim(), parent_id: parent || null } });
    } catch (e) { setError(problemText(e, 'Could not add the department.')); return; }
    if (!head) { onAdded(d); return; }
    try {
      await api(`/api/v1/departments/${d.id}`, { method: 'PATCH', body: { head_membership_id: head } });
      onAdded(d);
    } catch (e) { onAdded(d, `${d.name} was added, but its head wasn’t set: ${problemText(e)}`); }
  };
  const showProblem = touched || name.length > 0;
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Add department" description="Departments mirror your org chart. Each person belongs to one."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()}>Add department</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <Form validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); void submit(); }} style={{ display: 'contents' }}>
        <TextField label="Name" value={name} onChange={setName} isRequired autoFocus maxLength={120}
          isInvalid={showProblem && !!problem} errorMessage={showProblem ? problem ?? undefined : undefined} />
      </Form>
      <Select label="Inside" options={parents} selectedKey={parent} onSelectionChange={(k) => setParent(String(k))} />
      <Select label="Head (optional)" description="The manager for people here who have no manager of their own." options={heads} selectedKey={head} onSelectionChange={(k) => setHead(String(k))} />
    </FormDialog>
  );
}

function MoveDepartment({ d, all, onClose, onMoved }: { d: Department; all: Department[]; onClose: () => void; onMoved: (where: string) => void }) {
  const [to, setTo] = useState(d.parent_id ?? '');
  const [error, setError] = useState<string | null>(null);
  // A department can't move under itself or anything below it (the server refuses that cycle too).
  const below = all.filter((x) => x.path.includes(d.id) && x.id !== d.id);
  const options = [{ id: '', label: 'Top level' }, ...all.filter((x) => !x.path.includes(d.id)).map((x) => ({ id: x.id, label: deptPath(x.id, all) ?? x.name }))];
  const moving = below.reduce((n, x) => n + x.member_count, d.member_count);
  const submit = async () => {
    setError(null);
    try {
      await api(`/api/v1/departments/${d.id}`, { method: 'PATCH', body: { parent_id: to || null } });
      onMoved(to ? `inside ${deptPath(to, all)}` : 'to the top level');
    } catch (e) { setError(problemText(e, 'Could not move the department.')); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={`Move ${d.name}`}
      description={`Everything inside ${d.name} moves with it: ${below.length} ${below.length === 1 ? 'sub-department' : 'sub-departments'} and ${people(moving)}.`}
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()} isDisabled={to === (d.parent_id ?? '')}>Move</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <Select label="Move inside" options={options} selectedKey={to} onSelectionChange={(k) => setTo(String(k))} />
      {below.length > 0 && <p className="dir-hint">{d.name} can’t move inside itself or one of its own sub-departments, so {below.length === 1 ? `${below[0]!.name} isn’t` : 'those aren’t'} listed.</p>}
    </FormDialog>
  );
}

function ChangeHead({ d, members, onClose, onSaved }: { d: Department; members: Member[]; onClose: () => void; onSaved: () => void }) {
  const [head, setHead] = useState(d.head_membership_id ?? '');
  const [error, setError] = useState<string | null>(null);
  const options = [{ id: '', label: 'No head' }, ...members.filter((m) => m.status === 'active').map((m) => ({ id: m.id, label: m.display_name }))];
  const submit = async () => {
    try {
      await api(`/api/v1/departments/${d.id}`, { method: 'PATCH', body: { head_membership_id: head || null } });
      onSaved();
    } catch (e) { setError(problemText(e)); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={`Change head of ${d.name}`} description="The head is the manager of everyone in this department who has no manager of their own."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()}>Save</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <Select label="Head" options={options} selectedKey={head} onSelectionChange={(k) => setHead(String(k))} />
    </FormDialog>
  );
}

function DeleteDepartment({ d, all, onClose, onDeleted }: { d: Department; all: Department[]; onClose: () => void; onDeleted: () => void }) {
  const [error, setError] = useState<string | null>(null);
  const children = all.filter((x) => x.parent_id === d.id);
  if (children.length || d.member_count) {
    return (
      <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={`Move things out of ${d.name} first`} description="Only an empty department can be deleted, so nobody is left without one by accident."
        footer={<Button onPress={onClose}>Got it</Button>}>
        <ul className="must-move">
          {children.length > 0 && <li>{children.length === 1 ? 'Its sub-department' : `Its ${children.length} sub-departments`} ({children.map((c) => c.name).join(', ')}): move each one with <strong>Move to…</strong>, or delete it.</li>}
          {d.member_count > 0 && <li>{people(d.member_count)}: give each person another department on their page, or select them on <Link to="/admin/members">Members</Link> and use <strong>Set department</strong>.</li>}
        </ul>
      </FormDialog>
    );
  }
  const submit = async () => {
    try {
      await api(`/api/v1/departments/${d.id}`, { method: 'DELETE' });
      onDeleted();
    } catch (e) { setError(problemText(e, 'Could not delete the department.')); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={`Delete ${d.name}?`} description="It has no people and no sub-departments. This can’t be undone; the audit log keeps a record."
      footer={<><Button variant="secondary" autoFocus onPress={onClose}>Keep</Button><Button variant="danger" onPress={() => void submit()}><Trash2 aria-hidden="true" size="0.875rem" /> Delete department</Button></>}>
      {error ? <Notice tone="danger" title={error}>Deactivated people and positions also count. Move them out first.</Notice> : <p className="muted" style={{ margin: 0 }}>Settings overrides for this department are removed with it.</p>}
    </FormDialog>
  );
}
