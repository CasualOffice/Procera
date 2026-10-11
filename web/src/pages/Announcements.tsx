import { Check, ChevronDown, Megaphone, Pin, SquarePen } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { type Member, memberMap, useItems } from '../lib/data';
import type { Block } from '../lib/format';
import { timeAgo } from '../lib/format';
import { Link, useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { DocBody } from '../ui/Doc';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';

interface FeedItem { id: string; title: string; content: { blocks?: Block[] }; priority: string; pinned: boolean; require_ack: boolean; author_membership_id: string; published_at: string | null; read_at: string | null; acked_at: string | null }

/** First readable sentence(s) of an announcement, for the collapsed preview. */
export function excerpt(blocks: Block[] = [], max = 180): string {
  const text = blocks.map((b) => (b.type === 'list' ? (b.items ?? []).join(', ') : b.text ?? '')).filter(Boolean).join(' ');
  return text.length > max ? `${text.slice(0, max).replace(/\s+\S*$/, '')}…` : text;
}

/** Announcements feed (docs/05 §3.23): reading layout, pinned first, read receipts, acknowledgements. */
export function Announcements() {
  const { can } = useSession();
  const { query } = useRoute();
  const [items, reload, error] = useItems<FeedItem>('/api/v1/announcements/feed');
  const [members] = useItems<Member>(can('member:view') ? '/api/v1/members' : null);
  const people = memberMap(members);
  const [open, setOpen] = useState<string | null>(query.get('open'));
  const markRead = (f: FeedItem) => { if (!f.read_at) void api(`/api/v1/announcements/${f.id}/reads`, { method: 'POST' }).then(reload).catch(() => undefined); };
  useEffect(() => {
    const id = query.get('open');
    const f = id ? items?.find((x) => x.id === id) : undefined;
    if (f) { setOpen(f.id); markRead(f); document.getElementById(`post-${f.id}`)?.scrollIntoView({ block: 'center' }); }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items === null, query.get('open')]);
  const toggle = (f: FeedItem) => { setOpen(open === f.id ? null : f.id); markRead(f); };
  const canCompose = can('announcement:create') || can('announcement:publish') || can('announcement:manage');

  return (
    <>
      <div className="page-head">
        <div><h1>Announcements</h1><p>News and notices for you, newest first. Some ask you to confirm you’ve read them.</p></div>
        {false && canCompose && <Link className="btn btn-secondary btn-md link-btn" to="/admin/announcements"><SquarePen aria-hidden="true" size={15} /> Manage announcements</Link>}
      </div>
      {error ? <LoadError error={error} onRetry={reload} what="announcements" /> : items === null ? <Skeleton rows={3} label="Loading announcements" /> : items.length === 0 ? (
        <Empty icon={Megaphone} title="No announcements">When something is posted for you, it shows up here.</Empty>
      ) : (
        <ul className="posts">
          {items.map((f) => {
            const a = people.get(f.author_membership_id);
            const isOpen = open === f.id;
            const preview = excerpt(f.content.blocks);
            return (
              <li key={f.id} id={`post-${f.id}`} className={`post${f.read_at ? '' : ' post-unread'}${isOpen ? ' post-open' : ''}`}>
                <div className="post-head">
                  {a ? <Avatar name={a.display_name} size={32} /> : <span className="task-icon tag-orange"><Megaphone aria-hidden="true" size={16} /></span>}
                  <div className="post-main">
                    <p className="post-meta">
                      <span>{a?.display_name ?? 'Announcement'}</span><span aria-hidden="true">·</span><span>{timeAgo(f.published_at)}</span>
                      {f.pinned && <span className="post-pin"><Pin aria-hidden="true" size={12} /> Pinned</span>}
                    </p>
                    <h2 className="post-title">
                      <button type="button" aria-expanded={isOpen} onClick={() => toggle(f)}>
                        {!f.read_at && <span className="visually-hidden">Unread: </span>}{f.title}
                        <ChevronDown aria-hidden="true" size={16} className="post-chev" />
                      </button>
                    </h2>
                  </div>
                  <span className="post-tags">
                    {f.priority === 'critical' && <Tag color="red" dot>Critical</Tag>}
                    {f.require_ack && (f.acked_at ? <Tag color="green"><Check aria-hidden="true" size={12} /> Acknowledged</Tag> : <Tag color="orange">Needs acknowledgement</Tag>)}
                  </span>
                </div>
                {!isOpen && preview && <p className="post-preview" aria-hidden="true">{preview}</p>}
                {isOpen && (
                  <div className="post-body">
                    <DocBody blocks={f.content.blocks ?? []} />
                    {f.require_ack && !f.acked_at && (
                      <div className="post-ack">
                        <p>The sender asked everyone to confirm they’ve read this.</p>
                        <Button onPress={async () => { await api(`/api/v1/announcements/${f.id}/acks`, { method: 'POST' }); reload(); }}><Check aria-hidden="true" size={16} /> I’ve read this</Button>
                      </div>
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </>
  );
}
