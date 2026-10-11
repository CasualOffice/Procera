import { BookOpenCheck, Check, CircleDot, FileText, GitBranch, Hash, Pencil, ScrollText, Send, ShieldCheck, User } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type Member, memberMap, useItems } from '../lib/data';
import { type Block, blocksToText, shortDate, textToBlocks, timeAgo } from '../lib/format';
import { Link } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { DocBody } from '../ui/Doc';
import { StepUpDialog } from '../ui/StepUp';
import { KIND_TAG, STATE_TAG, Tag } from '../ui/Tag';

interface ArtifactResp { artifact: { id: string; kind: string; key: string; title: string; state: string; criticality: string; owner_membership_id: string | null; current_published_version_id: string | null }; versions: { id: string; version_no: number; lifecycle: string; published_at: string | null; change_reason: string | null }[] }
interface Version { id: string; version_no: number; lifecycle: string; content: { blocks?: Block[] }; content_hash: string | null; created_by: string; created_at: string; published_at: string | null; revision: number; reviews: { reviewer_membership_id: string; decision: string; comment: string | null; decided_at: string }[]; approvals_required: number | null; change_reason: string | null }
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
  const people = memberMap(members);

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

  /** Runs an action; on `step_up_required` asks to verify and retries once. */
  const act = async (fn: () => Promise<unknown>, done: string) => {
    setError(null);
    const run = async () => { await fn(); setToast(done); setEditing(false); setDialog(null); await load(); reloadAcks(); };
    try { await run(); } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(() => async () => { try { await run(); } catch (err) { setError(err instanceof Error ? err.message : 'Failed.'); } });
      else setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Something went wrong.');
    }
  };
  useEffect(() => { if (!toast) return; const t = setTimeout(() => setToast(null), 3500); return () => clearTimeout(t); }, [toast]);

  if (notFound) return <div className="page-head"><div><h1>Document not found</h1><p>It doesn’t exist or you don’t have access to it.</p></div></div>;
  if (!data || !version) return <div className="doc"><div className="skeleton-list"><span /><span /><span /></div></div>;

  const a = data.artifact;
  const kind = KIND_TAG[a.kind];
  const owner = a.owner_membership_id ? people.get(a.owner_membership_id) : undefined;
  const author = people.get(version.created_by);
  const blocks = version.content.blocks ?? [];
  const pendingAck = (acks ?? []).find((x) => x.subject.artifact_id === a.id);
  const v = version;
  const p = (action: string) => can(`${a.kind}:${action}`);
  const approvals = v.reviews.filter((r) => r.decision === 'approve').length;

  return (
    <article className="doc">
      <div className="doc-kicker">
        <span className={`doc-icon tag-${kind?.color ?? 'gray'}`} style={{ width: 20, height: 20 }}>{a.kind === 'sop' ? <ScrollText aria-hidden="true" size={12} /> : <FileText aria-hidden="true" size={12} />}</span>
        <span>{kind?.label}</span>
      </div>
      <h1 className="doc-title">{a.title}</h1>

      <dl className="props">
        <dt><CircleDot aria-hidden="true" size={15} /> Status</dt><dd><Tag color={STATE_TAG[v.lifecycle]?.color} dot>{STATE_TAG[v.lifecycle]?.label}</Tag>{a.criticality === 'controlled' && <Tag color="red">Controlled</Tag>}</dd>
        <dt><GitBranch aria-hidden="true" size={15} /> Version</dt><dd>v{v.version_no}{v.published_at && <span className="muted">· published {shortDate(v.published_at)}</span>}{v.change_reason && <span className="muted">· {v.change_reason}</span>}</dd>
        <dt><User aria-hidden="true" size={15} /> Owner</dt><dd>{owner ? <span className="person"><Avatar name={owner.display_name} size={20} />{owner.display_name}</span> : '—'}</dd>
        {v.lifecycle === 'in_review' && <><dt><ShieldCheck aria-hidden="true" size={15} /> Approvals</dt><dd>{approvals} of {v.approvals_required ?? 1}{v.reviews.map((r) => <span key={r.reviewer_membership_id} className="person" title={r.decision}><Avatar name={people.get(r.reviewer_membership_id)?.display_name ?? '?'} size={20} /></span>)}</dd></>}
        {v.content_hash && <><dt><Hash aria-hidden="true" size={15} /> Fingerprint</dt><dd><code className="mono" title="SHA-256 of the approved content">{v.content_hash.slice(0, 16)}…</code></dd></>}
      </dl>

      {error && <Notice tone="danger" title={error} />}
      {pendingAck && pendingAck.subject.version_id === v.id && (
        <div className="callout callout-info" role="region" aria-label="Acknowledgement requested">
          <BookOpenCheck aria-hidden="true" size={18} />
          <div><p><strong>Please read and acknowledge this version by {shortDate(pendingAck.due_at)}.</strong></p><p className="muted" style={{ fontSize: 13 }}>{pendingAck.ack_text}</p></div>
          <div className="actions"><Button onPress={() => void act(() => api(`/api/v1/acknowledgements/${pendingAck.id}/complete`, { method: 'POST', body: { content_hash: pendingAck.subject.content_hash } }), 'Acknowledged. Thank you.')}><Check aria-hidden="true" size={16} /> I acknowledge</Button></div>
        </div>
      )}

      {/* Lifecycle actions for this version */}
      <div className="doc-actions">
        {v.lifecycle === 'draft' && p('edit_draft') && !editing && <Button variant="secondary" onPress={() => { setDraft(blocksToText(blocks)); setEditing(true); }}><Pencil aria-hidden="true" size={15} /> Edit</Button>}
        {v.lifecycle === 'draft' && p('edit_draft') && !editing && <Button onPress={() => void act(() => api(`/api/v1/versions/${v.id}/submit`, { method: 'POST' }), 'Submitted.')} isDisabled={!blocks.length}><Send aria-hidden="true" size={15} /> Submit for review</Button>}
        {v.lifecycle === 'in_review' && p('approve') && <Button onPress={() => void act(() => api(`/api/v1/versions/${v.id}/reviews`, { method: 'POST', body: { decision: 'approve' } }), 'Approved.')}><Check aria-hidden="true" size={15} /> Approve</Button>}
        {v.lifecycle === 'in_review' && (p('review') || p('approve')) && <Button variant="secondary" onPress={() => { setComment(''); setDialog('changes'); }}>Request changes</Button>}
        {v.lifecycle === 'in_review' && p('edit_draft') && <Button variant="ghost" onPress={() => void act(() => api(`/api/v1/versions/${v.id}/withdraw`, { method: 'POST' }), 'Withdrawn to draft.')}>Withdraw</Button>}
        {v.lifecycle === 'approved' && p('publish') && <Button onPress={() => { setComment(''); setDialog('publish'); }}><Send aria-hidden="true" size={15} /> Publish</Button>}
        {['published', 'deprecated'].includes(v.lifecycle) && p('edit_draft') && <Button variant="secondary" onPress={() => void act(() => api(`/api/v1/artifacts/${a.id}/versions`, { method: 'POST' }), 'New draft created.')}><Pencil aria-hidden="true" size={15} /> New version</Button>}
        <span className="muted doc-meta">{author ? `Edited by ${author.display_name} · ` : ''}{timeAgo(v.created_at)} · {workspace?.name}</span>
      </div>
      <hr className="doc-rule" />

      {editing ? (
        <div className="stack">
          <label className="field-label" htmlFor="doc-editor">Content</label>
          <p className="muted" style={{ margin: 0, fontSize: 13 }}>Use a blank line between paragraphs, “# ” for a heading and “- ” for a list item.</p>
          <textarea id="doc-editor" className="textarea" value={draft} onChange={(e) => setDraft(e.target.value)} autoFocus />
          <div className="actions">
            <Button onPress={() => void act(() => api(`/api/v1/versions/${v.id}`, { method: 'PATCH', headers: { 'if-match': `"${v.revision}"` }, body: { content: { blocks: textToBlocks(draft) } } }), 'Saved.')}>Save draft</Button>
            <Button variant="secondary" onPress={() => setEditing(false)}>Cancel</Button>
          </div>
        </div>
      ) : <DocBody blocks={blocks} />}

      {data.versions.length > 1 && (
        <section aria-labelledby="hist" style={{ marginTop: 40 }}>
          <div className="section-head"><h2 id="hist">Version history</h2></div>
          <ul className="timeline">
            {data.versions.map((x) => <li key={x.id}><Tag color={STATE_TAG[x.lifecycle]?.color}>{STATE_TAG[x.lifecycle]?.label}</Tag><span><strong>v{x.version_no}</strong>{x.published_at ? ` · published ${shortDate(x.published_at)}` : ''}{x.change_reason ? ` · ${x.change_reason}` : ''}</span></li>)}
          </ul>
        </section>
      )}

      <FormDialog isOpen={dialog === 'changes'} onOpenChange={(o) => !o && setDialog(null)} title="Request changes" description="The draft goes back to the author with your comment."
        footer={<><Button variant="secondary" onPress={() => setDialog(null)}>Cancel</Button><Button onPress={() => void act(() => api(`/api/v1/versions/${v.id}/reviews`, { method: 'POST', body: { decision: 'changes_requested', comment } }), 'Sent back to the author.')} isDisabled={!comment.trim()}>Send</Button></>}>
        <TextField label="What should change?" value={comment} onChange={setComment} autoFocus />
      </FormDialog>
      <FormDialog isOpen={dialog === 'publish'} onOpenChange={(o) => !o && setDialog(null)} title={`Publish v${v.version_no}`} description={v.version_no > 1 ? 'The current version will be superseded. Say what changed so readers know.' : 'Everyone who can see the library will be able to read it.'}
        footer={<><Button variant="secondary" onPress={() => setDialog(null)}>Cancel</Button><Button onPress={() => void act(() => api(`/api/v1/versions/${v.id}/publish`, { method: 'POST', body: comment.trim() ? { change_reason: comment } : {} }), 'Published.')} isDisabled={v.version_no > 1 && !comment.trim()}>Publish now</Button></>}>
        <TextField label={v.version_no > 1 ? 'What changed?' : 'Note (optional)'} value={comment} onChange={setComment} autoFocus />
      </FormDialog>
      <StepUpDialog isOpen={stepUp !== null} onClose={() => setStepUp(null)} onDone={() => { const f = stepUp; setStepUp(null); void f?.(); }} />
      {toast && <div className="toast" role="status">{toast}</div>}
    </article>
  );
}
