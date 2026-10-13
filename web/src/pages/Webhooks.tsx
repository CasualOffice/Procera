import { Check, Copy, Plus, RotateCcw, Send, Trash2, Webhook } from 'lucide-react';
import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { useItems } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Checkbox } from '../ui/components/Checkbox';
import { ConfirmDialog } from '../ui/components/Dialog';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { StepUpDialog } from '../ui/StepUp';
import { Tag } from '../ui/Tag';
import './webhooks.css';

interface Endpoint {
  id: string; url: string; description: string | null; events: string[]; enabled: boolean; disabled_reason: string | null;
  created_at: string; revision: number; previous_secret_until: string | null; last_delivery: { state: string; status: number | null; at: string } | null;
}
interface EventType { type: string; description: string }
interface Delivery {
  id: string; event_id: string; event_type: string; state: 'pending' | 'delivered' | 'failed'; attempts: number; next_attempt_at: string | null;
  last_status: number | null; last_error: string | null; last_duration_ms: number | null; response_excerpt: string | null; created_at: string;
  delivered_at: string | null; payload: unknown;
}

const errText = (e: unknown, fallback: string) => (e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : fallback);
const DSTATE: Record<Delivery['state'], { label: string; color: 'green' | 'blue' | 'red' }> = {
  delivered: { label: 'Delivered', color: 'green' }, pending: { label: 'Retrying', color: 'blue' }, failed: { label: 'Failed', color: 'red' },
};

/**
 * Admin › Webhooks (BPMS-143, docs/13 §2.5): endpoints that receive signed events (Standard
 * Webhooks), with the events each wants, a secret shown once, a test event, a delivery log with
 * replay, and why an endpoint was turned off.
 */
export function Webhooks() {
  const { can } = useSession();
  const [items, reload, error] = useItems<Endpoint>('/api/v1/webhooks');
  const [events] = useItems<EventType>('/api/v1/webhook-events');
  const [adding, setAdding] = useState(false);
  const [secret, setSecret] = useState<{ title: string; secret: string } | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string } | null>(null);
  const [stepUp, setStepUp] = useState<(() => void) | null>(null);
  const configure = can('connector:configure');

  const act = async (f: () => Promise<void>, fallback: string) => {
    setNotice(null);
    try { await f(); } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(() => () => void act(f, fallback));
      else setNotice({ tone: 'danger', title: errText(e, fallback) });
    }
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>Webhooks</h1>
          <p>Tell your other systems when things happen in Procera. Each event is signed, carries ids only, and is retried for about a day if your endpoint is down.</p>
        </div>
        {configure && <Button onPress={() => setAdding(true)}><Plus aria-hidden="true" size="1rem" /> Add endpoint</Button>}
      </div>
      {notice && <Notice tone={notice.tone} title={notice.title} />}
      {error ? <LoadError error={error} onRetry={reload} what="webhooks" /> : items === null ? <Skeleton rows={3} label="Loading webhooks" /> : items.length === 0 ? (
        <div className="card"><Empty icon={Webhook} title="No webhook endpoints">Add an https endpoint to receive events such as a request finishing or a document being signed. <a href="https://casualoffice.github.io/Procera/docs-webhooks.html">How webhooks work</a></Empty></div>
      ) : (
        <ul className="hook-list" aria-label="Webhook endpoints">
          {items.map((ep) => (
            <li key={ep.id} className="card hook">
              <div className="hook-head">
                <span className="row-icon"><Webhook aria-hidden="true" size="1rem" /></span>
                <div className="row-main">
                  <p className="hook-url"><code>{ep.url}</code> {ep.enabled ? <Tag color="green" dot>On</Tag> : <Tag color="gray" dot>Off</Tag>}</p>
                  <p className="row-sub">
                    {ep.description ? `${ep.description} · ` : ''}{ep.events.includes('*') ? 'Every event' : `${ep.events.length} ${ep.events.length === 1 ? 'event' : 'events'}: ${ep.events.join(', ')}`}
                    {ep.last_delivery ? ` · last ${ep.last_delivery.state === 'delivered' ? 'delivered' : 'attempt failed'} ${timeAgo(ep.last_delivery.at)}${ep.last_delivery.status ? ` (${ep.last_delivery.status})` : ''}` : ' · nothing sent yet'}
                  </p>
                </div>
                <div className="hook-actions">
                  {configure && <Button variant="secondary" className="btn-sm" onPress={() => void act(async () => { await api(`/api/v1/webhooks/${ep.id}/test`, { method: 'POST' }); setNotice({ tone: 'success', title: 'A test event is on its way. It appears in the deliveries in a few seconds.' }); setOpen(ep.id); }, 'Couldn’t send a test event.')}><Send aria-hidden="true" size="0.875rem" /> Send test event</Button>}
                  <Button variant="ghost" className="btn-sm" aria-expanded={open === ep.id} onPress={() => setOpen(open === ep.id ? null : ep.id)}>{open === ep.id ? 'Hide deliveries' : 'Deliveries'}</Button>
                </div>
              </div>
              {!ep.enabled && ep.disabled_reason && <Notice tone="warning" title={`Turned off: ${ep.disabled_reason}`}>Fix your endpoint, then turn it back on. Events that happened meanwhile weren’t queued.</Notice>}
              {ep.previous_secret_until && <p className="hook-note">The previous secret also signs events until {shortDate(ep.previous_secret_until)}.</p>}
              {configure && (
                <div className="hook-manage">
                  <Button variant="ghost" className="btn-sm" onPress={() => void act(async () => { await api(`/api/v1/webhooks/${ep.id}`, { method: 'PATCH', headers: { 'If-Match': `"${ep.revision}"` }, body: { enabled: !ep.enabled } }); reload(); }, 'Couldn’t change it.')}>{ep.enabled ? 'Turn off' : 'Turn on'}</Button>
                  {can('connector:manage_secrets') && (
                    <ConfirmDialog title="Roll the signing secret?" confirmLabel="Roll secret" onConfirm={() => void act(async () => {
                      const r = await api<{ secret: string }>(`/api/v1/webhooks/${ep.id}/rotate-secret`, { method: 'POST' });
                      setSecret({ title: 'Your new signing secret', secret: r.secret }); reload();
                    }, 'Couldn’t roll the secret.')}
                      trigger={<Button variant="ghost" className="btn-sm"><RotateCcw aria-hidden="true" size="0.875rem" /> Roll secret</Button>}>
                      <p>Events are signed with both the old and the new secret for 24 hours, so you can update your receiver without missing any.</p>
                    </ConfirmDialog>
                  )}
                  <ConfirmDialog destructive title={`Delete ${ep.url}?`} confirmLabel="Delete endpoint" onConfirm={() => void act(async () => { await api(`/api/v1/webhooks/${ep.id}`, { method: 'DELETE' }); setNotice({ tone: 'success', title: 'Endpoint deleted.' }); reload(); }, 'Couldn’t delete it.')}
                    trigger={<Button variant="ghost" className="btn-sm" aria-label={`Delete ${ep.url}`}><Trash2 aria-hidden="true" size="0.875rem" /> Delete</Button>}>
                    <p>It stops receiving events at once, and its delivery log is deleted.</p>
                  </ConfirmDialog>
                </div>
              )}
              {open === ep.id && <Deliveries endpoint={ep.id} canReplay={configure} onRefresh={reload} />}
            </li>
          ))}
        </ul>
      )}
      {adding && <AddEndpoint events={events ?? []} onClose={() => setAdding(false)} onCreated={(s) => { setAdding(false); setSecret({ title: 'Your signing secret', secret: s }); reload(); }} />}
      {secret && <ShowSecret title={secret.title} secret={secret.secret} onClose={() => setSecret(null)} />}
      <StepUpDialog isOpen={stepUp !== null} onClose={() => setStepUp(null)} onDone={() => { const f = stepUp; setStepUp(null); f?.(); }} />
    </>
  );
}

function AddEndpoint({ events, onClose, onCreated }: { events: EventType[]; onClose: () => void; onCreated: (secret: string) => void }) {
  const [url, setUrl] = useState('https://');
  const [description, setDescription] = useState('');
  const [every, setEvery] = useState(true);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const ok = url.trim().length > 8 && (every || picked.size > 0);
  const submit = async () => {
    setBusy(true); setErr(null);
    try {
      const r = await api<{ secret: string }>('/api/v1/webhooks', { method: 'POST', body: { url: url.trim(), description: description.trim() || null, events: every ? ['*'] : [...picked] } });
      onCreated(r.secret);
    } catch (e) { setErr(errText(e, 'Couldn’t add the endpoint.')); setBusy(false); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && !busy && onClose()} title="Add a webhook endpoint" width={600}
      description="Procera POSTs a small signed JSON event to this URL. Your receiver fetches details through the API with its own token."
      footer={<><Button variant="secondary" onPress={onClose} isDisabled={busy}>Cancel</Button><Button onPress={() => void submit()} isDisabled={!ok || busy}>{busy ? 'Adding…' : 'Add endpoint'}</Button></>}>
      {err && <Notice tone="danger" title={err} />}
      <Form onSubmit={(e) => { e.preventDefault(); void submit(); }} className="stack">
        <TextField label="Endpoint URL" value={url} onChange={setUrl} autoFocus description="https only. Addresses inside a private network are refused." />
        <TextField label="Description (optional)" value={description} onChange={setDescription} maxLength={200} placeholder="e.g. HR system sync" />
        <fieldset className="hook-events">
          <legend>Events</legend>
          <Checkbox isSelected={every} onChange={setEvery}>Every event, including new types as they’re added</Checkbox>
          {!every && (
            <div className="hook-event-grid">
              {events.map((ev) => (
                <Checkbox key={ev.type} isSelected={picked.has(ev.type)} onChange={(on) => setPicked((s) => { const n = new Set(s); if (on) n.add(ev.type); else n.delete(ev.type); return n; })}>
                  <span className="hook-event"><code>{ev.type}</code><span className="muted">{ev.description}</span></span>
                </Checkbox>
              ))}
            </div>
          )}
        </fieldset>
      </Form>
    </FormDialog>
  );
}

function ShowSecret({ title, secret, onClose }: { title: string; secret: string; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  return (
    <FormDialog isOpen onOpenChange={() => undefined} title={title} width={560}
      description="Use it to check the webhook-signature header of each event. This is the only time you’ll see it."
      footer={<Button onPress={onClose}>I’ve saved it</Button>}>
      <div className="token-once">
        <code aria-label="Signing secret">{secret}</code>
        <Button variant="secondary" onPress={() => { void navigator.clipboard?.writeText(secret); setCopied(true); }}>
          {copied ? <><Check aria-hidden="true" size="0.9375rem" /> Copied</> : <><Copy aria-hidden="true" size="0.9375rem" /> Copy</>}
        </Button>
      </div>
      <p className="muted">Events follow the Standard Webhooks specification, so its libraries can verify them. <a href="https://casualoffice.github.io/Procera/docs-webhooks.html">How to verify</a></p>
    </FormDialog>
  );
}

function Deliveries({ endpoint, canReplay, onRefresh }: { endpoint: string; canReplay: boolean; onRefresh: () => void }) {
  const [items, reload, error] = useItems<Delivery>(`/api/v1/webhooks/${endpoint}/deliveries`);
  const [shown, setShown] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  return (
    <div className="hook-deliveries">
      <div className="hook-deliveries-head"><h3>Recent deliveries</h3><Button variant="ghost" className="btn-sm" onPress={() => { reload(); onRefresh(); }}>Refresh</Button></div>
      {err && <Notice tone="danger" title={err} />}
      {error ? <LoadError error={error} onRetry={reload} what="deliveries" /> : items === null ? <Skeleton rows={2} label="Loading deliveries" /> : items.length === 0 ? <p className="muted">Nothing sent yet.</p> : (
        <table className="table" aria-label="Deliveries">
          <thead><tr><th>Event</th><th>Status</th><th className="col-optional">Response</th><th className="col-optional">When</th><th aria-label="Actions" /></tr></thead>
          <tbody>
            {items.map((d) => (
              <tr key={d.id}>
                <td><button type="button" className="link-button" aria-expanded={shown === d.id} onClick={() => setShown(shown === d.id ? null : d.id)}><code>{d.event_type}</code></button>
                  {shown === d.id && <pre className="hook-payload" aria-label="Event body">{JSON.stringify(d.payload, null, 2)}{d.response_excerpt ? `\n\n— response —\n${d.response_excerpt}` : ''}</pre>}
                </td>
                <td><Tag color={DSTATE[d.state].color} dot>{DSTATE[d.state].label}</Tag>{d.state === 'pending' && d.next_attempt_at ? <span className="cell-sub">next try {timeAgo(d.next_attempt_at)}</span> : null}</td>
                <td className="col-optional cell-muted">{d.last_status ? `${d.last_status}${d.last_duration_ms !== null ? ` · ${d.last_duration_ms} ms` : ''}` : d.last_error ?? '—'}{d.attempts > 1 ? ` · ${d.attempts} tries` : ''}</td>
                <td className="col-optional cell-muted">{timeAgo(d.created_at)}</td>
                <td className="cell-actions">{canReplay && d.state !== 'pending' && <Button variant="ghost" className="btn-sm" onPress={() => void api(`/api/v1/webhooks/deliveries/${d.id}/replay`, { method: 'POST' }).then(reload).catch((e) => setErr(errText(e, 'Couldn’t replay it.')))}>Replay</Button>}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
