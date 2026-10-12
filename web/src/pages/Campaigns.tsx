import { BellRing, Download, FileText, Lock, Plus, Rocket, ScrollText, XCircle } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api, download } from '../lib/api';
import { type Artifact, useItems, useResource } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { Link, navigate, useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import { setCrumb } from '../ui/AppShell';
import { type Audience, AudiencePicker, EVERYONE, audienceOf, audienceSummary, isEmptyAudience, useDirectory } from '../ui/Audience';
import { Avatar } from '../ui/Avatar';
import { Bars } from '../ui/Bars';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { KIND_TAG, Tag, type TagColor } from '../ui/Tag';
import { term, useTerms } from '../lib/terms';

interface Progress { assigned: number; acknowledged: number; pending: number; overdue: number }
export interface Campaign {
  id: string; title: string; state: string; audience: unknown; ack_text: string; due_at: string | null; due_in_secs: number; created_at: string; launched_at: string | null; ended_at: string | null; end_reason: string | null;
  subject: { artifact_id?: string; announcement_id?: string; kind: string; title: string; version_id?: string; version_no?: number; semver?: string | null; link?: string }; progress: Progress; audience_preview?: number; origin?: string; audience_snapshot?: 'at_launch' | 'dynamic';
}
interface Recipient { id: string; name: string; email: string; department: string | null; state: string; due_at: string; overdue: boolean; acknowledged_at: string | null; reminders_sent: number }

const STATE: Record<string, { label: string; color: TagColor }> = {
  draft: { label: 'Draft', color: 'gray' }, active: { label: 'Active', color: 'blue' }, closed: { label: 'Closed', color: 'green' }, cancelled: { label: 'Cancelled', color: 'brown' },
};
const TABS = [['all', 'All'], ['draft', 'Drafts'], ['active', 'Active'], ['closed', 'Closed'], ['cancelled', 'Cancelled']] as const;
const pct = (p: Progress) => (p.assigned ? Math.round((p.acknowledged / p.assigned) * 100) : 0);

function Meter({ p, label }: { p: Progress; label: string }) {
  const v = pct(p);
  return (
    <span className="meter" role="meter" aria-valuemin={0} aria-valuemax={100} aria-valuenow={v} aria-label={label}>
      <span className="meter-bar"><span style={{ width: `${v}%` }} /></span>
      <span className="meter-num">{p.acknowledged} of {p.assigned}</span>
    </span>
  );
}

/** Admin · Campaigns list (docs/05 §3.24): source, audience, completion, overdue, deadline. */
export function Campaigns() {
  useTerms(); // the workspace's terms (CFG-008)
  const { can } = useSession();
  const { query } = useRoute();
  const [items, reload, error] = useItems<Campaign>('/api/v1/campaigns');
  const [tab, setTab] = useState<string>(query.get('status') ?? 'all');
  const [creating, setCreating] = useState(query.get('new') === '1');
  useEffect(() => { if (query.get('new') === '1') setCreating(true); }, [query]);
  const canManage = can('campaign:manage');
  const rows = useMemo(() => (items ?? []).filter((c) => tab === 'all' || c.state === tab), [items, tab]);
  const count = (k: string) => (k === 'all' ? items?.length ?? 0 : (items ?? []).filter((c) => c.state === k).length);
  const dir = useDirectory();

  return (
    <>
      <div className="page-head">
        <div><h1>Campaigns</h1><p>Ask people to read and acknowledge a published policy or {term('sop')}, then keep the evidence.</p></div>
        {canManage && <Button onPress={() => setCreating(true)}><Plus aria-hidden="true" size="1rem" /> New campaign</Button>}
      </div>
      {items && items.length > 0 && (
        <div className="toolbar">
          <div className="segmented seg-scroll" role="group" aria-label="Filter by status">
            {TABS.filter(([k]) => k === 'all' || count(k) > 0).map(([k, label]) => <button key={k} type="button" aria-pressed={tab === k} onClick={() => setTab(k)}>{label} <span className="seg-count">{count(k)}</span></button>)}
          </div>
          {tab === 'active' && <p className="muted" role="status">{rows.reduce((n, c) => n + c.progress.pending, 0)} still to acknowledge</p>}
        </div>
      )}
      {error ? <LoadError error={error} onRetry={reload} what="campaigns" /> : items === null ? <Skeleton variant="table" rows={4} label="Loading campaigns" /> : rows.length === 0 ? (
        <Empty art="campaigns" title="No campaigns yet" action={canManage ? <Button onPress={() => setCreating(true)}><Plus aria-hidden="true" size="1rem" /> New campaign</Button> : undefined}>
          A campaign asks a group of people to read a published document and confirm it, with reminders and an evidence export.
        </Empty>
      ) : (
        <div className="table-wrap">
          <table className="table" aria-label="Campaigns">
            <thead><tr><th>Campaign</th><th>Status</th><th>Acknowledged</th><th className="col-optional">Overdue</th><th className="col-optional">Audience</th><th className="col-optional">Due</th></tr></thead>
            <tbody>
              {rows.map((c) => (
                <tr key={c.id}>
                  <td>
                    <span className="cell-entity">
                      <span className={`doc-icon tag-${KIND_TAG[c.subject.kind]?.color ?? 'gray'}`}>{c.subject.kind === 'sop' ? <ScrollText aria-hidden="true" size="0.875rem" /> : <FileText aria-hidden="true" size="0.875rem" />}</span>
                      <span className="cell-stack"><Link to={`/admin/campaigns/${c.id}`}>{c.title}</Link><span className="cell-sub">{c.subject.title}{c.subject.version_no ? ` v${c.subject.version_no}` : ' · announcement'}</span></span>
                    </span>
                  </td>
                  <td><Tag color={STATE[c.state]?.color} dot>{STATE[c.state]?.label ?? c.state}</Tag></td>
                  <td>{c.state === 'draft' ? <span className="cell-muted">Not started</span> : <Meter p={c.progress} label={`${pct(c.progress)}% acknowledged`} />}</td>
                  <td className="col-optional">{c.progress.overdue ? <Tag color="red">{c.progress.overdue} overdue</Tag> : <span className="cell-muted">None</span>}</td>
                  <td className="col-optional cell-muted">{audienceSummary(audienceOf(c.audience), dir)}</td>
                  <td className="col-optional cell-muted">{c.due_at ? shortDate(c.due_at) : `${Math.round(c.due_in_secs / 86400)} days after launch`}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <NewCampaign isOpen={creating} initialDoc={query.get('doc')} initialDueDays={Number(query.get('due_days')) || null} onClose={() => { setCreating(false); if (query.get('new')) navigate('/admin/campaigns', { replace: true }); }} onCreated={(id) => navigate(`/admin/campaigns/${id}`)} />
    </>
  );
}

/** Create a draft campaign from a published document version. */
function NewCampaign({ isOpen, initialDoc = null, initialDueDays = null, onClose, onCreated }: { isOpen: boolean; initialDoc?: string | null; initialDueDays?: number | null; onClose: () => void; onCreated: (id: string) => void }) {
  useTerms(); // the workspace's terms (CFG-008)
  const [docs] = useItems<Artifact>(isOpen ? '/api/v1/artifacts' : null, [isOpen]);
  const published = (docs ?? []).filter((d) => d.current_published_version_id);
  const [doc, setDoc] = useState<string>('');
  const [title, setTitle] = useState('');
  const [titleTouched, setTitleTouched] = useState(false);
  const [audience, setAudience] = useState<Audience>(EVERYONE);
  const [due, setDue] = useState('');
  const [joiners, setJoiners] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // A template's campaign plan links here with the document and deadline filled in (BPMS-106).
  const dueFrom = (days: number | null) => { if (!days || days < 1 || days > 366) return ''; const d = new Date(Date.now() + days * 86_400_000); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`; };
  useEffect(() => { if (isOpen) { setDoc(initialDoc ?? ''); setTitle(''); setTitleTouched(false); setAudience(EVERYONE); setDue(dueFrom(initialDueDays)); setError(null); } }, [isOpen, initialDoc, initialDueDays]);
  const chosen = published.find((d) => d.id === doc);
  useEffect(() => { if (chosen && !titleTouched) setTitle(`Acknowledge ${chosen.title}`); }, [chosen, titleTouched]);
  const create = async () => {
    if (!chosen) return;
    setBusy(true); setError(null);
    try {
      const c = await api<Campaign>('/api/v1/campaigns', { method: 'POST', body: { version_id: chosen.current_published_version_id, title: title.trim() || undefined, audience, audience_snapshot: joiners ? 'dynamic' : 'at_launch', due_at: due ? new Date(`${due}T23:59:00`).toISOString() : undefined } });
      onClose(); onCreated(c.id);
    } catch (e) { setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not create the campaign.'); } finally { setBusy(false); }
  };
  return (
    <FormDialog isOpen={isOpen} onOpenChange={(o) => !o && onClose()} width={640} title="New campaign" description="It starts as a draft. You can check who it reaches before you launch it."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void create()} isDisabled={!chosen || isEmptyAudience(audience) || busy}>{busy ? 'Creating…' : 'Create draft'}</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <Form className="stack compose" onSubmit={(e) => { e.preventDefault(); void create(); }}>
        <div className="field" role="group" aria-labelledby="doc-pick-label">
          <span className="field-label" id="doc-pick-label">Document</span>
          {docs === null ? <Skeleton rows={2} label="Loading documents" /> : published.length === 0 ? (
            <p className="aud-note">Nothing is published yet. Publish a policy or {term('sop')} first, then come back.</p>
          ) : (
            <div className="doc-pick">
              {published.map((d) => (
                <button key={d.id} type="button" className="doc-pick-item" aria-pressed={doc === d.id} onClick={() => setDoc(d.id)}>
                  <span className={`doc-icon doc-icon-sm tag-${KIND_TAG[d.kind]?.color ?? 'gray'}`}>{d.kind === 'sop' ? <ScrollText aria-hidden="true" size="0.8125rem" /> : <FileText aria-hidden="true" size="0.8125rem" />}</span>
                  <span className="doc-pick-title">{d.title}</span>
                  <span className="doc-pick-kind">{KIND_TAG[d.kind]?.label}</span>
                </button>
              ))}
            </div>
          )}
          <span className="field-description">People acknowledge the current published version. A later version needs a new campaign.</span>
        </div>
        <TextField label="Name" value={title} onChange={(v) => { setTitle(v); setTitleTouched(true); }} description="Shown to people in their task list." />
        <AudiencePicker value={audience} onChange={setAudience} />
        <label className="check-row"><input type="checkbox" checked={joiners} onChange={(e) => setJoiners(e.target.checked)} /><span><strong>Also ask people who join later</strong><small>New members in this audience get the request while the campaign is open.</small></span></label>
        <div className="field"><label className="field-label" htmlFor="camp-due">Due on <span className="muted">(optional)</span></label><input id="camp-due" type="date" className="field-input" value={due} onChange={(e) => setDue(e.target.value)} /><span className="field-description">Leave empty to use the workspace default, counted from launch.</span></div>
      </Form>
    </FormDialog>
  );
}

/** Campaign page (docs/05 §3.24): completion, by-department table, recipients, evidence. */
export function CampaignDetail({ id }: { id: string }) {
  const { can } = useSession();
  const [c, reload, error] = useResource<Campaign>(`/api/v1/campaigns/${id}`);
  const [filter, setFilter] = useState<'all' | 'pending' | 'overdue' | 'completed'>('all');
  const [recipients, reloadRecipients] = useItems<Recipient>(c && c.state !== 'draft' ? `/api/v1/campaigns/${id}/recipients` : null, [c?.state]);
  const [toast, setToast] = useState<string | null>(null);
  const [actError, setActError] = useState<string | null>(null);
  const [ending, setEnding] = useState<null | 'close' | 'cancel'>(null);
  const [reason, setReason] = useState('');
  const dir = useDirectory();
  const canManage = can('campaign:manage');
  useEffect(() => { if (c) setCrumb(c.title); return () => setCrumb(null); }, [c]);
  useEffect(() => { if (!toast) return; const t = setTimeout(() => setToast(null), 3500); return () => clearTimeout(t); }, [toast]);

  const act = async <T,>(fn: () => Promise<T>, done: string | ((r: T) => string)) => {
    setActError(null);
    try { const r = await fn(); setToast(typeof done === 'string' ? done : done(r)); setEnding(null); reload(); reloadRecipients(); }
    catch (e) { setActError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Something went wrong. Try again.'); }
  };

  if (error) return <LoadError error={error} onRetry={reload} what="this campaign" />;
  if (!c) return <Skeleton variant="doc" label="Loading campaign" />;
  const p = c.progress;
  const rows = (recipients ?? []).filter((r) => filter === 'all' || (filter === 'overdue' ? r.overdue : filter === 'pending' ? r.state === 'pending' && !r.overdue : r.state === 'completed'));
  // People who left the organisation stay listed as evidence but are outside the counts (docs/22 A-44).
  const byDept = Object.values((recipients ?? []).filter((r) => r.state !== 'left').reduce<Record<string, { label: string; value: number; total: number }>>((acc, r) => {
    const k = r.department ?? 'No department';
    acc[k] ??= { label: k, value: 0, total: 0 };
    acc[k].total++; if (r.state === 'completed') acc[k].value++;
    return acc;
  }, {}));
  const live = c.state !== 'draft';

  return (
    <div className="camp">
      <div className="page-head">
        <div>
          <div className="drawer-tags"><Tag color={STATE[c.state]?.color} dot>{STATE[c.state]?.label ?? c.state}</Tag>{c.origin === 'reack' && <Tag color="purple">Started by publishing</Tag>}</div>
          <h1>{c.title}</h1>
          <p>{c.subject.kind === 'announcement' ? 'Confirm reading' : 'Acknowledge'} <Link to={c.subject.link ?? `/library/${c.subject.artifact_id}`}>{c.subject.title}</Link>{c.subject.kind === 'announcement' ? '' : ` v${c.subject.semver ?? c.subject.version_no}`} · {audienceSummary(audienceOf(c.audience), dir)}{c.due_at ? ` · due ${shortDate(c.due_at)}` : ''}</p>
        </div>
        <div className="actions">
          {live && <Button variant="secondary" onPress={() => void download(`/api/v1/campaigns/${c.id}/evidence`, `campaign-${c.id}-evidence.csv`).then(() => setToast('Evidence downloaded.')).catch((e: unknown) => setActError(e instanceof Error ? e.message : 'Download failed.'))}><Download aria-hidden="true" size="0.9375rem" /> Evidence CSV</Button>}
          {canManage && c.state === 'active' && p.pending > 0 && <Button variant="secondary" onPress={() => void act(() => api<{ reminded: number }>(`/api/v1/campaigns/${c.id}/remind`, { method: 'POST' }), (r) => `Reminded ${r.reminded} ${r.reminded === 1 ? 'person' : 'people'}.`)}><BellRing aria-hidden="true" size="0.9375rem" /> Remind pending</Button>}
          {canManage && c.state === 'active' && <Button variant="secondary" onPress={() => { setReason(''); setEnding('close'); }}><Lock aria-hidden="true" size="0.875rem" /> Close</Button>}
          {canManage && c.state === 'draft' && <Button variant="ghost" onPress={() => { setReason(''); setEnding('cancel'); }}><XCircle aria-hidden="true" size="0.9375rem" /> Cancel</Button>}
          {canManage && c.state === 'draft' && <Button onPress={() => void act(() => api(`/api/v1/campaigns/${c.id}/activate`, { method: 'POST' }), 'Launched. People have been notified.')}><Rocket aria-hidden="true" size="0.9375rem" /> Launch</Button>}
        </div>
      </div>
      {actError && <Notice tone="danger" title={actError} />}

      {!live ? (
        <div className="camp-draft">
          <p className="camp-big">{c.audience_preview ?? 0} <span>{c.audience_preview === 1 ? 'person' : 'people'} will be asked to acknowledge</span></p>
          <p className="muted">Launching notifies them and starts the clock{c.due_at ? '' : `: ${Math.round(c.due_in_secs / 86400)} days to complete`}. You can remind people and export the evidence any time.</p>
          <p className="camp-ack"><span className="muted">They confirm:</span> “{c.ack_text}”</p>
          <p className="muted">{c.audience_snapshot === 'at_launch' ? 'Only the people in the audience at launch are asked.' : 'People who join the audience later are asked too, while it is open.'}</p>
        </div>
      ) : (
        <>
          <section className="camp-summary" aria-label="Progress">
            <div className="camp-progress"><span style={{ width: `${pct(p)}%` }} /></div>
            <dl className="camp-figures">
              <div><dt>Acknowledged</dt><dd>{pct(p)}% <span>{p.acknowledged} of {p.assigned}</span></dd></div>
              <div><dt>Pending</dt><dd>{p.pending - p.overdue}</dd></div>
              <div><dt>Overdue</dt><dd className={p.overdue ? 'is-bad' : ''}>{p.overdue}</dd></div>
              <div><dt>{c.state === 'active' ? 'Launched' : 'Ended'}</dt><dd className="small">{c.state === 'active' ? timeAgo(c.launched_at) : shortDate(c.ended_at)}</dd></div>
            </dl>
            {c.end_reason && <p className="muted">Reason: {c.end_reason}</p>}
          </section>
          {byDept.length > 1 && <section className="camp-section"><Bars caption="Acknowledged by department" rows={byDept} unit="done" /></section>}
          <section className="camp-section" aria-labelledby="rec-h">
            <div className="section-head"><h2 id="rec-h">People</h2></div>
            <div className="toolbar">
              <div className="segmented" role="group" aria-label="Filter people">
                {([['all', 'All'], ['pending', 'Pending'], ['overdue', 'Overdue'], ['completed', 'Done']] as const).map(([k, label]) => <button key={k} type="button" aria-pressed={filter === k} onClick={() => setFilter(k)}>{label}</button>)}
              </div>
            </div>
            {recipients === null ? <Skeleton variant="table" rows={4} label="Loading people" /> : rows.length === 0 ? <Empty compact art="no-match" title="Nobody in this list" /> : (
              <div className="table-wrap">
                <table className="table" aria-label="People in this campaign">
                  <thead><tr><th>Name</th><th>Status</th><th className="col-optional">Department</th><th className="col-optional">Acknowledged</th><th className="col-optional">Reminders</th></tr></thead>
                  <tbody>
                    {rows.map((r) => (
                      <tr key={r.id}>
                        <td><span className="cell-entity"><Avatar name={r.name} size="1.5rem" /><span className="cell-stack"><span>{r.name}</span><span className="cell-sub">{r.email}</span></span></span></td>
                        <td>{r.state === 'completed' ? <Tag color="green" dot>Done</Tag> : r.state === 'left' ? <Tag color="gray" dot>Left organisation</Tag> : r.overdue ? <Tag color="red" dot>Overdue</Tag> : <Tag color="yellow" dot>Pending</Tag>}</td>
                        <td className="col-optional cell-muted">{r.department ?? ''}</td>
                        <td className="col-optional cell-muted">{r.acknowledged_at ? shortDate(r.acknowledged_at) : `due ${shortDate(r.due_at)}`}</td>
                        <td className="col-optional cell-muted">{r.reminders_sent || ''}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
        </>
      )}
      <FormDialog isOpen={ending !== null} onOpenChange={(o) => !o && setEnding(null)} title={ending === 'close' ? 'Close this campaign?' : 'Cancel this draft?'}
        description={ending === 'close' ? 'Nobody else can acknowledge it. The results are frozen as evidence.' : 'It will not be launched. You can create a new one later.'}
        footer={<><Button variant="secondary" onPress={() => setEnding(null)}>Keep it</Button><Button variant={ending === 'cancel' ? 'danger' : 'primary'} onPress={() => void act(() => api(`/api/v1/campaigns/${c.id}/${ending}`, { method: 'POST', body: reason.trim() ? { reason } : {} }), ending === 'close' ? 'Campaign closed.' : 'Draft cancelled.')}>{ending === 'close' ? 'Close campaign' : 'Cancel draft'}</Button></>}>
        <TextField label="Reason (optional)" description="Recorded in the audit log." value={reason} onChange={setReason} autoFocus />
      </FormDialog>
      {toast && <div className="toast" role="status">{toast}</div>}
    </div>
  );
}
