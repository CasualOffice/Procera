import { Archive, Check, Megaphone, Pin, Plus, Send, SquarePen, X } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { type Member, memberMap, useItems, useResource } from '../lib/data';
import { type Block, blocksToText, shortDate, textToBlocks, timeAgo } from '../lib/format';
import { Link, navigate, useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import { type Audience, AudiencePicker, EVERYONE, audienceOf, audienceSummary, isEmptyAudience, useDirectory } from '../ui/Audience';
import { Avatar } from '../ui/Avatar';
import { AnnouncementTabs } from './areaTabs';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { DocBody } from '../ui/Doc';
import { Drawer } from '../ui/Drawer';
import { Bars } from '../ui/Bars';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag, type TagColor } from '../ui/Tag';

export interface Announcement {
  /** Publishing needs someone else's approval (announcements.approval_required). */
  needs_approval?: boolean;
  id: string; title: string; content: { blocks?: Block[] }; audience: unknown; priority: string; pinned: boolean; require_ack: boolean;
  publish_at: string | null; expires_at: string; ack_by?: string | null; campaign_id?: string | null; state: string; expired: boolean; approval_required: boolean; channels: string[];
  author_membership_id: string; submitted_at: string | null; decided_by: string | null; decided_at: string | null; decision_comment: string | null;
  published_at: string | null; recipient_count: number | null; archived_at: string | null; archive_reason: string | null; created_at: string; revision: number;
}
interface Analytics { recipients: number; read: number; acknowledged: number; pending_ack: number; require_ack: boolean; by_department: { department: string | null; recipients: number; read: number; acknowledged: number }[] }

const STATE: Record<string, { label: string; color: TagColor }> = {
  draft: { label: 'Draft', color: 'gray' },
  pending_approval: { label: 'Waiting for approval', color: 'yellow' },
  scheduled: { label: 'Scheduled', color: 'orange' },
  published: { label: 'Published', color: 'green' },
  archived: { label: 'Archived', color: 'brown' },
};
const TABS = [['all', 'All'], ['draft', 'Drafts'], ['pending_approval', 'Waiting for approval'], ['scheduled', 'Scheduled'], ['published', 'Published'], ['archived', 'Archived']] as const;

const stateTag = (a: Announcement) => a.state === 'published' && a.expired ? <Tag color="gray" dot>Expired</Tag> : <Tag color={STATE[a.state]?.color} dot>{STATE[a.state]?.label ?? a.state}</Tag>;
const when = (a: Announcement) => a.archived_at ?? a.published_at ?? a.publish_at ?? a.submitted_at ?? a.created_at;
const CHANNEL: Record<string, string> = { in_app: 'in the app', email: 'by email', push: 'by push notification', webhook: 'by webhook', chat: 'in chat' };
const channelText = (cs: string[]) => { const t = cs.map((c) => CHANNEL[c] ?? c).join(' and '); return t.charAt(0).toUpperCase() + t.slice(1); };
const pct = (n: number, d: number) => (d ? Math.round((n / d) * 100) : 0);

/** Admin · Announcements (docs/05 §3.23): compose, approve, publish, archive and read receipts. */
export function ManageAnnouncements() {
  const { can } = useSession();
  const { query } = useRoute();
  const [items, reload, error] = useItems<Announcement>('/api/v1/announcements');
  const [members] = useItems<Member>(can('member:view') ? '/api/v1/members' : null);
  const people = memberMap(members);
  const dir = useDirectory();
  const [tab, setTab] = useState<string>(query.get('status') ?? 'all');
  const [composing, setComposing] = useState<Announcement | 'new' | null>(query.get('new') ? 'new' : null);
  const [open, setOpen] = useState<string | null>(query.get('id'));
  useEffect(() => { if (query.get('new')) setComposing('new'); }, [query]);
  const canCreate = can('announcement:create') || can('announcement:manage');
  const rows = useMemo(() => (items ?? []).filter((a) => tab === 'all' || a.state === tab).sort((a, b) => Date.parse(when(b)) - Date.parse(when(a))), [items, tab]);
  const count = (k: string) => (k === 'all' ? items?.length ?? 0 : (items ?? []).filter((a) => a.state === k).length);
  const selected = items?.find((a) => a.id === open) ?? null;

  return (
    <>
      <div className="page-head">
        <div><h1>Manage announcements</h1><p>Write, approve and publish announcements, and see who has read them.</p></div>
        {canCreate && <Button onPress={() => setComposing('new')}><Plus aria-hidden="true" size="1rem" /> New announcement</Button>}
      </div>
      <AnnouncementTabs current="manage" />
      <div className="toolbar">
        <div className="segmented seg-scroll" role="group" aria-label="Filter by status">
          {TABS.filter(([k]) => k === 'all' || count(k) > 0 || k === 'draft').map(([k, label]) => (
            <button key={k} type="button" aria-pressed={tab === k} onClick={() => setTab(k)}>{label} <span className="seg-count">{count(k)}</span></button>
          ))}
        </div>
      </div>
      {error ? <LoadError error={error} onRetry={reload} what="announcements" /> : items === null ? <Skeleton variant="table" rows={4} label="Loading announcements" /> : rows.length === 0 ? (
        items.length ? <Empty compact title={`No ${TABS.find(([k]) => k === tab)?.[1].toLowerCase()} announcements`} action={<Button variant="secondary" onPress={() => setTab('all')}>Show all</Button>} /> : (
          <Empty icon={Megaphone} title="No announcements yet" action={canCreate ? <Button onPress={() => setComposing('new')}><Plus aria-hidden="true" size="1rem" /> New announcement</Button> : undefined}>
            Post news, notices and policy changes to everyone or to chosen departments, groups and roles. You can ask people to confirm they’ve read it.
          </Empty>
        )
      ) : (
        <div className="table-wrap">
          <table className="table" aria-label="Announcements">
            <thead><tr><th>Title</th><th>Status</th><th className="col-optional">Audience</th><th className="col-optional">Author</th><th className="col-optional">Reach</th><th className="col-optional">Updated</th></tr></thead>
            <tbody>
              {rows.map((a) => {
                const au = people.get(a.author_membership_id);
                return (
                  <tr key={a.id}>
                    <td>
                      <span className="cell-entity">
                        <span className={`doc-icon tag-${a.priority === 'critical' ? 'red' : 'orange'}`}><Megaphone aria-hidden="true" size="0.875rem" /></span>
                        <button type="button" className="cell-link" onClick={() => setOpen(a.id)}>{a.title}</button>
                        {a.pinned && <Pin aria-label="Pinned" size="0.8125rem" className="cell-glyph" />}
                        {a.priority === 'critical' && <Tag color="red">Critical</Tag>}
                      </span>
                    </td>
                    <td>{stateTag(a)}</td>
                    <td className="col-optional cell-muted">{audienceSummary(audienceOf(a.audience), dir)}</td>
                    <td className="col-optional">{au ? <span className="person"><Avatar name={au.display_name} size="1.25rem" />{au.display_name}</span> : null}</td>
                    <td className="col-optional cell-muted">{a.recipient_count != null ? `${a.recipient_count} ${a.recipient_count === 1 ? 'person' : 'people'}` : ''}</td>
                    <td className="col-optional cell-muted"><time dateTime={when(a)}>{timeAgo(when(a))}</time></td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {selected && (
        <AnnouncementDrawer a={selected} people={people} onClose={() => { setOpen(null); if (query.get('id')) navigate('/admin/announcements', { replace: true }); }}
          onEdit={() => setComposing(selected)} onChanged={reload} />
      )}
      <Composer value={composing} onClose={() => { setComposing(null); if (query.get('new')) navigate('/admin/announcements', { replace: true }); }} onSaved={(a) => { reload(); setOpen(a.id); }} />
    </>
  );
}

/** One announcement: what it says, who it goes to, what happens next, and receipts once live. */
function AnnouncementDrawer({ a, people, onClose, onEdit, onChanged }: { a: Announcement; people: Map<string, Member>; onClose: () => void; onEdit: () => void; onChanged: () => void }) {
  const { can } = useSession();
  const dir = useDirectory();
  const [error, setError] = useState<string | null>(null);
  const [needsApproval, setNeedsApproval] = useState(false);
  const [dialog, setDialog] = useState<null | 'reject' | 'archive'>(null);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const live = a.state === 'published' || a.state === 'archived';
  const author = people.get(a.author_membership_id);
  const manage = can('announcement:manage');
  const publisher = can('announcement:publish') || manage;
  const [stats] = useResource<Analytics>(live ? `/api/v1/announcements/${a.id}/analytics` : null, [a.revision]);
  useEffect(() => { setError(null); setNeedsApproval(a.needs_approval === true); }, [a.id, a.state, a.needs_approval]);

  const act = async (path: string, body?: unknown) => {
    setBusy(true); setError(null);
    try { await api(`/api/v1/announcements/${a.id}/${path}`, { method: 'POST', body }); setDialog(null); setText(''); onChanged(); }
    catch (e) {
      if (e instanceof ApiError && e.code === 'state_conflict' && /approval/i.test(e.problem.detail ?? '')) setNeedsApproval(true);
      else setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Something went wrong. Try again.');
    } finally { setBusy(false); }
  };

  const next: Record<string, string> = {
    draft: a.decision_comment ? 'Sent back with changes requested. Edit it, then send it again.' : 'Only you and other editors can see this draft.',
    pending_approval: 'Waiting for a publisher to approve it. The author can’t approve their own announcement.',
    scheduled: `Goes out on ${a.publish_at ? new Date(a.publish_at).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : 'its scheduled date'}.`,
    published: a.expired ? 'Expired: it no longer shows in people’s feeds.' : `Live in people’s feeds until ${shortDate(a.expires_at)}.`,
    archived: `Archived${a.archive_reason ? `: ${a.archive_reason}` : ''}. It stays here for the record.`,
  };

  const actions = (
    <>
      {a.state === 'draft' && <Button variant="secondary" onPress={onEdit}><SquarePen aria-hidden="true" size="0.9375rem" /> Edit</Button>}
      {a.state === 'draft' && publisher && !needsApproval && <Button onPress={() => void act('publish')} isDisabled={busy}><Send aria-hidden="true" size="0.9375rem" /> {a.publish_at && Date.parse(a.publish_at) > Date.now() ? 'Schedule' : 'Publish now'}</Button>}
      {a.state === 'draft' && (!publisher || needsApproval) && <Button onPress={() => void act('submit')} isDisabled={busy}><Send aria-hidden="true" size="0.9375rem" /> Send for approval</Button>}
      {a.state === 'pending_approval' && publisher && <Button variant="secondary" onPress={() => { setText(''); setDialog('reject'); }}><X aria-hidden="true" size="0.9375rem" /> Request changes</Button>}
      {a.state === 'pending_approval' && publisher && <Button onPress={() => void act('approve')} isDisabled={busy}><Check aria-hidden="true" size="0.9375rem" /> Approve and publish</Button>}
      {(a.state === 'published' || a.state === 'scheduled') && <Button variant="secondary" onPress={() => { setText(''); setDialog('archive'); }}><Archive aria-hidden="true" size="0.9375rem" /> Archive</Button>}
    </>
  );

  return (
    <Drawer title={a.title} onClose={onClose} footer={actions}>
      <div className="drawer-tags">{stateTag(a)}{a.priority === 'critical' && <Tag color="red">Critical</Tag>}{a.pinned && <Tag color="gray"><Pin aria-hidden="true" size="0.6875rem" /> Pinned</Tag>}</div>
      <p className="drawer-next">{next[a.state]}</p>
      {needsApproval && <Notice tone="info" title="This one needs approval">Announcements to this audience must be approved by someone else before they go out. Send it for approval instead.</Notice>}
      {a.state === 'draft' && a.decision_comment && <Notice tone="warning" title="Changes requested">{a.decision_comment}</Notice>}
      {error && <Notice tone="danger" title={error} />}
      <dl className="props props-tight">
        <dt>Audience</dt><dd>{audienceSummary(audienceOf(a.audience), dir)}{a.recipient_count != null && <span className="muted">· {a.recipient_count} {a.recipient_count === 1 ? 'person' : 'people'}</span>}</dd>
        <dt>Author</dt><dd>{author ? <span className="person"><Avatar name={author.display_name} size="1.25rem" />{author.display_name}</span> : null}</dd>
        <dt>Acknowledgement</dt><dd>{a.require_ack ? 'People are asked to confirm they’ve read it' : 'Not asked'}</dd>
        {a.publish_at && <><dt>Publish</dt><dd>{new Date(a.publish_at).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })}</dd></>}
        <dt>Expires</dt><dd>{shortDate(a.expires_at)}</dd>
        {a.require_ack && a.ack_by && <><dt>Confirm by</dt><dd>{shortDate(a.ack_by)}{a.campaign_id && <> · <Link to={`/admin/campaigns/${a.campaign_id}`}>Reminders and evidence</Link></>}</dd></>}
        {a.channels.length > 0 && <><dt>Sent by</dt><dd>{channelText(a.channels)}</dd></>}
      </dl>
      {live && stats && (
        <section aria-labelledby="receipts-h" className="drawer-section">
          <h3 id="receipts-h" className="drawer-h">Receipts</h3>
          <div className="stat-line">
            <span><strong>{pct(stats.read, stats.recipients)}%</strong> read <span className="muted">({stats.read} of {stats.recipients})</span></span>
            {stats.require_ack && <span><strong>{pct(stats.acknowledged, stats.recipients)}%</strong> acknowledged <span className="muted">({stats.pending_ack} still to do)</span></span>}
          </div>
          {stats.by_department.length > 1 && (
            <Bars caption={stats.require_ack ? 'Acknowledged by department' : 'Read by department'} rows={stats.by_department.map((d) => ({ label: d.department ?? 'No department', value: stats.require_ack ? d.acknowledged : d.read, total: d.recipients }))} unit={stats.require_ack ? 'acknowledged' : 'read'} />
          )}
        </section>
      )}
      <section aria-labelledby="msg-h" className="drawer-section">
        <h3 id="msg-h" className="drawer-h">Message</h3>
        <div className="drawer-doc"><DocBody blocks={a.content.blocks ?? []} /></div>
      </section>
      <FormDialog isOpen={dialog === 'reject'} onOpenChange={(o) => !o && setDialog(null)} title="Request changes" description="It goes back to the author as a draft, with your note."
        footer={<><Button variant="secondary" onPress={() => setDialog(null)}>Cancel</Button><Button onPress={() => void act('reject', { comment: text })} isDisabled={!text.trim() || busy}>Send back</Button></>}>
        <TextField label="What should change?" value={text} onChange={setText} autoFocus />
      </FormDialog>
      <FormDialog isOpen={dialog === 'archive'} onOpenChange={(o) => !o && setDialog(null)} title="Archive this announcement?" description="It disappears from people’s feeds. Receipts are kept for the record."
        footer={<><Button variant="secondary" onPress={() => setDialog(null)}>Cancel</Button><Button variant="danger" onPress={() => void act('archive', { reason: text })} isDisabled={!text.trim() || busy}>Archive</Button></>}>
        <TextField label="Reason" description="Recorded in the audit log." value={text} onChange={setText} autoFocus />
      </FormDialog>
    </Drawer>
  );
}

const toLocalInput = (iso: string | null) => (iso ? new Date(Date.parse(iso) - new Date().getTimezoneOffset() * 60_000).toISOString().slice(0, 16) : '');

/** Compose or edit a draft. Saving keeps it a draft; publishing happens from the drawer. */
function Composer({ value, onClose, onSaved }: { value: Announcement | 'new' | null; onClose: () => void; onSaved: (a: Announcement) => void }) {
  const editing = value && value !== 'new' ? value : null;
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [audience, setAudience] = useState<Audience>(EVERYONE);
  const [critical, setCritical] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [ack, setAck] = useState(false);
  const [later, setLater] = useState('');
  const [expires, setExpires] = useState('');
  const [ackBy, setAckBy] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!value) return;
    setError(null);
    setTitle(editing?.title ?? ''); setBody(editing ? blocksToText(editing.content.blocks) : '');
    setAudience(editing ? audienceOf(editing.audience) : EVERYONE); setCritical(editing?.priority === 'critical');
    setPinned(editing?.pinned ?? false); setAck(editing?.require_ack ?? false);
    setLater(toLocalInput(editing?.publish_at ?? null)); setExpires(editing ? editing.expires_at.slice(0, 10) : '');
    setAckBy(editing?.ack_by ? editing.ack_by.slice(0, 10) : '');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);
  const save = async () => {
    setBusy(true); setError(null);
    const payload = {
      title: title.trim(), content: { blocks: textToBlocks(body) }, audience, priority: critical ? 'critical' : 'normal', pinned, require_ack: ack,
      publish_at: later ? new Date(later).toISOString() : null, expires_at: expires ? new Date(`${expires}T23:59:00`).toISOString() : null,
      ...(ack && ackBy ? { ack_by: new Date(`${ackBy}T23:59:00`).toISOString() } : {}),
    };
    try {
      const a = editing
        ? await api<Announcement>(`/api/v1/announcements/${editing.id}`, { method: 'PATCH', headers: { 'if-match': `"${editing.revision}"` }, body: payload })
        : await api<Announcement>('/api/v1/announcements', { method: 'POST', body: payload });
      onSaved(a); onClose();
    } catch (e) {
      setError(e instanceof ApiError ? (e.status === 412 ? 'Someone else changed this draft. Close and open it again.' : e.problem.detail ?? e.problem.title) : 'Could not save. Try again.');
    } finally { setBusy(false); }
  };
  const ready = title.trim() && body.trim() && !isEmptyAudience(audience);
  return (
    <FormDialog isOpen={value !== null} onOpenChange={(o) => !o && onClose()} width={680} title={editing ? 'Edit announcement' : 'New announcement'} description="It’s saved as a draft. You publish it, or send it for approval, in the next step."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void save()} isDisabled={!ready || busy}>{busy ? 'Saving…' : 'Save draft'}</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <Form onSubmit={(e) => { e.preventDefault(); if (ready) void save(); }} className="stack compose">
        <TextField label="Title" value={title} onChange={setTitle} isRequired autoFocus />
        <div className="field">
          <label className="field-label" htmlFor="ann-body">Message</label>
          <textarea id="ann-body" className="textarea textarea-sm" value={body} onChange={(e) => setBody(e.target.value)} aria-describedby="ann-body-hint" />
          <span id="ann-body-hint" className="field-description">Leave a blank line between paragraphs. Start a line with “# ” for a heading or “- ” for a list item.</span>
        </div>
        <AudiencePicker value={audience} onChange={setAudience} />
        <div className="compose-options" role="group" aria-label="Options">
          <label className="check-row"><input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} /><span><strong>Ask people to confirm they’ve read it</strong><small>Each person gets an “I’ve read this” button and you see who has.</small></span></label>
          <label className="check-row"><input type="checkbox" checked={critical} onChange={(e) => setCritical(e.target.checked)} /><span><strong>Critical</strong><small>Highlighted in red and also sent by email.</small></span></label>
          <label className="check-row"><input type="checkbox" checked={pinned} onChange={(e) => setPinned(e.target.checked)} /><span><strong>Pin to the top of the feed</strong></span></label>
        </div>
        <div className="compose-dates">
          <div className="field"><label className="field-label" htmlFor="ann-at">Publish at <span className="muted">(optional)</span></label><input id="ann-at" type="datetime-local" className="field-input" value={later} onChange={(e) => setLater(e.target.value)} /><span className="field-description">Leave empty to send it as soon as it’s published.</span></div>
          {ack && <div className="field"><label className="field-label" htmlFor="ann-ackby">Confirm by <span className="muted">(optional)</span></label><input id="ann-ackby" type="date" className="field-input" value={ackBy} max={expires || undefined} onChange={(e) => setAckBy(e.target.value)} /><span className="field-description">People get reminders until then, and their manager is told once if they haven’t confirmed. Leave empty for the workspace default.</span></div>}
          <div className="field"><label className="field-label" htmlFor="ann-exp">Expires on <span className="muted">(optional)</span></label><input id="ann-exp" type="date" className="field-input" value={expires} onChange={(e) => setExpires(e.target.value)} /><span className="field-description">Leave empty for the workspace default.</span></div>
        </div>
      </Form>
    </FormDialog>
  );
}
