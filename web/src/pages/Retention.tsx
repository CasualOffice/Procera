import { Archive, ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type LoadFailure, toFailure } from '../lib/data';
import { shortDate } from '../lib/format';
import { Link } from '../lib/router';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { StepUpDialog } from '../ui/StepUp';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';

interface RetentionClass { class: string; label: string; covers: string; mode: 'automatic' | 'never'; keep_days: number | null; measured_from: string | null; setting: string | null; notice_days?: number }
interface DueDraft { artifact_id: string; version_id: string; title: string; version_no: number; author: string | null; last_activity_at: string; deletes_on: string }
interface Overview {
  classes: RetentionClass[];
  due_drafts: DueDraft[];
  last_run: { at: string; purged: { draft: number; operational: { emails: number; notifications: number; digest_items: number }; notices: number } } | null;
}

const period = (d: number | null) => (d === null ? 'For good' : d % 365 === 0 ? `${d / 365} year${d === 365 ? '' : 's'}` : `${d} days`);
const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? '' : 's'}`;

/** Admin · Retention (BPMS-066, docs/14 §4): what each kind of record keeps, for how long, and what the job removes next. */
export function Retention() {
  const { can } = useSession();
  const [data, setData] = useState<Overview | null>(null);
  const [failed, setFailed] = useState<LoadFailure | null>(null);
  const [editing, setEditing] = useState<{ setting: string; label: string; days: number } | null>(null);
  const load = () => { setFailed(null); api<Overview>('/api/v1/admin/retention').then(setData).catch((e) => setFailed(toFailure(e))); };
  useEffect(load, []);

  if (failed) return <LoadError error={failed} onRetry={load} what="retention" />;
  if (!data) return <Skeleton rows={4} label="Loading retention" />;
  const canEdit = can('workspace:settings.manage');
  const last = data.last_run;
  const logs = last ? last.purged.operational.emails + last.purged.operational.notifications + last.purged.operational.digest_items : 0;

  return (
    <>
      <div className="page-head">
        <div><h1>Retention</h1><p>How long each kind of record is kept, and what the daily job removes.</p></div>
      </div>
      <div className="table-wrap">
        <table className="table" aria-label="Retention classes">
          <thead><tr><th>Records</th><th>Kept for</th><th>When it ends</th>{canEdit && <th><span className="visually-hidden">Actions</span></th>}</tr></thead>
          <tbody>
            {data.classes.map((c) => (
              <tr key={c.class}>
                <td><span style={{ display: 'block', fontWeight: 500 }}>{c.label}</span><span className="cell-muted" style={{ fontSize: '0.8125rem' }}>{c.covers}</span></td>
                <td>
                  {period(c.keep_days)}{c.measured_from && <span className="cell-muted"> after {c.measured_from}</span>}
                  {c.notice_days !== undefined && <span className="cell-muted" style={{ display: 'block', fontSize: '0.8125rem' }}>Authors are told {c.notice_days} days ahead</span>}
                </td>
                <td>{c.mode === 'automatic' ? <Tag color="yellow">Deleted automatically</Tag> : <Tag color="green">Never deleted by Procera</Tag>}</td>
                {canEdit && <td className="cell-actions">{c.setting && c.mode === 'automatic' && <Button variant="secondary" aria-label={`Change retention for ${c.label.toLowerCase()}`} onPress={() => setEditing({ setting: c.setting!, label: c.label, days: c.keep_days ?? 0 })}>Change</Button>}</td>}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="muted" style={{ fontSize: '0.8125rem', marginTop: '0.75rem' }}>
        {last ? <>Last run {shortDate(last.at)}: {plural(last.purged.draft, 'draft')} and {plural(logs, 'log entry').replace('entrys', 'entries')} removed, {plural(last.purged.notices, 'notice')} sent. Every run is in the <Link to="/admin/audit">audit log</Link>. </> : 'Nothing has needed removing yet. '}
        Governance records stay until disposition review arrives in a later release.
      </p>

      <section aria-labelledby="due-h" style={{ marginTop: '2rem' }}>
        <h2 id="due-h" style={{ fontSize: '0.9375rem', margin: '0 0 0.75rem' }}>Drafts due for deletion in the next 30 days</h2>
        {data.due_drafts.length === 0 ? (
          <Empty icon={ShieldCheck} title="No drafts are due">Drafts nobody has edited for the retention period appear here before they are deleted. Editing a draft resets its clock.</Empty>
        ) : (
          <div className="table-wrap">
            <table className="table" aria-label="Drafts due for deletion">
              <thead><tr><th>Draft</th><th className="col-optional">Author</th><th>Last edited</th><th>Deleted on</th></tr></thead>
              <tbody>{data.due_drafts.map((d) => (
                <tr key={d.version_id}>
                  <td><Link to={`/library/${d.artifact_id}`}>{d.title}</Link> <span className="cell-muted">v{d.version_no}</span></td>
                  <td className="cell-muted col-optional">{d.author ?? 'Unknown'}</td>
                  <td className="cell-muted">{shortDate(d.last_activity_at)}</td>
                  <td><Tag color="yellow"><Archive aria-hidden="true" size="0.75rem" /> {shortDate(d.deletes_on)}</Tag></td>
                </tr>
              ))}</tbody>
            </table>
          </div>
        )}
      </section>
      {editing && <EditPeriod {...editing} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); load(); }} />}
    </>
  );
}

function EditPeriod({ setting, label, days, onClose, onSaved }: { setting: string; label: string; days: number; onClose: () => void; onSaved: () => void }) {
  const [value, setValue] = useState(String(days));
  const [reason, setReason] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [stepUp, setStepUp] = useState(false);
  const n = Number(value);
  const save = async () => {
    setError(null);
    try {
      const current = await api<{ revision: number }>(`/api/v1/settings/${setting}`);
      await api(`/api/v1/settings/${setting}`, { method: 'PUT', headers: { 'if-match': `"${current.revision}"` }, body: { value: n * 86_400, reason: reason.trim() } });
      onSaved();
    } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(true);
      else setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not save the period.');
    }
  };
  return (
    <>
      <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={`Retention for ${label.toLowerCase()}`} description="Changes apply from the next run. The reason is kept in the audit log."
        footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void save()} isDisabled={!(n >= 1) || !reason.trim()}>Save</Button></>}>
        {error && <Notice tone="danger" title={error} />}
        <TextField label="Keep for (days)" type="number" value={value} onChange={setValue} autoFocus />
        <TextField label="Reason" value={reason} onChange={setReason} />
      </FormDialog>
      <StepUpDialog isOpen={stepUp} onClose={() => setStepUp(false)} onDone={() => { setStepUp(false); void save(); }} />
    </>
  );
}
