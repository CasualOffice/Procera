import { Check, Megaphone, Pin } from 'lucide-react';
import { useState } from 'react';
import { api } from '../lib/api';
import { type Member, memberMap, useItems } from '../lib/data';
import type { Block } from '../lib/format';
import { timeAgo } from '../lib/format';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { DocBody } from '../ui/Doc';
import { Tag } from '../ui/Tag';

interface FeedItem { id: string; title: string; content: { blocks?: Block[] }; priority: string; pinned: boolean; require_ack: boolean; author_membership_id: string; published_at: string | null; read_at: string | null; acked_at: string | null }

/** Announcements feed (docs/05 §3.23): pinned first, read receipts, acknowledgements. */
export function Announcements() {
  const { can } = useSession();
  const [items, reload] = useItems<FeedItem>('/api/v1/announcements/feed');
  const [members] = useItems<Member>(can('member:view') ? '/api/v1/members' : null);
  const people = memberMap(members);
  const [open, setOpen] = useState<string | null>(null);
  const toggle = async (f: FeedItem) => {
    setOpen(open === f.id ? null : f.id);
    if (!f.read_at) { await api(`/api/v1/announcements/${f.id}/reads`, { method: 'POST' }).catch(() => undefined); reload(); }
  };
  return (
    <>
      <div className="page-head">
        <div><h1>Announcements</h1><p>News and notices for you, newest first. Some ask you to confirm you’ve read them.</p></div>
      </div>
      {items === null ? <div className="skeleton-list"><span /><span /><span /></div> : items.length === 0 ? (
        <div className="empty-soft"><Megaphone aria-hidden="true" size={20} /><p className="empty-title">No announcements</p><p className="muted">When something is posted for you, it shows up here.</p></div>
      ) : (
        <ul className="posts">
          {items.map((f) => {
            const a = people.get(f.author_membership_id);
            const isOpen = open === f.id;
            return (
              <li key={f.id} className={`post${f.read_at ? '' : ' post-unread'}`}>
                <div className="post-head">
                  {a ? <Avatar name={a.display_name} size={32} /> : <span className="task-icon tag-orange"><Megaphone aria-hidden="true" size={16} /></span>}
                  <div className="post-main">
                    <p className="post-meta">{a?.display_name ?? 'Announcement'} · {timeAgo(f.published_at)}{f.pinned && <> · <Pin aria-hidden="true" size={12} /> Pinned</>}</p>
                    <h2 className="post-title"><button type="button" aria-expanded={isOpen} onClick={() => void toggle(f)}>{!f.read_at && <span className="visually-hidden">Unread: </span>}{f.title}</button></h2>
                  </div>
                  {f.priority === 'critical' && <Tag color="red" dot>Critical</Tag>}
                  {f.require_ack && (f.acked_at ? <Tag color="green"><Check aria-hidden="true" size={12} /> Acknowledged</Tag> : <Tag color="orange">Needs acknowledgement</Tag>)}
                </div>
                {isOpen && (
                  <div className="post-body">
                    <DocBody blocks={f.content.blocks ?? []} />
                    {f.require_ack && !f.acked_at && <Button onPress={async () => { await api(`/api/v1/announcements/${f.id}/acks`, { method: 'POST' }); reload(); }}><Check aria-hidden="true" size={16} /> I’ve read this</Button>}
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
