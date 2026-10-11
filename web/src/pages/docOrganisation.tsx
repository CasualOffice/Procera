import { Folder, User, UsersRound } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Input, SearchField } from 'react-aria-components';
import { api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { Avatar } from '../ui/Avatar';
import { useDirectory } from '../ui/Audience';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Select } from '../ui/components/Select';
import type { Act } from './docGovernance';

export interface CollectionItem { id: string; parent_id: string | null; name: string; path: string[]; visibility: unknown; documents: number }
interface Assignee { membership_id?: string; group_id?: string; name?: string }
type Raci = Record<'responsible' | 'accountable' | 'consulted' | 'informed', Assignee[]>;
const ROLES = [['responsible', 'Responsible', 'Does the work'], ['accountable', 'Accountable', 'Answers for it (one person or group)'], ['consulted', 'Consulted', 'Asked before changes'], ['informed', 'Informed', 'Told after changes']] as const;

/** "Ops / Facilities / Security" for a collection id. */
export function collectionPath(id: string | null | undefined, all: CollectionItem[]): string | null {
  if (!id) return null;
  const c = all.find((x) => x.id === id);
  if (!c) return null;
  return c.path.map((p) => all.find((x) => x.id === p)?.name ?? '…').join(' / ');
}

/** Where the document is filed (BPMS-041). Publishers move it. */
export function CollectionRow({ artifactId, collectionId, canEdit, act }: { artifactId: string; collectionId: string | null; canEdit: boolean; act: Act }) {
  const [collections] = useItems<CollectionItem>('/api/v1/collections');
  const [open, setOpen] = useState(false);
  const [pick, setPick] = useState<string>('');
  useEffect(() => { if (open) setPick(collectionId ?? ''); }, [open, collectionId]);
  const all = collections ?? [];
  const options = [{ id: '', label: 'Top level (no collection)' }, ...all.map((c) => ({ id: c.id, label: collectionPath(c.id, all) ?? c.name }))];
  const save = () => act(() => api(`/api/v1/artifacts/${artifactId}/collection`, { method: 'PUT', body: { collection_id: pick || null } }), 'Moved.').then((ok) => ok && setOpen(false));
  return (
    <>
      <dt><Folder aria-hidden="true" size="0.9375rem" /> Collection</dt>
      <dd>
        <span className={collectionId ? undefined : 'muted'}>{collectionPath(collectionId, all) ?? 'Top level'}</span>
        {canEdit && <span className="prop-actions"><button type="button" className="prop-link" onClick={() => setOpen(true)} aria-label="Change collection">Change</button></span>}
      </dd>
      <FormDialog isOpen={open} onOpenChange={setOpen} title="Move to a collection" description="Readers outside a restricted collection can’t see what is filed in it."
        footer={<><Button variant="secondary" onPress={() => setOpen(false)}>Cancel</Button><Button onPress={() => void save()}>Move</Button></>}>
        <Select label="Collection" options={options} selectedKey={pick} onSelectionChange={(k) => setPick(String(k))} />
      </FormDialog>
    </>
  );
}

/** The accountable owner. Publishers hand it over to another active member. */
export function OwnerRow({ artifactId, owner, canEdit, act }: { artifactId: string; owner: Member | undefined; canEdit: boolean; act: Act }) {
  const [members] = useItems<Member>(canEdit ? '/api/v1/members' : null);
  const [open, setOpen] = useState(false);
  const [pick, setPick] = useState('');
  useEffect(() => { if (open) setPick(owner?.id ?? ''); }, [open, owner?.id]);
  const options = (members ?? []).filter((m) => m.status === 'active').map((m) => ({ id: m.id, label: m.display_name }));
  const save = () => act(() => api(`/api/v1/artifacts/${artifactId}/owner`, { method: 'PUT', body: { membership_id: pick } }), 'Owner changed.').then((ok) => ok && setOpen(false));
  return (
    <>
      <dt><User aria-hidden="true" size="0.9375rem" /> Owner</dt>
      <dd>
        {owner ? <span className="person"><Avatar name={owner.display_name} size="1.25rem" />{owner.display_name}</span> : <span className="muted">No owner</span>}
        {canEdit && <span className="prop-actions"><button type="button" className="prop-link" onClick={() => setOpen(true)} aria-label="Change owner">Change</button></span>}
      </dd>
      <FormDialog isOpen={open} onOpenChange={setOpen} title="Change the owner" description="The owner is accountable for keeping this document correct and current."
        footer={<><Button variant="secondary" onPress={() => setOpen(false)}>Cancel</Button><Button onPress={() => void save()} isDisabled={!pick}>Save</Button></>}>
        <Select label="Owner" options={options} selectedKey={pick || null} onSelectionChange={(k) => setPick(String(k))} />
      </FormDialog>
    </>
  );
}

/** Responsible, accountable, consulted and informed (docs/05 §6.1). Publishers edit; everyone who can read it sees it. */
export function RaciRows({ artifactId, canEdit, act }: { artifactId: string; canEdit: boolean; act: Act }) {
  const [raci, setRaci] = useState<Raci | null>(null);
  const [open, setOpen] = useState(false);
  const load = () => { api<Raci>(`/api/v1/artifacts/${artifactId}/raci`).then(setRaci).catch(() => setRaci(null)); };
  useEffect(load, [artifactId]);
  if (!raci) return null;
  const empty = ROLES.every(([k]) => raci[k].length === 0);
  if (empty && !canEdit) return null;
  return (
    <>
      <dt><UsersRound aria-hidden="true" size="0.9375rem" /> RACI</dt>
      <dd className="raci-dd">
        {empty ? <span className="muted">Not set</span> : (
          <span className="raci-list">
            {ROLES.filter(([k]) => raci[k].length).map(([k, label]) => (
              <span key={k} className="raci-item"><span className="raci-role" title={label}>{label[0]}</span>{raci[k].map((x) => x.name).join(', ')}</span>
            ))}
          </span>
        )}
        {canEdit && <span className="prop-actions"><button type="button" className="prop-link" onClick={() => setOpen(true)} aria-label="Edit RACI">Edit</button></span>}
      </dd>
      {open && <RaciDialog artifactId={artifactId} raci={raci} onClose={() => setOpen(false)} onSaved={() => { setOpen(false); load(); }} act={act} />}
    </>
  );
}

function RaciDialog({ artifactId, raci, onClose, onSaved, act }: { artifactId: string; raci: Raci; onClose: () => void; onSaved: () => void; act: Act }) {
  const [members] = useItems<Member>('/api/v1/members');
  const dir = useDirectory();
  const [draft, setDraft] = useState<Raci>(raci);
  const [role, setRole] = useState<keyof Raci>('responsible');
  const [q, setQ] = useState('');
  const people = useMemo(() => [
    ...(members ?? []).filter((m) => m.status === 'active').map((m) => ({ key: `m:${m.id}`, a: { membership_id: m.id } as Assignee, name: m.display_name, group: false })),
    ...dir.groups.map((g) => ({ key: `g:${g.id}`, a: { group_id: g.id } as Assignee, name: g.name, group: true })),
  ].filter((p) => !q || p.name.toLowerCase().includes(q.toLowerCase())), [members, dir.groups, q]);
  const has = (a: Assignee) => draft[role].some((x) => (a.membership_id && x.membership_id === a.membership_id) || (a.group_id && x.group_id === a.group_id));
  const toggle = (a: Assignee) => setDraft({ ...draft, [role]: has(a) ? draft[role].filter((x) => !((a.membership_id && x.membership_id === a.membership_id) || (a.group_id && x.group_id === a.group_id))) : role === 'accountable' ? [a] : [...draft[role], a] });
  const strip = (v: Assignee[]) => v.map(({ membership_id, group_id }) => (membership_id ? { membership_id } : { group_id }));
  const save = () => act(() => api(`/api/v1/artifacts/${artifactId}/raci`, { method: 'PUT', body: { responsible: strip(draft.responsible), accountable: strip(draft.accountable), consulted: strip(draft.consulted), informed: strip(draft.informed) } }), 'RACI saved.').then((ok) => ok && onSaved());
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} width={640} title="RACI" description="Who does the work, who answers for it, who is asked and who is told."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void save()}>Save</Button></>}>
      <div className="segmented" role="group" aria-label="Role">
        {ROLES.map(([k, label]) => <button key={k} type="button" aria-pressed={role === k} onClick={() => setRole(k)}>{label} <span className="seg-count">{draft[k].length}</span></button>)}
      </div>
      <p className="muted" style={{ margin: 0, fontSize: '0.8125rem' }}>{ROLES.find(([k]) => k === role)?.[2]}</p>
      <SearchField className="search-input" aria-label="Find people or groups" value={q} onChange={setQ}><Input placeholder="Find people or groups…" /></SearchField>
      <ul className="pick-list" aria-label="People and groups">
        {people.map((p) => (
          <li key={p.key}>
            <label className="check-row"><input type="checkbox" checked={has(p.a)} onChange={() => toggle(p.a)} /><span><strong>{p.name}</strong>{p.group && <small>Group</small>}</span></label>
          </li>
        ))}
      </ul>
    </FormDialog>
  );
}
