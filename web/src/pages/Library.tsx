import { FileText, Plus, ScrollText } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api, download } from '../lib/api';
import { type Artifact, type Member, memberMap, useItems } from '../lib/data';
import { shortDate, slugify } from '../lib/format';
import { Link, navigate, useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { KIND_TAG, STATE_TAG, Tag } from '../ui/Tag';
import { term, useTerms } from '../lib/terms';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { DataGrid } from '../ui/components/DataGrid';
import { BookOpen, Download } from 'lucide-react';
import { type CollectionItem, collectionPath } from './docOrganisation';
import { CollectionTree } from './libraryCollections';

type Filter = 'all' | 'policy' | 'sop';

/** Library (docs/05 §3.2, §3.21): policies and SOPs. */
export function Library() {
  useTerms(); // the workspace's terms (CFG-008)
  const { can } = useSession();
  const { query } = useRoute();
  const [filter, setFilter] = useState<Filter>('all');
  const collection = query.get('collection');
  const [docs, reload, loadError] = useItems<Artifact>(collection ? `/api/v1/artifacts?collection=${collection}` : '/api/v1/artifacts');
  const [tree, setTree] = useState<{ items: CollectionItem[]; can_manage: boolean } | null>(null);
  const loadTree = () => { api<{ items: CollectionItem[]; can_manage: boolean }>('/api/v1/collections').then(setTree).catch(() => setTree({ items: [], can_manage: false })); };
  useEffect(loadTree, []);
  const here = collectionPath(collection, tree?.items ?? []);
  const showTree = !!tree && (tree.items.length > 0 || tree.can_manage);
  const [members] = useItems<Member>(can('member:view') ? '/api/v1/members' : null);
  const people = memberMap(members);
  const newKind = query.get('new');
  const [creating, setCreating] = useState<'policy' | 'sop' | null>(newKind === 'policy' || newKind === 'sop' ? newKind : null);
  useEffect(() => { if (newKind === 'policy' || newKind === 'sop') setCreating(newKind); }, [newKind]);

  const rows = useMemo(() => (docs ?? []).filter((d) => filter === 'all' || d.kind === filter), [docs, filter]);
  const counts = { all: docs?.length ?? 0, policy: docs?.filter((d) => d.kind === 'policy').length ?? 0, sop: docs?.filter((d) => d.kind === 'sop').length ?? 0 };
  const canCreate = can('policy:create') || can('sop:create');

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{here ?? 'Library'}</h1>
          <p>{here ? 'Documents in this collection and the ones inside it.' : `Policies say what and why; ${term('sop', { plural: true })} say how. Everything here is versioned, reviewed and audited.`}</p>
        </div>
        <div className="page-actions">
        <Button variant="secondary" onPress={() => void download(`/api/v1/raci/export${collection ? `?collection=${collection}` : ''}`, 'raci.csv')}><Download aria-hidden="true" size="1rem" /> Export RACI</Button>
        {canCreate && <Button onPress={() => setCreating(can('policy:create') ? 'policy' : 'sop')}><Plus aria-hidden="true" size="1rem" /> New document</Button>}
        </div>
      </div>

      <div className={showTree ? 'library-layout' : undefined}>
      {showTree && <CollectionTree items={tree!.items} current={collection} canManage={tree!.can_manage} onChanged={() => { loadTree(); reload(); }} />}
      <div>

      <div className="toolbar">
        <div className="segmented" role="group" aria-label="Filter by type">
          {(['all', 'policy', 'sop'] as const).map((f) => (
            <button key={f} type="button" aria-pressed={filter === f} onClick={() => setFilter(f)}>
              {f === 'all' ? 'All' : f === 'policy' ? 'Policies' : term('sop', { plural: true, capitalised: true })} <span className="seg-count">{counts[f]}</span>
            </button>
          ))}
        </div>
      </div>

      {loadError ? <LoadError error={loadError} onRetry={reload} what="the library" /> : docs === null ? <Skeleton variant="table" rows={5} label="Loading documents" /> : docs.length === 0 ? (
        <Empty icon={BookOpen} title="Your library is empty" action={canCreate ? <Button onPress={() => setCreating(can('policy:create') ? 'policy' : 'sop')}><Plus aria-hidden="true" size="1rem" /> Create your first document</Button> : undefined}>Start with a policy such as a Code of Conduct, or one of the {term('sop', { plural: true })} your team already follows.</Empty>
      ) : (
        <DataGrid<Artifact> caption="Documents" showCaption={false} list="library.documents" data={rows} rowHref={(d) => `/library/${d.id}`} initialSort={[{ id: 'title', desc: false }]}
          columns={[
            { id: 'title', header: 'Name', value: (d) => d.title, cell: (d) => <span className="cell-entity"><span className={`doc-icon tag-${KIND_TAG[d.kind]?.color ?? 'gray'}`}>{d.kind === 'sop' ? <ScrollText aria-hidden="true" size="0.875rem" /> : <FileText aria-hidden="true" size="0.875rem" />}</span><Link to={`/library/${d.id}`}>{d.title}</Link></span> },
            { id: 'kind', header: 'Type', type: 'enum', optional: true, value: (d) => KIND_TAG[d.kind]?.label ?? d.kind, cell: (d) => <Tag color={KIND_TAG[d.kind]?.color}>{KIND_TAG[d.kind]?.label ?? d.kind}</Tag> },
            { id: 'state', header: 'Status', type: 'enum', value: (d) => STATE_TAG[d.state]?.label ?? d.state, cell: (d) => <Tag color={STATE_TAG[d.state]?.color} dot>{STATE_TAG[d.state]?.label ?? d.state}</Tag> },
            { id: 'owner', header: 'Owner', optional: true, value: (d) => (d.owner_membership_id ? people.get(d.owner_membership_id)?.display_name ?? '' : ''), cell: (d) => { const o = d.owner_membership_id ? people.get(d.owner_membership_id) : undefined; return o ? <span className="person"><Avatar name={o.display_name} size="1.25rem" />{o.display_name}</span> : <span className="cell-muted">No owner</span>; } },
            { id: 'criticality', header: 'Criticality', type: 'enum', optional: true, value: (d) => (d.criticality === 'controlled' ? 'Controlled' : 'Standard'), cell: (d) => (d.criticality === 'controlled' ? <Tag color="red">Controlled</Tag> : <span className="cell-muted">Standard</span>) },
            { id: 'key', header: 'Key', hidden: true, value: (d) => d.key },
            { id: 'created', header: 'Created', type: 'date', optional: true, value: (d) => d.created_at, cell: (d) => <span className="cell-muted">{shortDate(d.created_at)}</span> },
          ]} />
      )}
      </div>
      </div>

      <NewDocumentDialog kind={creating} onClose={() => { setCreating(null); if (newKind) navigate('/library', { replace: true }); }} onCreated={(id) => { reload(); navigate(`/library/${id}`); }} />
    </>
  );
}

function NewDocumentDialog({ kind, onClose, onCreated }: { kind: 'policy' | 'sop' | null; onClose: () => void; onCreated: (id: string) => void }) {
  useTerms(); // the workspace's terms (CFG-008)
  const { can } = useSession();
  const [type, setType] = useState<'policy' | 'sop'>('policy');
  const [title, setTitle] = useState('');
  const [controlled, setControlled] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (kind) { setType(kind); setTitle(''); setControlled(false); setError(null); } }, [kind]);
  const submit = async () => {
    setBusy(true); setError(null);
    try {
      const r = await api<{ artifact: { id: string } }>('/api/v1/artifacts', { method: 'POST', body: { kind: type, key: slugify(title) || `${type}-${Date.now()}`, title, criticality: controlled ? 'controlled' : 'standard', content: { blocks: [] } } });
      onCreated(r.artifact.id); onClose();
    } catch (e) {
      setError(e instanceof ApiError ? (e.code === 'key_taken' ? 'A document with this name already exists.' : e.message) : 'Could not create the document.');
    } finally { setBusy(false); }
  };
  return (
    <FormDialog isOpen={kind !== null} onOpenChange={(o) => !o && onClose()} title="New document" description="It starts as a draft. Nobody else sees it until it’s published."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()} isDisabled={busy || !title.trim()}>{busy ? 'Creating…' : 'Create draft'}</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <div className="radio-cards" role="group" aria-label="Document type">
        <button type="button" className="radio-card" aria-pressed={type === 'policy'} disabled={!can('policy:create')} onClick={() => setType('policy')}>
          <span className="doc-icon tag-purple"><FileText aria-hidden="true" size="0.875rem" /></span><span><strong>Policy</strong><small>What the rule is and why</small></span>
        </button>
        <button type="button" className="radio-card" aria-pressed={type === 'sop'} disabled={!can('sop:create')} onClick={() => setType('sop')}>
          <span className="doc-icon tag-blue"><ScrollText aria-hidden="true" size="0.875rem" /></span><span><strong>{term('sop', { capitalised: true })}</strong><small>How the work is done, step by step</small></span>
        </button>
      </div>
      <Form onSubmit={(e) => { e.preventDefault(); if (title.trim()) void submit(); }}>
        <TextField label="Title" value={title} onChange={setTitle} isRequired autoFocus description={title ? `Key: ${slugify(title)}` : 'For example “Acceptable Use of IT”.'} />
      </Form>
      <label className="check-row"><input type="checkbox" checked={controlled} onChange={(e) => setControlled(e.target.checked)} /> <span><strong>Controlled</strong><small>Always needs an independent review before it can be published.</small></span></label>
    </FormDialog>
  );
}
