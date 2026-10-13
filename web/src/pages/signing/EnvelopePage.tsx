import { Ban, Check, Clock, Download, Eye, Pencil, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Form, Label, Text, TextArea, TextField as AriaTextField } from 'react-aria-components';
import { api, download } from '../../lib/api';
import { useResource } from '../../lib/data';
import { navigate, useRoute } from '../../lib/router';
import { useSession } from '../../lib/session';
import { type Envelope, type Recipient, STATE_COLOR, STATE_LABEL, envelopePdf, kindLabel } from '../../lib/signing';
import { setCrumb } from '../../ui/AppShell';
import { Button } from '../../ui/components/Button';
import { FormDialog } from '../../ui/components/FormDialog';
import { Notice } from '../../ui/components/Notice';
import { LoadError, Skeleton } from '../../ui/States';
import { Tag } from '../../ui/Tag';
import { PdfPages } from './PdfPages';
import { dateTime, hueOf, initials, problemText, progressText, signers } from './shared';
import './sender.css';

/** Where one recipient stands, in words (colour is never the only cue). */
function standing(env: Envelope, r: Recipient, next: Recipient | undefined): { text: string; tone: 'done' | 'now' | 'wait' | 'stop' } {
  if (r.state === 'signed') return { text: `Signed ${dateTime(r.signed_at)}`, tone: 'done' };
  if (r.state === 'declined') return { text: `Declined ${dateTime(r.declined_at)}${r.decline_reason ? `: ${r.decline_reason}` : ''}`, tone: 'stop' };
  if (env.state !== 'sent') return { text: env.state === 'draft' ? 'Not sent yet' : 'Didn’t sign', tone: 'wait' };
  const turn = env.order_mode === 'parallel' || next?.id === r.id;
  if (r.viewed_at) return { text: `Viewed ${dateTime(r.viewed_at)}${turn ? ' · signs next' : ''}`, tone: turn ? 'now' : 'wait' };
  return { text: turn ? 'Signs next · waiting' : 'Waiting', tone: turn ? 'now' : 'wait' };
}
const ICON = { done: Check, now: Clock, wait: Clock, stop: X } as const;

/**
 * An envelope (docs/05 §3.22 "Tracking"): who signs in what order and where each stands, the people
 * who get a copy, the original and (once complete) the signed copy, and cancelling with a reason.
 */
export function EnvelopePage({ id }: { id: string }) {
  const { membershipId, can } = useSession();
  const { query } = useRoute();
  const [env, reload, error] = useResource<Envelope>(`/api/v1/envelopes/${id}`);
  const [pdf, setPdf] = useState<ArrayBuffer | null>(null);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string } | null>(query.get('sent') ? { tone: 'success', title: 'Sent. Each signer is told when it’s their turn.' } : null);
  const [voiding, setVoiding] = useState(false);
  useEffect(() => { if (env) setCrumb(env.title); return () => setCrumb(null); }, [env?.title]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { if (query.get('sent')) navigate(`/documents/${id}`, { replace: true }); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { envelopePdf(id).then(setPdf).catch(() => setPdf(null)); }, [id]);

  if (error) return <LoadError error={error} onRetry={reload} what="this document" />;
  if (!env) return <Skeleton variant="doc" label="Loading the document" />;
  const mine = env.created_by === membershipId;
  const order = signers(env);
  const next = env.order_mode === 'sequential' ? order.find((r) => r.state === 'pending') : undefined;
  const cc = env.recipients.filter((r) => r.role === 'cc');
  const ended = env.state === 'declined' || env.state === 'voided';
  const fail = (e: unknown, fallback: string) => setNotice({ tone: 'danger', title: problemText(e, fallback) });

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{env.title}</h1>
          <p className="sign-meta">
            <Tag color={STATE_COLOR[env.state]} dot>{STATE_LABEL[env.state]}</Tag>
            <span>From {env.sender_name}</span>
            {env.sent_at && <span>Sent {dateTime(env.sent_at)}</span>}
            {env.state === 'sent' && env.expires_at && <span>Expires {dateTime(env.expires_at)}</span>}
            {env.completed_at && <span>Completed {dateTime(env.completed_at)}</span>}
          </p>
        </div>
        <div className="sign-head-actions">
          {env.state === 'draft' && mine && <Button onPress={() => navigate(`/documents/${id}/edit`)}><Pencil aria-hidden="true" size="0.9375rem" /> Continue editing</Button>}
          {env.state === 'completed' && <Button onPress={() => void download(`/api/v1/envelopes/${id}/document?signed=true`, `${env.title} (signed).pdf`).catch((e) => fail(e, 'The download failed.'))}><Download aria-hidden="true" size="0.9375rem" /> Download signed copy</Button>}
          <Button variant="secondary" onPress={() => void download(`/api/v1/envelopes/${id}/document`, `${env.title}.pdf`).catch((e) => fail(e, 'The download failed.'))}><Download aria-hidden="true" size="0.9375rem" /> Download original</Button>
          {env.state === 'sent' && (mine || can('document:void')) && <Button variant="secondary" onPress={() => setVoiding(true)}><Ban aria-hidden="true" size="0.9375rem" /> Cancel…</Button>}
        </div>
      </div>
      {notice && <Notice tone={notice.tone} title={notice.title} />}
      {ended && env.ended_reason && <Notice tone="danger" title={env.state === 'declined' ? 'A signer declined, so nobody else can sign.' : 'This was cancelled, so nobody else can sign.'}>{env.ended_reason}</Notice>}
      {env.message && <blockquote className="sign-message">{env.message}</blockquote>}

      <div className="sign-detail">
        <section aria-labelledby="who-h">
          <div className="section-head"><h2 id="who-h">Signers</h2><span className="muted">{progressText(env)} · {env.order_mode === 'sequential' ? 'in order' : 'in any order'}</span></div>
          {order.length === 0 ? <p className="muted">No signers yet.</p> : (
            <ol className="sign-timeline" aria-label="Signers">
              {order.map((r, i) => {
                const s = standing(env, r, next);
                const Icon = ICON[s.tone];
                return (
                  <li key={r.id} className={`sign-step-row is-${s.tone}`}>
                    <span className="sign-swatch sign-swatch-lg" style={{ background: hueOf(env.recipients, r.id) }} aria-hidden="true">{initials(r.name)}</span>
                    <div className="row-main">
                      <p className="row-title">{env.order_mode === 'sequential' ? `${i + 1}. ` : ''}{r.name}</p>
                      <p className="row-sub sign-standing"><Icon aria-hidden="true" size="0.8125rem" /> {s.text}</p>
                    </div>
                  </li>
                );
              })}
            </ol>
          )}
          {cc.length > 0 && (
            <>
              <div className="section-head sign-cc-head"><h2 id="cc-h">Gets a copy</h2></div>
              <ul className="card rows" aria-labelledby="cc-h">
                {cc.map((r) => <li key={r.id}><span className="sign-swatch sign-swatch-cc" aria-hidden="true">{initials(r.name)}</span><div className="row-main"><p className="row-title">{r.name}</p><p className="row-sub">{r.email}</p></div></li>)}
              </ul>
            </>
          )}
          <p className="muted sign-hash">Document fingerprint (SHA-256): <code>{env.document_hash}</code></p>
        </section>

        <section aria-labelledby="doc-h" className="sign-preview">
          <div className="section-head"><h2 id="doc-h"><Eye aria-hidden="true" size="0.9375rem" /> Original document</h2></div>
          {pdf ? (
            <PdfPages data={pdf} label="Original document" maxWidth={640} overlay={(p) => env.fields.filter((f) => f.page === p).map((f, i) => {
              const hue = hueOf(env.recipients, f.recipient_id);
              const who = env.recipients.find((r) => r.id === f.recipient_id)?.name ?? '';
              return (
                <span key={f.id ?? i} className="field-box is-static" title={`${f.label || kindLabel(f.kind)} for ${who}`}
                  style={{ left: `${f.x * 100}%`, top: `${f.y * 100}%`, width: `${f.w * 100}%`, height: `${f.h * 100}%`, borderColor: hue, background: `color-mix(in srgb, ${hue} 10%, #fff)` }}>
                  <span className="field-box-who" style={{ background: hue }}>{initials(who)}</span>
                  <span className="field-box-name">{f.label || kindLabel(f.kind)}</span>
                </span>
              );
            })} />
          ) : <p className="muted">Loading the document…</p>}
        </section>
      </div>

      {voiding && <VoidDialog onClose={() => setVoiding(false)} onDone={() => { setVoiding(false); setNotice({ tone: 'success', title: 'Cancelled. The signers who were asked have been told.' }); reload(); }} id={id} title={env.title} />}
    </>
  );
}

function VoidDialog({ id, title, onClose, onDone }: { id: string; title: string; onClose: () => void; onDone: () => void }) {
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const submit = async () => {
    if (!reason.trim()) return;
    setBusy(true); setErr(null);
    try {
      await api(`/api/v1/envelopes/${id}/void`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: { reason: reason.trim() } });
      onDone();
    } catch (e) { setErr(problemText(e, 'Couldn’t cancel it. Try again.')); setBusy(false); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && !busy && onClose()} title={`Cancel “${title}”?`} width={520}
      description="Nobody can sign it after this. Anyone who already signed keeps their record, and the signers who were asked are told why."
      footer={<><Button variant="secondary" onPress={onClose} isDisabled={busy}>Keep it</Button><Button variant="danger" onPress={() => void submit()} isDisabled={busy || !reason.trim()}><Ban aria-hidden="true" size="0.9375rem" /> {busy ? 'Cancelling…' : 'Cancel signing'}</Button></>}>
      {err && <Notice tone="danger" title={err} />}
      <Form onSubmit={(e) => { e.preventDefault(); void submit(); }}>
        <AriaTextField className="field field-wide" value={reason} onChange={setReason} isRequired maxLength={1000} autoFocus>
          <Label className="field-label">Reason <span aria-hidden="true" className="field-required">*</span></Label>
          <TextArea className="field-input sign-textarea" rows={3} />
          <Text slot="description" className="field-description">The signers see this.</Text>
        </AriaTextField>
      </Form>
    </FormDialog>
  );
}
