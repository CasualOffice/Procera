import { Plus, Workflow } from 'lucide-react';
import { useState } from 'react';
import { Form } from 'react-aria-components';
import { useItems } from '../lib/data';
import { Link, navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { problemText } from '../lib/directory';
import { Button } from '../ui/components/Button';
import { DataGrid } from '../ui/components/DataGrid';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { RadioCards } from '../ui/components/RadioCards';
import { TextField } from '../ui/components/TextField';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { STATE_TAG, Tag } from '../ui/Tag';
import { KEY_RE, type ProcessRow, type ProcessTemplate, createProcess, keyFrom } from './studio/api';
import './studio/studio.css';

/**
 * Admin › Processes (BPMS-100, docs/05 §3.5): the request types this workspace designs itself.
 * Each opens in the Guided Studio; a new one starts blank or from a built-in.
 */
export function Processes({ startNew = false }: { startNew?: boolean }) {
  const { can } = useSession();
  const [rows, reload, error] = useItems<ProcessRow>('/api/v1/processes');
  const [creating, setCreating] = useState(startNew && can('process:create'));
  const newButton = can('process:create') && <Button onPress={() => setCreating(true)}><Plus aria-hidden="true" size="1rem" /> New process</Button>;
  return (
    <>
      <div className="page-head">
        <div>
          <h1>Processes</h1>
          <p>{rows?.length ? `${rows.length} ${rows.length === 1 ? 'process' : 'processes'} · published ones appear under New request` : 'Design your own request types: the form people fill and who approves it'}</p>
        </div>
        {!!rows?.length && <div className="actions">{newButton}</div>}
      </div>
      {error ? <LoadError error={error} onRetry={reload} what="processes" /> : rows === null ? <Skeleton variant="table" rows={4} label="Loading processes" /> : !rows.length ? (
        <Empty art="templates" title="No processes yet" action={newButton}>
          A process is a request people can make, like a laptop or a training budget: the questions they answer, then who approves it and by when. Start blank or from a request type you already know.
        </Empty>
      ) : (
        <DataGrid<ProcessRow> caption="Processes" showCaption={false} data={rows} initialSort={[{ id: 'title', desc: false }]} rowHref={(p) => `/admin/processes/${p.id}`}
          columns={[
            { id: 'title', header: 'Process', value: (p) => p.title, cell: (p) => <span className="cell-entity"><span className="row-icon"><Workflow aria-hidden="true" size="0.875rem" /></span><Link to={`/admin/processes/${p.id}`}>{p.title}</Link></span> },
            { id: 'key', header: 'Key', value: (p) => p.key, cell: (p) => <code className="studio-key">{p.key}</code> },
            { id: 'state', header: 'State', type: 'enum', options: ['Draft', 'In review', 'Published'], value: (p) => stateTag(p).label, cell: (p) => { const t = stateTag(p); return <Tag color={t.color} dot>{t.label}</Tag>; } },
            { id: 'published', header: 'Published', type: 'number', value: (p) => p.published?.version_no ?? null, cell: (p) => <span className="cell-muted">{p.published ? `v${p.published.version_no}` : '—'}</span> },
            { id: 'draft', header: 'Draft', type: 'number', value: (p) => p.draft?.version_no ?? null, cell: (p) => <span className="cell-muted">{p.draft ? `v${p.draft.version_no}${p.draft.lifecycle === 'in_review' ? ' · in review' : ''}` : '—'}</span> },
          ]} />
      )}
      {creating && <NewProcess onClose={() => setCreating(false)} onCreated={(id) => navigate(`/admin/processes/${id}`)} />}
    </>
  );
}

/** The list's state tag: a live process with a newer draft says so. */
export function stateTag(p: Pick<ProcessRow, 'state' | 'draft' | 'published'>) {
  if (p.published && p.draft) return { label: p.draft.lifecycle === 'in_review' ? 'Changes in review' : 'Published · draft open', color: 'green' as const };
  return STATE_TAG[p.state] ?? { label: p.state, color: 'gray' as const };
}

function NewProcess({ onClose, onCreated }: { onClose: () => void; onCreated: (id: string) => void }) {
  const [templates, , tplError] = useItems<ProcessTemplate>('/api/v1/process-templates');
  const [title, setTitle] = useState('');
  const [key, setKey] = useState('');
  const [keyEdited, setKeyEdited] = useState(false);
  const [template, setTemplate] = useState('blank');
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const effectiveKey = keyEdited ? key : keyFrom(title);
  const titleProblem = title.trim() ? null : 'Give the process a name.';
  const keyProblem = KEY_RE.test(effectiveKey) ? null : 'Use 2 to 64 lower-case letters, digits, dots, dashes or underscores, starting with a letter or digit.';
  const submit = async () => {
    setTouched(true);
    if (titleProblem || keyProblem || busy) return;
    setBusy(true);
    try {
      const { id } = await createProcess({ key: effectiveKey, title: title.trim(), template });
      onCreated(id);
    } catch (e) { setError(problemText(e, 'Could not create the process.')); setBusy(false); }
  };
  // Blank first, then the built-ins in the server's order.
  const options = [...(templates ?? [])].sort((a, b) => (a.key === 'blank' ? -1 : b.key === 'blank' ? 1 : 0))
    .map((t) => ({ id: t.key, label: t.name, description: t.description }));
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="New process" description="It starts as a draft only you and other designers see. Publish it when it’s ready." width={600}
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()} isDisabled={busy}>Create and open</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <Form validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); void submit(); }} style={{ display: 'contents' }}>
        <TextField label="Name" value={title} onChange={(v) => { setTitle(v); setError(null); }} isRequired autoFocus maxLength={120}
          description="What people ask for, e.g. “Laptop request”." isInvalid={touched && !!titleProblem} errorMessage={touched ? titleProblem ?? undefined : undefined} />
        <TextField label="Key" value={effectiveKey} onChange={(v) => { setKey(v.toLowerCase()); setKeyEdited(true); setError(null); }} isRequired maxLength={64}
          description="A permanent identifier for links and exports. It can’t change later." isInvalid={(touched || keyEdited) && !!keyProblem} errorMessage={(touched || keyEdited) ? keyProblem ?? undefined : undefined} />
        {tplError ? <Notice tone="warning" title="Couldn’t load the templates. You can still start blank." /> : templates === null ? <Skeleton variant="list" rows={2} label="Loading templates" /> : (
          <div className="studio-templates">
            <RadioCards label="Start from" value={template} onChange={setTemplate} options={options.length ? options : [{ id: 'blank', label: 'Blank', description: 'An empty request with no steps yet.' }]} />
          </div>
        )}
      </Form>
    </FormDialog>
  );
}
