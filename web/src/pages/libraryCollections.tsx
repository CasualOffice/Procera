import { Folder, FolderOpen, Lock, Plus, Settings2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { Link } from '../lib/router';
import { type Audience, AudiencePicker, EVERYONE, audienceOf, isEmptyAudience } from '../ui/Audience';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { TextField } from '../ui/components/TextField';
import { type CollectionItem, collectionPath } from './docOrganisation';

/** Collection tree for the Library rail (docs/05 §3.2): "All documents", then the tree, indented by depth. */
export function CollectionTree({ items, current, canManage, onChanged }: { items: CollectionItem[]; current: string | null; canManage: boolean; onChanged: () => void }) {
  const [editing, setEditing] = useState<CollectionItem | 'new' | null>(null);
  const sel = items.find((c) => c.id === current) ?? null;
  return (
    <nav className="collection-tree" aria-label="Collections">
      <h2>
        <span>Collections</span>
        {canManage && <button type="button" className="icon-btn" aria-label="New collection" onClick={() => setEditing('new')}><Plus aria-hidden="true" size="0.875rem" /></button>}
      </h2>
      <ul>
        <li><Link to="/library" aria-current={current === null ? 'true' : undefined}><FolderOpen aria-hidden="true" size="0.875rem" /> All documents</Link></li>
        {items.map((c) => (
          <li key={c.id} style={{ paddingLeft: `${(c.path.length - 1) * 0.875}rem` }}>
            <Link to={`/library?collection=${c.id}`} aria-current={current === c.id ? 'true' : undefined}>
              <Folder aria-hidden="true" size="0.875rem" />
              <span>{c.name}</span>
              {c.visibility != null && <Lock aria-label="Restricted" size="0.75rem" className="lock" />}
              <span className="count">{c.documents || ''}</span>
            </Link>
          </li>
        ))}
      </ul>
      {canManage && sel && <Button variant="ghost" onPress={() => setEditing(sel)}><Settings2 aria-hidden="true" size="0.875rem" /> Manage “{sel.name}”</Button>}
      {editing && <CollectionDialog collection={editing === 'new' ? null : editing} parentHint={editing === 'new' ? current : null} all={items} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); onChanged(); }} />}
    </nav>
  );
}

function CollectionDialog({ collection, parentHint, all, onClose, onSaved }: { collection: CollectionItem | null; parentHint: string | null; all: CollectionItem[]; onClose: () => void; onSaved: () => void }) {
  const [name, setName] = useState(collection?.name ?? '');
  const [parent, setParent] = useState<string>(collection ? collection.parent_id ?? '' : parentHint ?? '');
  const [restrict, setRestrict] = useState(collection?.visibility != null);
  const [aud, setAud] = useState<Audience>(collection?.visibility ? audienceOf(collection.visibility) : { ...EVERYONE, all: false });
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setError(null), [name, parent]);
  // A collection cannot move under itself or its descendants.
  const parents = [{ id: '', label: 'Top level' }, ...all.filter((c) => !collection || !c.path.includes(collection.id)).map((c) => ({ id: c.id, label: collectionPath(c.id, all) ?? c.name }))];
  const fail = (e: unknown) => setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not save the collection.');
  const save = async () => {
    try {
      const saved = collection
        ? await api<CollectionItem>(`/api/v1/collections/${collection.id}`, { method: 'PATCH', body: { name, parent_id: parent || null } })
        : await api<CollectionItem>('/api/v1/collections', { method: 'POST', body: { name, parent_id: parent || null } });
      const wanted = restrict ? aud : null;
      const had = collection?.visibility ?? null;
      if (JSON.stringify(wanted) !== JSON.stringify(had)) {
        await api(`/api/v1/collections/${saved.id}/visibility`, { method: 'PUT', body: { visibility: wanted } });
      }
      onSaved();
    } catch (e) { fail(e); }
  };
  const remove = async () => { try { await api(`/api/v1/collections/${collection!.id}`, { method: 'DELETE' }); onSaved(); } catch (e) { fail(e); } };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} width={600} title={collection ? 'Collection settings' : 'New collection'} description="Collections group documents by area, like Operations or People."
      footer={<>{collection && <Button variant="ghost" onPress={() => void remove()}>Delete</Button>}<span style={{ flex: 1 }} /><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void save()} isDisabled={!name.trim() || (restrict && isEmptyAudience(aud))}>{collection ? 'Save' : 'Create'}</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <TextField label="Name" value={name} onChange={setName} autoFocus />
      <Select label="Inside" options={parents} selectedKey={parent} onSelectionChange={(k) => setParent(String(k))} />
      <label className="check-row"><input type="checkbox" checked={restrict} onChange={(e) => setRestrict(e.target.checked)} /><span><strong>Only some people can see it</strong><small>Readers outside the audience won’t see this collection or anything filed in it. Authors and admins always can.</small></span></label>
      {restrict && <AudiencePicker value={aud} onChange={setAud} />}
    </FormDialog>
  );
}
