import { ArrowDown, Check, Download, Eraser, PenLine, ShieldCheck, Type, X } from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ApiError, api, download } from '../../lib/api';
import { type LoadFailure, toFailure } from '../../lib/data';
import { shortDate } from '../../lib/format';
import { Link } from '../../lib/router';
import { type Ceremony as CeremonyData, type SignField, envelopePdf, kindLabel } from '../../lib/signing';
import { useSession } from '../../lib/session';
import { setCrumb } from '../../ui/AppShell';
import { Button } from '../../ui/components/Button';
import { Checkbox } from '../../ui/components/Checkbox';
import { FormDialog } from '../../ui/components/FormDialog';
import { Notice } from '../../ui/components/Notice';
import { TextField } from '../../ui/components/TextField';
import { LoadError, Skeleton } from '../../ui/States';
import { StepUpDialog } from '../../ui/StepUp';
import { PdfPages } from './PdfPages';
import './signing.css';

type Adopted = { kind: 'typed'; text: string } | { kind: 'drawn'; text: string; image: string };

const errText = (e: unknown, fallback: string) => (e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : fallback);
const initialsOf = (name: string) => name.split(/\s+/).filter(Boolean).map((w) => w[0]!.toUpperCase()).slice(0, 3).join('');

/**
 * The signing ceremony (BPMS-107, docs/05 §3.22, FR-DS04): who sent it and why, consent to signing
 * electronically, the document with only my fields (a "Next field" guide counts what's left), a
 * typed or drawn signature adopted once and applied field by field, then Finish. Decline needs a
 * reason. Strong envelopes ask the signer to confirm it's them. Works at phone width and by keyboard.
 */
export function Ceremony({ recipient }: { recipient: string }) {
  const { user } = useSession();
  const [data, setData] = useState<CeremonyData | null>(null);
  const [pdf, setPdf] = useState<ArrayBuffer | null>(null);
  const [error, setError] = useState<LoadFailure | null>(null);
  const [consent, setConsent] = useState(false);
  const [adopted, setAdopted] = useState<Adopted | null>(null);
  const [applied, setApplied] = useState<Set<string>>(new Set());
  const [values, setValues] = useState<Record<string, string | boolean>>({});
  const [adopting, setAdopting] = useState<string | null>(null); // the field that asked for a signature
  const [declining, setDeclining] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [stepUp, setStepUp] = useState(false);
  const [done, setDone] = useState<{ completed: boolean } | null>(null);

  const load = useCallback(async () => {
    try {
      const d = await api<CeremonyData>(`/api/v1/signing/${recipient}`);
      setData(d);
      setCrumb(d.envelope.title);
      setPdf(await envelopePdf(d.envelope.id, d.envelope.state === 'completed'));
    } catch (e) { setError(toFailure(e)); }
  }, [recipient]);
  useEffect(() => { void load(); }, [load]);

  const mine = useMemo(() => data?.fields ?? [], [data]);
  const fillable = useMemo(() => mine.filter((f) => ['signature', 'initials', 'text', 'checkbox'].includes(f.kind)), [mine]);
  const isDone = useCallback((f: SignField) => {
    if (f.kind === 'signature' || f.kind === 'initials') return applied.has(f.id!);
    if (f.kind === 'text') return !f.required || String(values[f.id!] ?? '').trim() !== '';
    if (f.kind === 'checkbox') return !f.required || values[f.id!] === true;
    return true;
  }, [applied, values]);
  const remaining = fillable.filter((f) => !isDone(f));
  const canSign = !!data?.my_turn && done === null;
  const ready = canSign && consent && !!adopted && remaining.length === 0;

  const focusField = (f: SignField) => {
    const el = document.getElementById(`sf-${f.id}`);
    el?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    window.setTimeout(() => el?.focus({ preventScroll: true }), 250);
  };
  const next = () => { const f = remaining[0]; if (f) focusField(f); };

  const apply = (f: SignField) => {
    if (!consent) { setProblem('First agree to sign electronically, above the document.'); return; }
    if (!adopted) { setAdopting(f.id!); return; }
    setApplied((s) => new Set(s).add(f.id!));
    setProblem(null);
  };

  const submit = async () => {
    if (!data || !adopted) return;
    setBusy(true); setProblem(null);
    const vals = Object.fromEntries(Object.entries(values).filter(([id]) => mine.some((f) => f.id === id && (f.kind === 'text' || f.kind === 'checkbox'))));
    try {
      const r = await api<{ state: string }>(`/api/v1/signing/${recipient}/sign`, {
        method: 'POST',
        body: { document_hash: data.envelope.document_hash, consent, adopted, values: vals },
      });
      setDone({ completed: r.state === 'completed' });
      window.scrollTo({ top: 0, behavior: 'smooth' });
    } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(true);
      else setProblem(errText(e, 'Couldn’t sign. Try again.'));
    } finally { setBusy(false); }
  };

  if (error) return <LoadError error={error} onRetry={() => { setError(null); void load(); }} what="this document" />;
  if (!data) return <Skeleton rows={4} label="Loading the document" />;
  const e = data.envelope;
  const waitingFor = data.progress.find((p) => p.state === 'pending' && p.ord < data.recipient.ord);

  return (
    <div className="ceremony">
      <header className="cer-head">
        <div className="cer-head-main">
          <p className="cer-eyebrow">From {e.sender_name}{e.expires_at && e.state === 'sent' ? ` · sign by ${shortDate(e.expires_at)}` : ''}</p>
          <h1>{e.title}</h1>
        </div>
        {canSign && (
          <div className="cer-actions">
            <span className="cer-left" aria-live="polite">{remaining.length === 0 ? 'All fields done' : `${remaining.length} ${remaining.length === 1 ? 'field' : 'fields'} left`}</span>
            {remaining.length > 0 && <Button variant="secondary" onPress={next}><ArrowDown aria-hidden="true" size="0.9375rem" /> Next field</Button>}
            <Button variant="ghost" onPress={() => setDeclining(true)}>Decline</Button>
            <Button onPress={() => void submit()} isDisabled={!ready || busy}><Check aria-hidden="true" size="0.9375rem" /> {busy ? 'Signing…' : 'Finish'}</Button>
          </div>
        )}
      </header>

      {done && (
        <Notice tone="success" title={`You signed ${e.title}.`}>
          {done.completed
            ? <>Everyone has signed. <Button variant="secondary" className="btn-sm" onPress={() => void download(`/api/v1/envelopes/${e.id}/document?signed=true`, `${e.title} (signed).pdf`)}><Download aria-hidden="true" size="0.875rem" /> Download the signed copy</Button></>
            : <>We’ll let you know when everyone has signed, with a copy for your records.</>}
        </Notice>
      )}
      {!done && e.state === 'completed' && (
        <Notice tone="success" title="Everyone has signed this document.">
          <Button variant="secondary" className="btn-sm" onPress={() => void download(`/api/v1/envelopes/${e.id}/document?signed=true`, `${e.title} (signed).pdf`)}><Download aria-hidden="true" size="0.875rem" /> Download the signed copy</Button>
        </Notice>
      )}
      {!done && data.recipient.state === 'signed' && e.state === 'sent' && <Notice tone="info" title={`You signed on ${shortDate(data.recipient.signed_at!)}.`}>We’ll let you know when everyone has signed.</Notice>}
      {e.state === 'declined' && <Notice tone="warning" title="This document was declined, so nobody else can sign it." />}
      {e.state === 'voided' && <Notice tone="warning" title={`${e.sender_name} cancelled this document. It no longer needs your signature.`} />}
      {e.state === 'expired' && <Notice tone="warning" title="This document expired before everyone signed it." />}
      {data.recipient.role === 'cc' && e.state === 'sent' && <Notice tone="info" title="You’ll get a copy when everyone has signed." />}
      {!data.my_turn && data.recipient.role === 'signer' && data.recipient.state === 'pending' && e.state === 'sent' && waitingFor && (
        <Notice tone="info" title={`${waitingFor.name} signs first. We’ll email you when it’s your turn.`} />
      )}

      {canSign && (
        <section className="cer-intro card" aria-labelledby="cer-intro">
          <h2 id="cer-intro" className="visually-hidden">Before you sign</h2>
          {e.message && <blockquote className="cer-message"><p>{e.message}</p><footer>— {e.sender_name}</footer></blockquote>}
          <div className="cer-consent">
            <Checkbox isSelected={consent} onChange={setConsent}>{data.consent_text}</Checkbox>
            {e.mode === 'strong' && <p className="cer-note"><ShieldCheck aria-hidden="true" size="0.875rem" /> You’ll be asked to confirm it’s you when you finish.</p>}
          </div>
          <div className="cer-adopt">
            {adopted ? (
              <>
                <span className="cer-adopt-label">Your signature</span>
                <SignaturePreview adopted={adopted} />
                <Button variant="ghost" className="btn-sm" onPress={() => setAdopting('change')}>Change</Button>
              </>
            ) : (
              <Button variant="secondary" onPress={() => setAdopting('intro')} isDisabled={!consent}><PenLine aria-hidden="true" size="0.9375rem" /> Adopt your signature</Button>
            )}
          </div>
          <p className="cer-meaning muted">By signing, you confirm: “{data.meaning}.”</p>
        </section>
      )}
      {problem && <Notice tone="danger" title={problem} />}

      {pdf ? (
        <PdfPages data={pdf} label={`Document: ${e.title}`} onError={(m) => setProblem(m)}
          overlay={(page) => (done || e.state === 'completed') ? null : mine.filter((f) => f.page === page).map((f) => (
            <FieldBox key={f.id} f={f} active={canSign} done={isDone(f)} adopted={adopted} applied={applied.has(f.id!)} name={data.recipient.name}
              value={values[f.id!]} onApply={() => apply(f)} onValue={(v) => setValues((s) => ({ ...s, [f.id!]: v }))} />
          ))} />
      ) : <Skeleton rows={6} label="Loading the document" />}

      {canSign && <p className="cer-foot muted">Signed copies include a certificate of completion with each signer’s name, time and how they signed. <Link to="/work/signatures">Back to your documents</Link></p>}

      {adopting && (
        <AdoptDialog initialName={adopted?.text ?? user?.display_name ?? data.recipient.name} onClose={() => setAdopting(null)}
          onAdopt={(a) => {
            setAdopted(a);
            if (adopting !== 'intro' && adopting !== 'change') setApplied((s) => new Set(s).add(adopting));
            setAdopting(null);
          }} />
      )}
      {declining && <DeclineDialog recipient={recipient} title={e.title} onClose={() => setDeclining(false)} onDone={() => { setDeclining(false); void load(); }} />}
      <StepUpDialog isOpen={stepUp} onClose={() => setStepUp(false)} onDone={() => { setStepUp(false); void submit(); }} />
    </div>
  );
}

function SignaturePreview({ adopted, initials = false }: { adopted: Adopted; initials?: boolean }) {
  if (adopted.kind === 'drawn') return <img className="sig-img" src={adopted.image} alt={`Drawn signature of ${adopted.text}`} />;
  return <span className="sig-typed">{initials ? initialsOf(adopted.text) : adopted.text}</span>;
}

function FieldBox({ f, active, done, adopted, applied, name, value, onApply, onValue }: {
  f: SignField; active: boolean; done: boolean; adopted: Adopted | null; applied: boolean; name: string;
  value: string | boolean | undefined; onApply: () => void; onValue: (v: string | boolean) => void;
}) {
  const style = { left: `${f.x * 100}%`, top: `${f.y * 100}%`, width: `${f.w * 100}%`, height: `${f.h * 100}%` };
  const label = f.label ?? kindLabel(f.kind);
  const cls = `sig-field kind-${f.kind}${done ? ' is-done' : ''}${f.required ? ' is-required' : ''}`;
  if (f.kind === 'signature' || f.kind === 'initials') {
    return (
      <button type="button" id={`sf-${f.id}`} className={cls} style={style} disabled={!active}
        aria-label={applied ? `${label}: signed` : `${f.kind === 'signature' ? 'Sign here' : 'Initial here'}${f.label ? `: ${f.label}` : ''}`} onClick={onApply}>
        {applied && adopted ? <SignaturePreview adopted={adopted} initials={f.kind === 'initials'} /> : <span className="sig-field-cta"><PenLine aria-hidden="true" size="0.875rem" /> {f.kind === 'signature' ? 'Sign' : 'Initial'}</span>}
      </button>
    );
  }
  if (f.kind === 'text') {
    return (
      <input id={`sf-${f.id}`} className={cls} style={style} disabled={!active} value={String(value ?? '')} maxLength={200}
        aria-label={`${label}${f.required ? ' (required)' : ''}`} placeholder={label} onChange={(e) => onValue(e.target.value)} />
    );
  }
  if (f.kind === 'checkbox') {
    return (
      <button type="button" role="checkbox" id={`sf-${f.id}`} className={cls} style={style} disabled={!active} aria-checked={value === true}
        aria-label={`${label}${f.required ? ' (required)' : ''}`} onClick={() => onValue(value !== true)}>
        {value === true && <Check aria-hidden="true" size="0.875rem" />}
      </button>
    );
  }
  // Filled in for the signer: shown as it will print.
  const today = new Date().toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });
  return <span className={`${cls} is-auto`} style={style} aria-label={`${label}: filled in when you sign`}>{f.kind === 'date_signed' ? today : name}</span>;
}

function AdoptDialog({ initialName, onClose, onAdopt }: { initialName: string; onClose: () => void; onAdopt: (a: Adopted) => void }) {
  const [tab, setTab] = useState<'type' | 'draw'>('type');
  const [name, setName] = useState(initialName);
  const [drawn, setDrawn] = useState<string | null>(null);
  const ok = name.trim().length > 0 && (tab === 'type' || !!drawn);
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Adopt your signature" width={560}
      description="It’s placed on each signature field you click. Your typed name is recorded with it."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button>
        <Button isDisabled={!ok} onPress={() => onAdopt(tab === 'type' ? { kind: 'typed', text: name.trim() } : { kind: 'drawn', text: name.trim(), image: drawn! })}>Adopt and sign</Button></>}>
      <div className="adopt-tabs" role="radiogroup" aria-label="How to sign">
        <button type="button" role="radio" aria-checked={tab === 'type'} className="adopt-tab" onClick={() => setTab('type')}><Type aria-hidden="true" size="0.9375rem" /> Type</button>
        <button type="button" role="radio" aria-checked={tab === 'draw'} className="adopt-tab" onClick={() => setTab('draw')}><PenLine aria-hidden="true" size="0.9375rem" /> Draw</button>
      </div>
      <TextField label="Full name" value={name} onChange={setName} maxLength={100} autoFocus />
      {tab === 'type' ? (
        <div className="adopt-preview" aria-label="Preview"><span className="sig-typed">{name.trim() || 'Your name'}</span></div>
      ) : <DrawPad onChange={setDrawn} />}
    </FormDialog>
  );
}

/** A canvas to draw a signature with a mouse, finger or pen. Typing is always offered as well. */
function DrawPad({ onChange }: { onChange: (png: string | null) => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const drawing = useRef(false);
  const [empty, setEmpty] = useState(true);
  useEffect(() => {
    const c = canvas.current!;
    const r = c.getBoundingClientRect();
    const ratio = window.devicePixelRatio || 1;
    c.width = Math.round(r.width * ratio);
    c.height = Math.round(r.height * ratio);
    const ctx = c.getContext('2d')!;
    ctx.scale(ratio, ratio);
    ctx.lineWidth = 2.2; ctx.lineCap = 'round'; ctx.lineJoin = 'round'; ctx.strokeStyle = '#1c1c1a';
  }, []);
  const at = (e: React.PointerEvent) => { const r = canvas.current!.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top] as const; };
  const finish = () => {
    if (!drawing.current) return;
    drawing.current = false;
    onChange(canvas.current!.toDataURL('image/png'));
  };
  const clear = () => {
    const c = canvas.current!;
    c.getContext('2d')!.clearRect(0, 0, c.width, c.height);
    setEmpty(true); onChange(null);
  };
  return (
    <div className="drawpad">
      <canvas ref={canvas} className="drawpad-canvas" aria-label="Signature drawing area" role="img"
        onPointerDown={(e) => { e.currentTarget.setPointerCapture(e.pointerId); drawing.current = true; const [x, y] = at(e); const ctx = canvas.current!.getContext('2d')!; ctx.beginPath(); ctx.moveTo(x, y); setEmpty(false); }}
        onPointerMove={(e) => { if (!drawing.current) return; const [x, y] = at(e); const ctx = canvas.current!.getContext('2d')!; ctx.lineTo(x, y); ctx.stroke(); }}
        onPointerUp={finish} onPointerCancel={finish} />
      <div className="drawpad-bar">
        <span className="muted">{empty ? 'Draw your signature above' : 'Looks good?'}</span>
        <Button variant="ghost" className="btn-sm" onPress={clear} isDisabled={empty}><Eraser aria-hidden="true" size="0.875rem" /> Clear</Button>
      </div>
    </div>
  );
}

function DeclineDialog({ recipient, title, onClose, onDone }: { recipient: string; title: string; onClose: () => void; onDone: () => void }) {
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const go = async () => {
    setBusy(true); setErr(null);
    try { await api(`/api/v1/signing/${recipient}/decline`, { method: 'POST', body: { reason: reason.trim() } }); onDone(); }
    catch (e) { setErr(errText(e, 'Couldn’t decline. Try again.')); }
    finally { setBusy(false); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && !busy && onClose()} title={`Decline to sign ${title}?`} width={480}
      description="The sender is told why, and nobody else can sign this document."
      footer={<><Button variant="secondary" onPress={onClose} isDisabled={busy}>Keep it</Button>
        <Button variant="danger" onPress={() => void go()} isDisabled={busy || !reason.trim()}><X aria-hidden="true" size="0.9375rem" /> Decline</Button></>}>
      {err && <Notice tone="danger" title={err} />}
      <TextField label="Reason" value={reason} onChange={setReason} maxLength={1000} autoFocus description="For example: the start date is wrong." />
    </FormDialog>
  );
}

