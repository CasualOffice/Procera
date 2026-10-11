import { FileText, Plus, ScrollText, Search } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Form, Input, SearchField } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
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
import { Empty, LoadError, Skeleton } from '../ui/States';
import { BookOpen, SearchX } from 'lucide-react';

type Filter = 'all' | 'policy' | 'sop';

/** Library (docs/05 §3.2, §3.21): policies and SOPs. */
export function Library() {
  const { can } = useSession();
  const { query } = useRoute();
  const [filter, setFilter] = useState<Filter>('all');
  const [q, setQ] = useState('');
  const [docs, reload, loadError] = useItems<Artifact>('/api/v1/artifacts');
  const [members] = useItems<Member>(can('member:view') ? '/api/v1/members' : null);
  const people = memberMap(members);
  const newKind = query.get('new');
  const [creating, setCreating] = useState<'policy' | 'sop' | null>(newKind === 'policy' || newKind === 'sop' ? newKind : null);
  useEffect(() => { if (newKind === 'policy' || newKind === 'sop') setCreating(newKind); }, [newKind]);

  const rows = useMemo(() => (docs ?? []).filter((d) => (filter === 'all' || d.kind === filter) && (!q || `${d.title} ${d.key}`.toLowerCase().includes(q.toLowerCase()))), [docs, filter, q]);
  const counts = { all: docs?.length ?? 0, policy: docs?.filter((d) => d.kind === 'policy').length ?? 0, sop: docs?.filter((d) => d.kind === 'sop').length ?? 0 };
  const canCreate = can('policy:create') || can('sop:create');

  return (
    <>
      <div className="page-head">
        <div>
          <h1>Library</h1>
          <p>Policies say what and why; SOPs say how. Everything here is versioned, reviewed and audited.</p>
        </div>
        {canCreate && <Button onPress={() => setCreating(can('policy:create') ? 'policy' : 'sop')}><Plus aria-hidden="true" size={16} /> New document</Button>}
      </div>

      <div className="toolbar">
        <div className="segmented" role="group" aria-label="Filter by type">
          {(['all', 'policy', 'sop'] as const).map((f) => (
            <button key={f} type="button" aria-pressed={filter === f} onClick={() => setFilter(f)}>
              {f === 'all' ? 'All' : f === 'policy' ? 'Policies' : 'SOPs'} <span className="seg-count">{counts[f]}</span>
            </button>
          ))}
        </div>
        <SearchField className="search-input" aria-label="Filter documents" value={q} onChange={setQ}>
          <Search aria-hidden="true" size={15} />
          <Input placeholder="Filter by name…" />
        </SearchField>
      </div>

      {loadError ? <LoadError error={loadError} onRetry={reload} what="the library" /> : docs === null ? <Skeleton variant="table" rows={5} label="Loading documents" /> : rows.length === 0 ? (
        docs.length ? (
          <Empty icon={SearchX} title="No documents match" action={<Button variant="secondary" onPress={() => { setQ(''); setFilter('all'); }}>Clear filters</Button>}>Nothing matches {q ? `“${q}”` : 'this filter'}.</Empty>
        ) : (
          <Empty icon={BookOpen} title="Your library is empty" action={canCreate ? <Button onPress={() => setCreating(can('policy:create') ? 'policy' : 'sop')}><Plus aria-hidden="true" size={16} /> Create your first document</Button> : undefined}>Start with a policy such as a Code of Conduct, or an SOP your team already follows.</Empty>
        )
      ) : (
        <div className="table-wrap">
          <table className="table" aria-label="Documents">
            <thead><tr><th>Name</th><th className="col-optional">Type</th><th>Status</th><th className="col-optional">Owner</th><th className="col-optional">Criticality</th><th className="col-optional">Created</th></tr></thead>
            <tbody>
              {rows.map((d) => {
                const o = d.owner_membership_id ? people.get(d.owner_membership_id) : undefined;
                return (
                  <tr key={d.id}>
                    <td><span className="cell-entity"><span className={`doc-icon tag-${KIND_TAG[d.kind]?.color ?? 'gray'}`}>{d.kind === 'sop' ? <ScrollText aria-hidden="true" size={14} /> : <FileText aria-hidden="true" size={14} />}</span><Link to={`/library/${d.id}`}>{d.title}</Link></span></td>
                    <td className="col-optional"><Tag color={KIND_TAG[d.kind]?.color}>{KIND_TAG[d.kind]?.label ?? d.kind}</Tag></td>
                    <td><Tag color={STATE_TAG[d.state]?.color} dot>{STATE_TAG[d.state]?.label ?? d.state}</Tag></td>
                    <td className="col-optional">{o ? <span className="person"><Avatar name={o.display_name} size={20} />{o.display_name}</span> : <span className="cell-muted">No owner</span>}</td>
                    <td className="col-optional">{d.criticality === 'controlled' ? <Tag color="red">Controlled</Tag> : <span className="cell-muted">Standard</span>}</td>
                    <td className="cell-muted col-optional">{shortDate(d.created_at)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      <NewDocumentDialog kind={creating} onClose={() => { setCreating(null); if (newKind) navigate('/library', { replace: true }); }} onCreated={(id) => { reload(); navigate(`/library/${id}`); }} />
    </>
  );
}

function NewDocumentDialog({ kind, onClose, onCreated }: { kind: 'policy' | 'sop' | null; onClose: () => void; onCreated: (id: string) => void }) {
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
          <span className="doc-icon tag-purple"><FileText aria-hidden="true" size={14} /></span><span><strong>Policy</strong><small>What the rule is and why</small></span>
        </button>
        <button type="button" className="radio-card" aria-pressed={type === 'sop'} disabled={!can('sop:create')} onClick={() => setType('sop')}>
          <span className="doc-icon tag-blue"><ScrollText aria-hidden="true" size={14} /></span><span><strong>SOP</strong><small>How the work is done, step by step</small></span>
        </button>
      </div>
      <Form onSubmit={(e) => { e.preventDefault(); if (title.trim()) void submit(); }}>
        <TextField label="Title" value={title} onChange={setTitle} isRequired autoFocus description={title ? `Key: ${slugify(title)}` : 'For example “Acceptable Use of IT”.'} />
      </Form>
      <label className="check-row"><input type="checkbox" checked={controlled} onChange={(e) => setControlled(e.target.checked)} /> <span><strong>Controlled</strong><small>Always needs an independent review before it can be published.</small></span></label>
    </FormDialog>
  );
}
