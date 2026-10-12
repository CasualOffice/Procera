import { BadgeCheck, CalendarClock, Check, CircleSlash, GitPullRequestArrow, Hash, MessageSquarePlus, RefreshCw, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Form } from 'react-aria-components';
import { api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { TextField } from '../ui/components/TextField';
import { Empty } from '../ui/States';
import { Tag, type TagColor } from '../ui/Tag';

/** Library governance on an artifact (BPMS-044, docs/08 "Library"). */
export interface Governance {
  review_cadence_days: number | null; next_review_at: string | null; last_reviewed_at: string | null;
  review_status: 'none' | 'current' | 'due_soon' | 'overdue'; verified: boolean; verified_at: string | null; verification_expires_at: string | null;
}
/** Runs an action with the page's error, step-up and reload handling; resolves true on success. */
export type Act = (fn: () => Promise<unknown>, done: string) => Promise<boolean>;
interface VersionRef { id: string; version_no: number; lifecycle: string }

const REVIEW: Record<string, { label: string; color: TagColor }> = {
  current: { label: 'Up to date', color: 'green' }, due_soon: { label: 'Review due soon', color: 'orange' }, overdue: { label: 'Review overdue', color: 'red' }, none: { label: 'No review date', color: 'gray' },
};

/**
 * Property rows: when the document is next reviewed and whether it is verified, each with the
 * action that changes it, for the people allowed to (publishers recertify, approvers verify).
 */
export function GovernanceRows({ artifactId, g, canRecertify, canVerify, act }: { artifactId: string; g: Governance; canRecertify: boolean; canVerify: boolean; act: Act }) {
  const [dialog, setDialog] = useState<null | 'recertify' | 'cadence' | 'unverify'>(null);
  const [comment, setComment] = useState('');
  const [days, setDays] = useState('');
  useEffect(() => { if (dialog === 'cadence') setDays(g.review_cadence_days ? String(g.review_cadence_days) : ''); if (dialog) setComment(''); }, [dialog, g.review_cadence_days]);
  const r = REVIEW[g.review_status] ?? REVIEW.none!;
  return (
    <>
      <dt><CalendarClock aria-hidden="true" size="0.9375rem" /> Review</dt>
      <dd>
        <Tag color={r.color} dot={g.review_status !== 'none'}>{r.label}</Tag>
        {g.next_review_at && <span className="muted">{g.review_status === 'overdue' ? 'was due' : 'next'} {shortDate(g.next_review_at)}</span>}
        {g.review_cadence_days && <span className="muted">· every {g.review_cadence_days} days</span>}
        {canRecertify && <span className="prop-actions">
          <button type="button" className="prop-link" onClick={() => setDialog('recertify')}>Recertify</button>
          <button type="button" className="prop-link" onClick={() => setDialog('cadence')}>Change schedule</button>
        </span>}
      </dd>
      <dt><BadgeCheck aria-hidden="true" size="0.9375rem" /> Verified</dt>
      <dd>
        {g.verified ? <Tag color="green"><BadgeCheck aria-hidden="true" size="0.75rem" /> Verified{g.verification_expires_at ? ` until ${shortDate(g.verification_expires_at)}` : ''}</Tag> : <span className="muted">Not verified</span>}
        {canVerify && <span className="prop-actions">
          {g.verified
            ? <button type="button" className="prop-link" onClick={() => setDialog('unverify')}>Remove verification</button>
            : <button type="button" className="prop-link" onClick={() => void act(() => api(`/api/v1/artifacts/${artifactId}/verification`, { method: 'POST' }), 'Verified.')}>Verify</button>}
        </span>}
      </dd>

      <FormDialog isOpen={dialog === 'recertify'} onOpenChange={(o) => !o && setDialog(null)} title="Recertify this document" description="Confirm it is still accurate. The next review date moves forward by its schedule."
        footer={<><Button variant="secondary" onPress={() => setDialog(null)}>Cancel</Button><Button onPress={() => void act(() => api(`/api/v1/artifacts/${artifactId}/recertify`, { method: 'POST', body: comment.trim() ? { comment: comment.trim() } : {} }), 'Recertified.').then((ok) => ok && setDialog(null))}><RefreshCw aria-hidden="true" size="0.9375rem" /> Recertify</Button></>}>
        <TextField label="Note (optional)" description="For example what you checked. Kept in the audit log." value={comment} onChange={setComment} autoFocus />
      </FormDialog>
      <FormDialog isOpen={dialog === 'cadence'} onOpenChange={(o) => !o && setDialog(null)} title="Review schedule" description="How often this document must be confirmed as still accurate. Leave empty to use the workspace default."
        footer={<><Button variant="secondary" onPress={() => setDialog(null)}>Cancel</Button><Button onPress={() => void act(() => api(`/api/v1/artifacts/${artifactId}/review-cadence`, { method: 'PUT', body: { days: days.trim() ? Number(days) : null } }), 'Review schedule saved.').then((ok) => ok && setDialog(null))} isDisabled={!!days.trim() && !(Number(days) > 0)}>Save</Button></>}>
        <TextField label="Review every (days)" inputMode="numeric" value={days} onChange={setDays} autoFocus description="For example 365 for once a year." />
      </FormDialog>
      <FormDialog isOpen={dialog === 'unverify'} onOpenChange={(o) => !o && setDialog(null)} title="Remove verification?" description="Readers will no longer see this document marked as verified."
        footer={<><Button variant="secondary" onPress={() => setDialog(null)}>Keep it</Button><Button variant="danger" onPress={() => void act(() => api(`/api/v1/artifacts/${artifactId}/verification`, { method: 'DELETE' }), 'Verification removed.').then((ok) => ok && setDialog(null))}><CircleSlash aria-hidden="true" size="0.9375rem" /> Remove</Button></>}>
        <p className="muted" style={{ margin: 0 }}>You can verify it again at any time.</p>
      </FormDialog>
    </>
  );
}

/** For plain readers: the fingerprint and review date tucked away for the curious. */
export function ReaderDetails({ fingerprint, nextReview }: { fingerprint: string | null; nextReview: string | null }) {
  if (!fingerprint && !nextReview) return null;
  return (
    <details className="doc-details">
      <summary>Details</summary>
      <dl className="props">
        {fingerprint && <><dt><Hash aria-hidden="true" size="0.9375rem" /> Fingerprint</dt><dd><code className="mono" title="SHA-256 of the approved content">{fingerprint.slice(0, 16)}…</code></dd></>}
        {nextReview && <><dt><CalendarClock aria-hidden="true" size="0.9375rem" /> Next review</dt><dd>{shortDate(nextReview)}</dd></>}
      </dl>
    </details>
  );
}

/** A due or overdue review, prominent for the people who can recertify. */
export function ReviewCallout({ artifactId, g, act }: { artifactId: string; g: Governance; act: Act }) {
  if (g.review_status !== 'overdue' && g.review_status !== 'due_soon') return null;
  const overdue = g.review_status === 'overdue';
  return (
    <div className={`callout ${overdue ? 'callout-danger' : 'callout-warn'}`} role="region" aria-label="Review due">
      <CalendarClock aria-hidden="true" size="1.125rem" />
      <div><p><strong>{overdue ? `This review was due on ${shortDate(g.next_review_at)}.` : `Review due on ${shortDate(g.next_review_at)}.`}</strong></p><p className="muted" style={{ fontSize: '0.8125rem' }}>Confirm it is still accurate, or start a new version if it needs changes.</p></div>
      <div className="actions"><Button onPress={() => void act(() => api(`/api/v1/artifacts/${artifactId}/recertify`, { method: 'POST', body: {} }), 'Recertified.')}><RefreshCw aria-hidden="true" size="0.9375rem" /> Recertify now</Button></div>
    </div>
  );
}

interface ChangeRequest { id: string; title: string; body: string; state: string; requested_by: string; requested_at: string; resolved_by: string | null; resolved_at: string | null; resolution: string | null; version_id: string | null }
const CR_STATE: Record<string, { label: string; color: TagColor }> = { open: { label: 'Open', color: 'blue' }, accepted: { label: 'Accepted', color: 'purple' }, declined: { label: 'Declined', color: 'gray' }, implemented: { label: 'Implemented', color: 'green' } };

/**
 * Suggested changes (BPMS-044): anyone who can read may suggest one; publishers accept, decline
 * with a reason, or mark it implemented in a version.
 */
export function ChangeRequests({ artifactId, people, versions, canSuggest, canResolve, act }: { artifactId: string; people: Map<string, Member>; versions: VersionRef[]; canSuggest: boolean; canResolve: boolean; act: Act }) {
  const [items, reload] = useItems<ChangeRequest>(`/api/v1/artifacts/${artifactId}/change-requests`);
  const [suggesting, setSuggesting] = useState(false);
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [resolving, setResolving] = useState<null | { cr: ChangeRequest; decision: 'declined' | 'implemented' }>(null);
  const [reason, setReason] = useState('');
  const [version, setVersion] = useState('');
  const run = async (fn: () => Promise<unknown>, done: string) => { const ok = await act(fn, done); reload(); return ok; };
  useEffect(() => { if (resolving) { setReason(''); setVersion(versions[0]?.id ?? ''); } }, [resolving, versions]);
  const open = (items ?? []).filter((c) => c.state === 'open' || c.state === 'accepted').length;

  return (
    <section aria-labelledby="cr-h" className="cr">
      <div className="section-head">
        <h2 id="cr-h">Suggested changes</h2>
        {open > 0 && <span className="count-pill">{open}</span>}
        {canSuggest && <Button variant="secondary" className="btn-sm section-action" onPress={() => { setTitle(''); setBody(''); setSuggesting(true); }}><MessageSquarePlus aria-hidden="true" size="0.875rem" /> Suggest a change</Button>}
      </div>
      {items === null ? null : items.length === 0 ? (
        <Empty compact icon={GitPullRequestArrow} title="No suggestions yet">Spotted something out of date or unclear? Suggest a change and the owner will see it.</Empty>
      ) : (
        <ul className="cr-list">
          {items.map((c) => {
            const who = people.get(c.requested_by);
            const st = CR_STATE[c.state] ?? CR_STATE.open!;
            const impl = c.version_id ? versions.find((v) => v.id === c.version_id) : undefined;
            return (
              <li key={c.id} className="cr-item">
                <div className="cr-main">
                  <p className="cr-title">{c.title} <Tag color={st.color}>{st.label}</Tag></p>
                  {c.body && <p className="cr-body">{c.body}</p>}
                  <p className="cr-meta">
                    {who && <span className="person"><Avatar name={who.display_name} size="1rem" />{who.display_name}</span>}
                    <span>{timeAgo(c.requested_at)}</span>
                    {c.resolution && <span>· {c.state === 'declined' ? 'Reason' : 'Note'}: {c.resolution}</span>}
                    {impl && <span>· in v{impl.version_no}</span>}
                  </p>
                </div>
                {canResolve && (c.state === 'open' || c.state === 'accepted') && (
                  <div className="cr-actions">
                    {c.state === 'open' && <Button variant="secondary" className="btn-sm" onPress={() => void run(() => api(`/api/v1/change-requests/${c.id}/resolve`, { method: 'POST', body: { decision: 'accepted' } }), 'Accepted.')}><Check aria-hidden="true" size="0.875rem" /> Accept</Button>}
                    <Button variant="secondary" className="btn-sm" onPress={() => setResolving({ cr: c, decision: 'implemented' })}>Mark implemented</Button>
                    {c.state === 'open' && <Button variant="ghost" className="btn-sm" onPress={() => setResolving({ cr: c, decision: 'declined' })}><X aria-hidden="true" size="0.875rem" /> Decline</Button>}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}

      <FormDialog isOpen={suggesting} onOpenChange={setSuggesting} title="Suggest a change" description="The document owner sees your suggestion and can accept it, decline it, or make the change in a new version."
        footer={<><Button variant="secondary" onPress={() => setSuggesting(false)}>Cancel</Button><Button isDisabled={!title.trim()} onPress={() => void run(() => api(`/api/v1/artifacts/${artifactId}/change-requests`, { method: 'POST', body: { title: title.trim(), body: body.trim() } }), 'Suggestion sent.').then((ok) => ok && setSuggesting(false))}>Send suggestion</Button></>}>
        <Form className="stack" onSubmit={(e) => e.preventDefault()}>
          <TextField label="What should change?" value={title} onChange={setTitle} isRequired autoFocus />
          <div className="field">
            <label className="field-label" htmlFor="cr-body">Details <span className="muted">(optional)</span></label>
            <textarea id="cr-body" className="textarea textarea-xs" value={body} onChange={(e) => setBody(e.target.value)} />
          </div>
        </Form>
      </FormDialog>
      <FormDialog isOpen={resolving !== null} onOpenChange={(o) => !o && setResolving(null)}
        title={resolving?.decision === 'declined' ? 'Decline this suggestion' : 'Mark as implemented'}
        description={resolving?.decision === 'declined' ? 'Say why, so the person who suggested it understands.' : 'Pick the version that includes this change.'}
        footer={<><Button variant="secondary" onPress={() => setResolving(null)}>Cancel</Button>
          <Button isDisabled={resolving?.decision === 'declined' ? !reason.trim() : !version}
            onPress={() => { const r = resolving!; void run(() => api(`/api/v1/change-requests/${r.cr.id}/resolve`, { method: 'POST', body: { decision: r.decision, ...(reason.trim() ? { resolution: reason.trim() } : {}), ...(r.decision === 'implemented' ? { version_id: version } : {}) } }), r.decision === 'declined' ? 'Suggestion declined.' : 'Marked as implemented.').then((ok) => ok && setResolving(null)); }}>
            {resolving?.decision === 'declined' ? 'Decline' : 'Mark implemented'}</Button></>}>
        {resolving?.decision === 'implemented' && (
          <div className="field">
            <label className="field-label" htmlFor="cr-version">Version</label>
            <select id="cr-version" className="field-input" value={version} onChange={(e) => setVersion(e.target.value)}>
              {versions.map((v) => <option key={v.id} value={v.id}>v{v.version_no} ({v.lifecycle.replace('_', ' ')})</option>)}
            </select>
          </div>
        )}
        <TextField label={resolving?.decision === 'declined' ? 'Reason' : 'Note (optional)'} value={reason} onChange={setReason} isRequired={resolving?.decision === 'declined'} />
      </FormDialog>
    </section>
  );
}
