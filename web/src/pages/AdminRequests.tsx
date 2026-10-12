import { useEffect, useMemo, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { Link, useRoute } from '../lib/router';
import { type RequestRow, type WorkItem, fieldErrors, people } from '../lib/requests';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { DataGrid, type GridColumn } from '../ui/components/DataGrid';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { TextField } from '../ui/components/TextField';
import { SectionTabs } from '../ui/SectionTabs';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';
import './requests.css';

/*
 * Admin › Requests oversight (BPMS-088, `/admin/requests`): steps nobody could be found for, steps
 * past their due date, and every open request. A stuck step can be reassigned to one person with a
 * reason; the server re-runs separation of duties and records it in the audit log.
 */

type Tab = 'unassigned' | 'overdue' | 'open';
const TABS: { id: Tab; label: string }[] = [
  { id: 'unassigned', label: 'Unassigned' },
  { id: 'overdue', label: 'Overdue' },
  { id: 'open', label: 'All open requests' },
];

export function AdminRequests() {
  const { query } = useRoute();
  const tab: Tab = (TABS.find((t) => t.id === query.get('tab'))?.id) ?? 'unassigned';
  return (
    <>
      <div className="page-head">
        <div><h1>Requests oversight</h1><p>Steps nobody could be found for, steps past their due date, and every request still in progress. Reassign a stuck step to keep it moving.</p></div>
      </div>
      <SectionTabs label="Oversight views" tabs={TABS.map((t) => ({ label: t.label, to: `/admin/requests?tab=${t.id}`, current: t.id === tab }))} />
      {tab === 'open' ? <OpenRequests /> : <StuckSteps key={tab} state={tab} />}
    </>
  );
}

function StuckSteps({ state }: { state: 'unassigned' | 'overdue' }) {
  const [rows, reload, error] = useItems<WorkItem>(`/api/v1/work?state=${state}`);
  const [target, setTarget] = useState<WorkItem | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const columns = useMemo<GridColumn<WorkItem>[]>(() => [
    { id: 'request', header: 'Request', value: (w) => w.request.title, cell: (w) => (
      <span className="cell-stack"><Link to={`/requests/${w.request.id}`}>{w.request.title}</Link><span className="cell-sub">{w.request.display_id} · {w.request.type_name}</span></span>
    ) },
    { id: 'step', header: 'Step', value: (w) => w.step },
    { id: 'initiator', header: 'Requested by', value: (w) => w.request.initiator.name, optional: true },
    { id: 'with', header: 'With', value: (w) => people(w.assignees ?? []), cell: (w) => (w.assignees?.length ? people(w.assignees) : <Tag color="orange">Nobody</Tag>) },
    { id: 'due', header: 'Due', type: 'date', value: (w) => w.due_at ?? '', cell: (w) => (w.due_at ? (
      <span className="cell-stack"><time dateTime={w.due_at}>{shortDate(w.due_at)}</time>{w.overdue && <span><Tag color="red" dot>Overdue</Tag></span>}</span>
    ) : <span className="cell-muted">No due date</span>) },
    { id: 'actions', header: 'Actions', sortable: false, value: () => '', cell: (w) => (
      <Button variant="secondary" onPress={() => { setDone(null); setTarget(w); }} aria-label={`Reassign ${w.step} of ${w.request.display_id}…`}>Reassign…</Button>
    ) },
  ], []);
  if (error) return <LoadError error={error} onRetry={reload} what={state === 'unassigned' ? 'unassigned steps' : 'overdue steps'} />;
  if (rows === null) return <Skeleton variant="table" rows={3} label="Loading steps" />;
  return (
    <>
      {done && <Notice tone="success" title={done} />}
      <DataGrid<WorkItem> caption={state === 'unassigned' ? 'Unassigned steps' : 'Overdue steps'} showCaption={false} data={rows} columns={columns} initialSort={[{ id: 'due', desc: false }]}
        empty={state === 'unassigned'
          ? <Empty art="caught-up" title="Every step has someone">When a step can’t find an approver (no manager, an empty group, everyone excluded), it waits here for you.</Empty>
          : <Empty art="caught-up" title="Nothing is overdue">Steps still open after their due date show up here, whoever has them.</Empty>} />
      <ReassignDialog item={target} onClose={() => setTarget(null)} onDone={(name) => { setDone(`${target?.request.display_id ?? 'The step'} is now with ${name}.`); setTarget(null); reload(); }} />
    </>
  );
}

function OpenRequests() {
  const [rows, reload, error] = useItems<RequestRow>('/api/v1/requests?scope=all&status=in_progress');
  const columns = useMemo<GridColumn<RequestRow>[]>(() => [
    { id: 'title', header: 'Request', value: (r) => r.title, cell: (r) => (
      <span className="cell-stack"><Link to={`/requests/${r.id}`}>{r.title}</Link><span className="cell-sub">{r.display_id} · {r.type_name}</span></span>
    ) },
    { id: 'initiator', header: 'Requested by', value: (r) => r.initiator.name },
    { id: 'step', header: 'Where it is', value: (r) => `${r.current_step ?? ''} ${people(r.assignees)}`, cell: (r) => (
      <span className="cell-stack"><span>{r.current_step ?? '—'}</span><span className="cell-sub">{r.assignees.length ? `with ${people(r.assignees)}` : 'with nobody'}</span></span>
    ) },
    { id: 'updated', header: 'Updated', type: 'date', value: (r) => r.updated_at, cell: (r) => <time className="cell-muted" dateTime={r.updated_at} title={shortDate(r.updated_at)}>{timeAgo(r.updated_at)}</time> },
  ], []);
  if (error) return <LoadError error={error} onRetry={reload} what="open requests" />;
  if (rows === null) return <Skeleton variant="table" rows={4} label="Loading requests" />;
  return (
    <DataGrid<RequestRow> caption="Open requests" showCaption={false} data={rows} columns={columns} rowHref={(r) => `/requests/${r.id}`} initialSort={[{ id: 'updated', desc: false }]}
      empty={<Empty art="caught-up" title="Nothing in progress">Requests waiting on someone show up here.</Empty>} />
  );
}

/** Pick one person and say why; SoD refusals come back on the person field. */
function ReassignDialog({ item, onClose, onDone }: { item: WorkItem | null; onClose: () => void; onDone: (name: string) => void }) {
  const { can } = useSession();
  const [members] = useItems<Member>(item && can('member:view') ? '/api/v1/members' : null, [item?.id]);
  const [who, setWho] = useState('');
  const [reason, setReason] = useState('');
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { setWho(''); setReason(''); setErrors({}); setFailure(null); }, [item?.id]);
  const options = (members ?? []).filter((m) => m.status === 'active').map((m) => ({ id: m.id, label: `${m.display_name} (${m.email})` }));
  const submit = async () => {
    if (!item) return;
    const missing: Record<string, string> = {};
    if (!who) missing.membership_id = 'Choose who should take this step.';
    if (!reason.trim()) missing.reason = 'Say why, for the audit log and the people concerned.';
    setErrors(missing); setFailure(null);
    if (Object.keys(missing).length) return;
    setBusy(true);
    try {
      const r = await api<{ assignees: string[] }>(`/api/v1/work/${item.id}/reassign`, { method: 'POST', body: { membership_id: who, reason: reason.trim() } });
      onDone(r.assignees.join(', '));
    } catch (e) {
      const fe = fieldErrors(e);
      if (Object.keys(fe).length) setErrors(fe);
      else setFailure(e instanceof ApiError && e.code === 'already_decided' ? 'Someone decided this step already. Close this and refresh the list.' : e instanceof Error ? e.message : 'Couldn’t reassign the step.');
    } finally { setBusy(false); }
  };
  return (
    <FormDialog isOpen={!!item} onOpenChange={(o) => !o && onClose()} title="Reassign step"
      description={item ? `${item.step} of ${item.request.display_id} (${item.request.title}). The new person is notified; the change is recorded in the audit log and the request’s timeline.` : undefined}
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()} isDisabled={busy}>Reassign</Button></>}>
      {failure && <Notice tone="danger" title={failure} />}
      <div className="stack">
        <Select label="Person" isRequired options={options} selectedKey={who || null} onSelectionChange={(k) => setWho(String(k ?? ''))}
          placeholder={members === null ? 'Loading people…' : 'Choose a person'} isInvalid={!!errors.membership_id} errorMessage={errors.membership_id}
          description="The person who started the request, and anyone separation of duties excludes, can’t take it." />
        <TextField label="Reason" isRequired value={reason} onChange={setReason} isInvalid={!!errors.reason} errorMessage={errors.reason}
          description="For example “Manager on leave until Monday”." />
      </div>
    </FormDialog>
  );
}
