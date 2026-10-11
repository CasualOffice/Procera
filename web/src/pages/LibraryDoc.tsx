import { CollectionRow, OwnerRow, RaciRows } from './docOrganisation';
import { BookOpenCheck, Check, CircleDot, FileText, GitBranch, Hash, Pencil, ScrollText, Send, ShieldCheck, User } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type Member, memberMap, useItems, useResource } from '../lib/data';
import { type Block, blocksToText, shortDate, textToBlocks, timeAgo } from '../lib/format';
import { Link } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { RadioCards } from '../ui/components/RadioCards';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { DocBody } from '../ui/Doc';
import { StepUpDialog } from '../ui/StepUp';
import { setCrumb } from '../ui/AppShell';
import { rememberRecent } from '../lib/recent';
import { LoadError, Skeleton } from '../ui/States';
import { KIND_TAG, STATE_TAG, Tag } from '../ui/Tag';
import { ChangeRequests, type Governance, GovernanceRows, ReviewCallout } from './docGovernance';
import { ApplicabilityRow } from './docApplicability';

interface ArtifactResp { artifact: { id: string; kind: string; key: string; title: string; state: string; criticality: string; owner_membership_id: string | null; current_published_version_id: string | null; governance?: Governance; category?: string | null; applicability?: unknown; restricted?: boolean; collection_id?: string | null }; versions: { id: string; version_no: number; lifecycle: string; published_at: string | null; change_reason: string | null }[] }
interface Version { id: string; version_no: number; lifecycle: string; content: { blocks?: Block[] }; content_hash: string | null; created_by: string; created_at: string; published_at: string | null; revision: number; reviews: { reviewer_membership_id: string; decision: string; comment: string | null; decided_at: string }[]; approvals_required: number | null; change_reason: string | null; semver?: string | null; reack_required?: boolean }
/** What publishing would do (BPMS-045): numbers per change, who is asked to re-acknowledge and by when. */
interface PublishPreview { version_no: number; next_semver_major: string; next_semver_minor: string; reack_required_if_major: boolean; reack_required_if_minor: boolean; audience_size: number; due_at: string }
interface PublishResult { semver: string; reack: { required: boolean; due_at: string | null; audience_size: number | null; campaign_id: string | null } }
interface ReackCampaign { id: string; origin: string; state: string; due_at: string | null; subject: { version_id: string }; progress: { assigned: number; acknowledged: number } }
type Change = 'major' | 'minor';
const people = (n: number) => `${n.toLocaleString()} ${n === 1 ? 'person' : 'people'}`;

/** The publish dialog's consequence line ("Publishing v3.0 asks 1,240 people to re-acknowledge by 30 Nov"). */
function reackSentence(p: PublishPreview, change: Change): string {
  if (p.version_no <= 1) return 'Publishing v1.0 makes it the version people read. Nobody is asked to acknowledge it automatically.';
  const next = change === 'major' ? p.next_semver_major : p.next_semver_minor;
  const asks = change === 'major' ? p.reack_required_if_major : p.reack_required_if_minor;
  if (!asks) return `Publishing v${next} does not ask anyone to re-acknowledge.`;
  if (!p.audience_size) return `Publishing v${next} asks for a re-acknowledgement, but nobody it applies to is an active member yet.`;
  return `Publishing v${next} asks ${people(p.audience_size)} to re-acknowledge by ${shortDate(p.due_at)}.`;
}

interface Ack { id: string; due_at: string; ack_text: string; subject: { artifact_id: string; version_id: string; content_hash: string; version_no: number } }

/** A governed document (docs/05 §3.21): reading view, properties, lifecycle actions, acknowledgement. */
export function LibraryDoc({ id }: { id: string }) {
  const { can, workspace } = useSession();
  const [data, setData] = useState<ArtifactResp | null>(null);
  const [version, setVersion] = useState<Version | null>(null);
  const [notFound, setNotFound] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [dialog, setDialog] = useState<null | 'changes' | 'publish'>(null);
  const [comment, setComment] = useState('');
  const [stepUp, setStepUp] = useState<null | (() => Promise<void>)>(null);
  const [members] = useItems<Member>(can('member:view') ? '/api/v1/members' : null);
  const [acks, reloadAcks] = useItems<Ack>('/api/v1/me/acknowledgements?state=pending');
  const [change, setChange] = useState<Change>('major');
  const [preview] = useResource<PublishPreview>(dialog === 'publish' && version ? `/api/v1/versions/${version.id}/publish-preview` : null, [dialog]);
  const [campaigns, reloadCampaigns] = useItems<ReackCampaign>(can('campaign:view') ? '/api/v1/campaigns' : null);
  const directory = memberMap(members);

  const load = useCallback(async () => {
    try {
      const a = await api<ArtifactResp>(`/api/v1/artifacts/${id}`);
      setData(a);
      const latest = a.versions[0];
      if (latest) setVersion(await api<Version>(`/api/v1/versions/${latest.id}`));
    } catch (e) {
      if (e instanceof ApiError && e.status === 404) setNotFound(true);
      else setError(e instanceof Error ? e.message : 'Could not load.');
    }
  }, [id]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    if (data) {
      setCrumb(data.artifact.title);
      document.title = `${data.artifact.title} · Procera`;
      rememberRecent(workspace?.workspace_id, { kind: data.artifact.kind, id: data.artifact.id, title: data.artifact.title, to: `/library/${data.artifact.id}` });
    }
    return () => setCrumb(null);
  }, [data, workspace?.workspace_id]);

  /** Runs an action; on `step_up_required` asks to verify and retries once. */
  const act = async (fn: () => Promise<unknown>, done: string | (() => string)): Promise<boolean> => {
    setError(null);
    const run = async () => { await fn(); setToast(typeof done === 'string' ? done : done()); setEditing(false); setDialog(null); await load(); reloadAcks(); reloadCampaigns(); };
    try { await run(); return true; } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(() => async () => { try { await run(); } catch (err) { setError(err instanceof Error ? err.message : 'Failed.'); } });
      else setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Something went wrong.');
      return false;
    }
  };
  useEffect(() => { if (!toast) return; const t = setTimeout(() => setToast(null), 3500); return () => clearTimeout(t); }, [toast]);

  if (notFound) return <div className="page-head"><div><h1>Document not found</h1><p>It doesn’t exist or you don’t have access to it.</p></div></div>;
  if (!data && error) return <div className="doc"><LoadError error={{ message: error }} onRetry={() => { setError(null); void load(); }} what="this document" /></div>;
  if (!data || !version) return <div className="doc"><Skeleton variant="doc" label="Loading document" /></div>;

  const a = data.artifact;
  const kind = KIND_TAG[a.kind];
  const owner = a.owner_membership_id ? directory.get(a.owner_membership_id) : undefined;
  const author = directory.get(version.created_by);
  const blocks = version.content.blocks ?? [];
  const pendingAck = (acks ?? []).find((x) => x.subject.artifact_id === a.id);
  const v = version;
  const p = (action: string) => can(`${a.kind}:${action}`);
  const approvals = v.reviews.filter((r) => r.decision === 'approve').length;
  const reack = (campaigns ?? []).find((c) => c.origin === 'reack' && c.subject.version_id === a.current_published_version_id);
  const publish = () => {
    let done = 'Published.';
    return act(async () => {
      const body: Record<string, string> = {};
      if (comment.trim()) body.change_reason = comment;
      if (v.version_no > 1) body.change = change;
      const r = await api<PublishResult>(`/api/v1/versions/${v.id}/publish`, { method: 'POST', body });
      if (r.reack.campaign_id && r.reack.audience_size) done = `Published v${r.semver}. ${people(r.reack.audience_size)} asked to re-acknowledge.`;
    }, () => done);
  };

  return (
    <article className="doc">
      <div className="doc-kicker">
        <span className={`doc-icon tag-${kind?.color ?? 'gray'}`} style={{ width: '1.25rem', height: '1.25rem' }}>{a.kind === 'sop' ? <ScrollText aria-hidden="true" size="0.75rem" /> : <FileText aria-hidden="true" size="0.75rem" />}</span>
        <span>{kind?.label}</span>
      </div>
      <h1 className="doc-title">{a.title}</h1>

      <dl className="props">
        <dt><CircleDot aria-hidden="true" size="0.9375rem" /> Status</dt><dd><Tag color={STATE_TAG[v.lifecycle]?.color} dot>{STATE_TAG[v.lifecycle]?.label}</Tag>{a.criticality === 'controlled' && <Tag color="red">Controlled</Tag>}</dd>
        <dt><GitBranch aria-hidden="true" size="0.9375rem" /> Version</dt><dd>v{v.version_no}{v.semver && <span className="muted">· {v.semver}</span>}{v.published_at && <span className="muted">· published {shortDate(v.published_at)}</span>}{v.change_reason && <span className="muted">· {v.change_reason}</span>}</dd>
        <OwnerRow artifactId={a.id} owner={owner} canEdit={p('publish')} act={act} />
        <CollectionRow artifactId={a.id} collectionId={a.collection_id ?? null} canEdit={p('publish')} act={act} />
        <RaciRows artifactId={a.id} canEdit={p('publish')} act={act} />
        {v.lifecycle === 'in_review' && <><dt><ShieldCheck aria-hidden="true" size="0.9375rem" /> Approvals</dt><dd>{approvals} of {v.approvals_required ?? 1}{v.reviews.map((r) => <span key={r.reviewer_membership_id} className="person" title={r.decision}><Avatar name={directory.get(r.reviewer_membership_id)?.display_name ?? '?'} size="1.25rem" /></span>)}</dd></>}
        {a.governance && a.current_published_version_id && <GovernanceRows artifactId={a.id} g={a.governance} canRecertify={p('publish')} canVerify={p('approve')} act={act} />}
        {a.kind === 'policy' && (p('publish') || !!a.category || !!a.restricted || a.applicability != null) && <ApplicabilityRow artifactId={a.id} category={a.category ?? null} applicability={a.applicability ?? null} restricted={!!a.restricted} canEdit={p('publish')} act={act} />}
        {reack && <><dt><BookOpenCheck aria-hidden="true" size="0.9375rem" /> Acknowledged</dt><dd><Link to={`/admin/campaigns/${reack.id}`}>{reack.progress.acknowledged} of {people(reack.progress.assigned)}</Link><span className="muted">· re-acknowledgement{reack.due_at ? ` due ${shortDate(reack.due_at)}` : ''}</span></dd></>}
        {v.content_hash && <><dt><Hash aria-hidden="true" size="0.9375rem" /> Fingerprint</dt><dd><code className="mono" title="SHA-256 of the approved content">{v.content_hash.slice(0, 16)}…</code></dd></>}
      </dl>

      {error && <Notice tone="danger" title={error} />}
      {a.governance && a.current_published_version_id && p('publish') && <ReviewCallout artifactId={a.id} g={a.governance} act={act} />}
      {pendingAck && pendingAck.subject.version_id === v.id && (
        <div className="callout callout-info" role="region" aria-label="Acknowledgement requested">
          <BookOpenCheck aria-hidden="true" size="1.125rem" />
          <div><p><strong>Please read and acknowledge this version by {shortDate(pendingAck.due_at)}.</strong></p><p className="muted" style={{ fontSize: '0.8125rem' }}>{pendingAck.ack_text}</p></div>
          <div className="actions"><Button onPress={() => void act(() => api(`/api/v1/acknowledgements/${pendingAck.id}/complete`, { method: 'POST', body: { content_hash: pendingAck.subject.content_hash } }), 'Acknowledged. Thank you.')}><Check aria-hidden="true" size="1rem" /> I acknowledge</Button></div>
        </div>
      )}

      {/* Lifecycle actions for this version */}
      {!['published', 'deprecated', 'retired'].includes(v.lifecycle) && <Lifecycle state={v.lifecycle} hasContent={blocks.length > 0} approvals={approvals} required={v.approvals_required ?? 1} canReview={p('review') || p('approve')} canEdit={p('edit_draft')} canPublish={p('publish')} publishedAt={v.published_at} />}
      <div className="doc-actions">
        {v.lifecycle === 'draft' && p('edit_draft') && !editing && <Button variant="secondary" onPress={() => { setDraft(blocksToText(blocks)); setEditing(true); }}><Pencil aria-hidden="true" size="0.9375rem" /> Edit</Button>}
        {v.lifecycle === 'draft' && p('edit_draft') && !editing && <Button onPress={() => void act(() => api(`/api/v1/versions/${v.id}/submit`, { method: 'POST' }), 'Submitted.')} isDisabled={!blocks.length}><Send aria-hidden="true" size="0.9375rem" /> Submit for review</Button>}
        {v.lifecycle === 'in_review' && p('approve') && <Button onPress={() => void act(() => api(`/api/v1/versions/${v.id}/reviews`, { method: 'POST', body: { decision: 'approve' } }), 'Approved.')}><Check aria-hidden="true" size="0.9375rem" /> Approve</Button>}
        {v.lifecycle === 'in_review' && (p('review') || p('approve')) && <Button variant="secondary" onPress={() => { setComment(''); setDialog('changes'); }}>Request changes</Button>}
        {v.lifecycle === 'in_review' && p('edit_draft') && <Button variant="ghost" onPress={() => void act(() => api(`/api/v1/versions/${v.id}/withdraw`, { method: 'POST' }), 'Withdrawn to draft.')}>Withdraw</Button>}
        {v.lifecycle === 'approved' && p('publish') && <Button onPress={() => { setComment(''); setChange('major'); setDialog('publish'); }}><Send aria-hidden="true" size="0.9375rem" /> Publish</Button>}
        {['published', 'deprecated'].includes(v.lifecycle) && p('edit_draft') && <Button variant="secondary" onPress={() => void act(() => api(`/api/v1/artifacts/${a.id}/versions`, { method: 'POST' }), 'New draft created.')}><Pencil aria-hidden="true" size="0.9375rem" /> New version</Button>}
        <span className="muted doc-meta">{author ? `Edited by ${author.display_name} · ` : ''}{timeAgo(v.created_at)} · {workspace?.name}</span>
      </div>
      <hr className="doc-rule" />

      {editing ? (
        <div className="stack">
          <label className="field-label" htmlFor="doc-editor">Content</label>
          <p className="muted" style={{ margin: 0, fontSize: '0.8125rem' }}>Use a blank line between paragraphs, “# ” for a heading and “- ” for a list item.</p>
          <textarea id="doc-editor" className="textarea" value={draft} onChange={(e) => setDraft(e.target.value)} autoFocus />
          <div className="actions">
            <Button onPress={() => void act(() => api(`/api/v1/versions/${v.id}`, { method: 'PATCH', headers: { 'if-match': `"${v.revision}"` }, body: { content: { blocks: textToBlocks(draft) } } }), 'Saved.')}>Save draft</Button>
            <Button variant="secondary" onPress={() => setEditing(false)}>Cancel</Button>
          </div>
        </div>
      ) : <DocBody blocks={blocks} />}

      {a.current_published_version_id && <ChangeRequests artifactId={a.id} people={directory} versions={data.versions} canSuggest={p('view')} canResolve={p('publish')} act={act} />}

      {data.versions.length > 1 && (
        <section aria-labelledby="hist" style={{ marginTop: '2.5rem' }}>
          <div className="section-head"><h2 id="hist">Version history</h2></div>
          <ul className="timeline">
            {data.versions.map((x, i) => {
              const prev = data.versions[i + 1];
              return <li key={x.id}><Tag color={STATE_TAG[x.lifecycle]?.color}>{STATE_TAG[x.lifecycle]?.label}</Tag><span><strong>v{x.version_no}</strong>{x.published_at ? ` · published ${shortDate(x.published_at)}` : ''}{x.change_reason ? ` · ${x.change_reason}` : ''}</span>{prev && <Link className="timeline-link" to={`/library/${a.id}/compare?from=${prev.version_no}&to=${x.version_no}`}>Compare with v{prev.version_no}</Link>}</li>;
            })}
          </ul>
        </section>
      )}

      <FormDialog isOpen={dialog === 'changes'} onOpenChange={(o) => !o && setDialog(null)} title="Request changes" description="The draft goes back to the author with your comment."
        footer={<><Button variant="secondary" onPress={() => setDialog(null)}>Cancel</Button><Button onPress={() => void act(() => api(`/api/v1/versions/${v.id}/reviews`, { method: 'POST', body: { decision: 'changes_requested', comment } }), 'Sent back to the author.')} isDisabled={!comment.trim()}>Send</Button></>}>
        <TextField label="What should change?" value={comment} onChange={setComment} autoFocus />
      </FormDialog>
      <FormDialog isOpen={dialog === 'publish'} onOpenChange={(o) => !o && setDialog(null)} title={`Publish v${v.version_no}`} description={v.version_no > 1 ? 'The current version will be superseded. Say what changed so readers know.' : 'Everyone who can see the library will be able to read it.'}
        footer={<><Button variant="secondary" onPress={() => setDialog(null)}>Cancel</Button><Button onPress={() => void publish()} isDisabled={v.version_no > 1 && !comment.trim()}>Publish now</Button></>}>
        <div className="stack">
          <TextField label={v.version_no > 1 ? 'What changed?' : 'Note (optional)'} value={comment} onChange={setComment} autoFocus />
          {v.version_no > 1 && preview && (
            <RadioCards label="Type of change" value={change} onChange={(x) => setChange(x as Change)} options={[
              { id: 'major', label: 'Major', meta: `v${preview.next_semver_major}`, description: preview.reack_required_if_major ? 'Changes what people must do. They are asked to read it again.' : 'Changes what people must do.' },
              { id: 'minor', label: 'Minor', meta: `v${preview.next_semver_minor}`, description: preview.reack_required_if_minor ? 'Wording and corrections. People are still asked to read it again.' : 'Wording and corrections. Nobody is asked to read it again.' },
            ]} />
          )}
          {preview && <p className="publish-preview" aria-live="polite"><BookOpenCheck aria-hidden="true" size="1rem" />{reackSentence(preview, change)}</p>}
        </div>
      </FormDialog>
      <StepUpDialog isOpen={stepUp !== null} onClose={() => setStepUp(null)} onDone={() => { const f = stepUp; setStepUp(null); void f?.(); }} />
      {toast && <div className="toast" role="status">{toast}</div>}
    </article>
  );
}

const STEPS = [['draft', 'Draft'], ['in_review', 'In review'], ['approved', 'Approved'], ['published', 'Published']] as const;

/**
 * Where this version is in its lifecycle and what happens next, in one sentence (docs/05 §6).
 * The stepper marks the current step with weight and finished steps with a check, not colour alone.
 */
function Lifecycle({ state, hasContent, approvals, required, canReview, canEdit, canPublish, publishedAt }: { state: string; hasContent: boolean; approvals: number; required: number; canReview: boolean; canEdit: boolean; canPublish: boolean; publishedAt: string | null }) {
  const at = STEPS.findIndex(([k]) => k === state);
  const next: Record<string, string> = {
    draft: !hasContent ? 'This draft is empty. Add the content, then submit it for review.' : canEdit ? 'Only editors can see this draft. Submit it for review when it reads well.' : 'This draft is still being written.',
    in_review: `Waiting for review: ${approvals} of ${required} ${required === 1 ? 'approval' : 'approvals'} so far.${canReview ? ' You can approve it or ask for changes.' : ''}`,
    approved: canPublish ? 'Approved. Publish it to make it the version everyone reads.' : 'Approved and waiting to be published.',
    published: `Published${publishedAt ? ` on ${shortDate(publishedAt)}` : ''}. This is the version everyone reads.`,
    deprecated: 'A newer version replaced this one. It stays here for the record.',
    retired: 'This document was retired. It stays here for the record.',
  };
  return (
    <div className="lifecycle">
      {at >= 0 && (
        <ol className="lifecycle-steps" aria-label="Lifecycle">
          {STEPS.map(([k, label], i) => (
            <li key={k} className={i < at ? 'done' : i === at ? 'current' : ''} aria-current={i === at ? 'step' : undefined}>
              <span className="lifecycle-mark" aria-hidden="true">{i < at || (i === at && state === 'published') ? <Check size="0.6875rem" strokeWidth={3} /> : null}</span>{label}
            </li>
          ))}
        </ol>
      )}
      <p className="lifecycle-next">{next[state] ?? ''}</p>
    </div>
  );
}
