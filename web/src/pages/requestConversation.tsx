import { Bell, BellOff, Lock, MessageSquare, X } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type Member, useItems, useResource } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { fieldErrors } from '../lib/requests';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { Tag } from '../ui/Tag';

/*
 * The conversation on a request (BPMS-214, FR-R02): comments from everyone who can see it, internal
 * notes the requester never sees, @mentions that notify, and watching to follow along.
 */

interface Person { id: string; name: string }
interface Comment { id: string; author: Person; body: string; internal: boolean; mentions: Person[]; created_at: string }
interface Conversation { comments: Comment[]; watching: boolean; watchers: Person[]; can_comment: boolean; can_post_internal: boolean }

/** Splits a body so the mentioned names (as @Name) are highlighted. */
function Body({ text, mentions }: { text: string; mentions: Person[] }) {
  const names = mentions.map((m) => m.name).sort((a, b) => b.length - a.length);
  if (!names.length) return <>{text}</>;
  const re = new RegExp(`(@(?:${names.map((n) => n.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|')}))`, 'g');
  return <>{text.split(re).map((part, i) => (part.startsWith('@') && names.includes(part.slice(1)) ? <span key={i} className="mention">{part}</span> : part))}</>;
}

export function RequestConversation({ requestId, requester }: { requestId: string; requester: Person }) {
  const { membershipId, can } = useSession();
  const [data, reload, error] = useResource<Conversation>(`/api/v1/requests/${requestId}/comments`);
  const [members] = useItems<Member>(can('member:view') ? '/api/v1/members' : null);
  const [body, setBody] = useState('');
  const [internal, setInternal] = useState(false);
  const [mentions, setMentions] = useState<Person[]>([]);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);

  // A notification links to #comment-<id>: bring that comment into view once it's loaded.
  useEffect(() => {
    if (!data || !window.location.hash.startsWith('#comment-')) return;
    document.getElementById(window.location.hash.slice(1))?.scrollIntoView({ block: 'center' });
  }, [data]);

  const options = useMemo(() => (members ?? [])
    .filter((m) => m.status === 'active' && m.id !== membershipId && !mentions.some((x) => x.id === m.id) && !(internal && m.id === requester.id))
    .map((m) => ({ id: m.id, label: m.display_name })), [members, membershipId, mentions, internal, requester.id]);

  const mention = (id: string) => {
    const m = (members ?? []).find((x) => x.id === id);
    if (!m) return;
    setMentions((xs) => [...xs, { id: m.id, name: m.display_name }]);
    setBody((b) => `${b}${b && !b.endsWith(' ') ? ' ' : ''}@${m.display_name} `);
    setErrors((e) => ({ ...e, mentions: '' }));
    area.current?.focus();
  };
  const unmention = (p: Person) => {
    setMentions((xs) => xs.filter((x) => x.id !== p.id));
    setBody((b) => b.replace(`@${p.name} `, '').replace(`@${p.name}`, ''));
  };

  const send = async () => {
    if (!body.trim()) { setErrors({ body: 'Write something first.' }); return; }
    setBusy(true); setErrors({}); setFailure(null);
    try {
      await api(`/api/v1/requests/${requestId}/comments`, { method: 'POST', body: { body: body.trim(), internal, mentions: mentions.map((m) => m.id) } });
      setBody(''); setMentions([]); setInternal(false); reload();
    } catch (e) {
      const f = fieldErrors(e);
      if (Object.keys(f).length) setErrors(f);
      else setFailure(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Couldn’t post that. Try again.');
    } finally { setBusy(false); }
  };
  const toggleWatch = async () => {
    if (!data) return;
    await api(`/api/v1/requests/${requestId}/watch`, { method: data.watching ? 'DELETE' : 'PUT' }).catch(() => undefined);
    reload();
  };

  if (error) return null;
  const watchers = data?.watchers ?? [];
  return (
    <section aria-labelledby="req-conversation" className="req-section req-conversation">
      <div className="section-head">
        <h2 id="req-conversation">Conversation{data && data.comments.length > 0 && <span className="muted conv-count"> · {data.comments.length}</span>}</h2>
        {data && (
          <span className="conv-watch">
            {watchers.length > 0 && <span className="muted" title={watchers.map((w) => w.name).join(', ')}>{watchers.length} watching</span>}
            <Button variant="secondary" className="btn-sm" aria-pressed={data.watching} onPress={() => void toggleWatch()}>
              {data.watching ? <><BellOff aria-hidden="true" size="0.875rem" /> Stop watching</> : <><Bell aria-hidden="true" size="0.875rem" /> Watch</>}
            </Button>
          </span>
        )}
      </div>
      {!data ? <p className="muted">Loading the conversation…</p> : (
        <>
          {data.comments.length === 0 ? (
            <p className="conv-empty"><MessageSquare aria-hidden="true" size="1rem" /> No comments yet. Ask a question or add context; the people on this request are told.</p>
          ) : (
            <ol className="conv-list" aria-label="Comments">
              {data.comments.map((c) => (
                <li key={c.id} id={`comment-${c.id}`} className={c.internal ? 'conv-item is-internal' : 'conv-item'}>
                  <Avatar name={c.author.name} size="1.75rem" />
                  <div className="conv-bubble">
                    <p className="conv-meta">
                      <strong>{c.author.name}</strong>
                      {c.author.id === requester.id && <span className="muted"> · requester</span>}
                      <time dateTime={c.created_at} title={shortDate(c.created_at)}> · {timeAgo(c.created_at)}</time>
                      {c.internal && <Tag color="yellow"><Lock aria-hidden="true" size="0.6875rem" /> Internal</Tag>}
                    </p>
                    <p className="conv-body"><Body text={c.body} mentions={c.mentions} /></p>
                  </div>
                </li>
              ))}
            </ol>
          )}
          {data.can_comment && (
            <form className={internal ? 'conv-composer is-internal' : 'conv-composer'} aria-label="Add a comment" onSubmit={(e) => { e.preventDefault(); void send(); }}>
              {failure && <Notice tone="danger" title={failure} />}
              <label className="field-label" htmlFor="conv-body">{internal ? 'Internal note' : 'Comment'}</label>
              <textarea id="conv-body" ref={area} rows={3} value={body} maxLength={4000}
                placeholder={internal ? `Only people working on it see this, not ${requester.name}.` : 'Write a comment… Use Mention to notify someone.'}
                onChange={(e) => { setBody(e.target.value); setErrors((x) => ({ ...x, body: '' })); }}
                onKeyDown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); void send(); } }}
                aria-invalid={!!errors.body} aria-describedby={errors.body ? 'conv-body-e' : 'conv-body-h'} />
              {errors.body ? <span id="conv-body-e" className="field-error">{errors.body}</span> : <span id="conv-body-h" className="field-description">⌘/Ctrl + Enter to send.</span>}
              {mentions.length > 0 && (
                <ul className="conv-mentions" aria-label="Mentioned">
                  {mentions.map((m) => (
                    <li key={m.id}><span>@{m.name}</span><button type="button" aria-label={`Remove mention of ${m.name}`} onClick={() => unmention(m)}><X aria-hidden="true" size="0.75rem" /></button></li>
                  ))}
                </ul>
              )}
              {errors.mentions && <span className="field-error" role="alert">{errors.mentions}</span>}
              <div className="conv-actions">
                {members && <div className="conv-mention"><Select label="Mention" selectedKey={null} placeholder="Mention someone" options={options} onSelectionChange={(k) => k && mention(String(k))} /></div>}
                {data.can_post_internal && (
                  <label className="check-row conv-internal"><input type="checkbox" checked={internal} onChange={(e) => { setInternal(e.target.checked); if (e.target.checked) setMentions((xs) => xs.filter((x) => x.id !== requester.id)); }} /> Internal note, hidden from {requester.name}</label>
                )}
                <Button type="submit" isDisabled={busy}>{busy ? 'Posting…' : internal ? 'Add note' : 'Comment'}</Button>
              </div>
            </form>
          )}
        </>
      )}
    </section>
  );
}
