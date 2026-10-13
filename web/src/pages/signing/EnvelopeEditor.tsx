import { ArrowDown, ArrowUp, Send, Trash2 } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { Button as AriaButton, ListBox, ListBoxItem, Popover, Select as AriaSelect, SelectValue } from 'react-aria-components';
import { ChevronDown } from 'lucide-react';
import { ApiError, api } from '../../lib/api';
import { type Member, toFailure, type LoadFailure, useItems } from '../../lib/data';
import { navigate } from '../../lib/router';
import { type Envelope, type Recipient, envelopePdf } from '../../lib/signing';
import { setCrumb } from '../../ui/AppShell';
import { Button } from '../../ui/components/Button';
import { Notice } from '../../ui/components/Notice';
import { Select } from '../../ui/components/Select';
import { LoadError, Skeleton } from '../../ui/States';
import { type Audience, AudiencePicker, audienceSummary, useDirectory } from '../../ui/Audience';
import { FieldPlacer, type LocalField } from './FieldPlacer';
import { hueOf, initials, problemText, putDraft, signers } from './shared';
import './sender.css';

type Save = 'idle' | 'dirty' | 'saving' | 'saved' | 'error';
const ROLES = [{ id: 'signer', label: 'Signs' }, { id: 'cc', label: 'Gets a copy' }];
const toLocal = (e: Envelope): LocalField[] => e.fields.map((f) => ({ ...f, key: f.id ?? crypto.randomUUID() }));
const toWire = (fs: LocalField[]) => fs.map(({ key: _k, id: _i, value: _v, ...f }) => ({ ...f, label: f.label?.trim() || null, required: f.kind === 'signature' || f.required }));

/**
 * The draft editor (docs/05 §3.22): recipients in signing order, fields placed on the document, then
 * review and send. Every change saves to the draft (If-Match on its revision); someone else's change
 * meanwhile reloads the latest version with a notice.
 */
export function EnvelopeEditor({ id }: { id: string }) {
  const [env, setEnv] = useState<Envelope | null>(null);
  const [fields, setFields] = useState<LocalField[]>([]);
  const [loadError, setLoadError] = useState<LoadFailure | null>(null);
  const [pdf, setPdf] = useState<ArrayBuffer | null>(null);
  const [pdfError, setPdfError] = useState<string | null>(null);
  const [save, setSave] = useState<Save>('idle');
  const [notice, setNotice] = useState<{ tone: 'warning' | 'danger'; title: string } | null>(null);
  const [sendError, setSendError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [audience, setAudience] = useState<Audience>({ all: false, departments: [], groups: [], roles: [] });
  const dir = useDirectory();
  const rev = useRef(0);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const latest = useRef<LocalField[]>([]);
  const dirty = useRef(false);

  const adopt = useCallback((e: Envelope, withFields = true) => {
    rev.current = e.revision;
    setEnv(e);
    if (withFields) { const f = toLocal(e); latest.current = f; setFields(f); }
  }, []);
  const load = useCallback(async () => {
    try {
      const e = await api<Envelope>(`/api/v1/envelopes/${id}`);
      if (e.state !== 'draft') { navigate(`/documents/${id}`, { replace: true }); return; }
      adopt(e); setLoadError(null);
    } catch (e) { setLoadError(toFailure(e)); }
  }, [id, adopt]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => { envelopePdf(id).then(setPdf).catch((e) => setPdfError(problemText(e, 'The document couldn’t be loaded.'))); }, [id]);
  useEffect(() => { if (env) setCrumb(env.title); return () => setCrumb(null); }, [env?.title]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => clearTimeout(timer.current), []);

  /** Runs draft writes one at a time, each with the revision the previous one returned. */
  const run = <T,>(fn: () => Promise<T>): Promise<T> => {
    const p = queue.current.then(fn, fn);
    queue.current = p.catch(() => undefined);
    return p;
  };
  const failed = async (e: unknown) => {
    if (e instanceof ApiError && e.status === 412) {
      clearTimeout(timer.current); dirty.current = false;
      setNotice({ tone: 'warning', title: 'This draft was changed somewhere else, so the latest version was loaded. Check it and carry on.' });
      setSave('idle');
      await load();
    } else {
      setSave('error');
      setNotice({ tone: 'danger', title: problemText(e, 'Couldn’t save the draft. Check your connection and try again.') });
    }
  };

  const saveFields = () => run(async () => {
    if (!dirty.current) return true;
    dirty.current = false;
    setSave('saving');
    try {
      const e = await putDraft(id, 'fields', { fields: toWire(latest.current) }, rev.current);
      adopt(e, false);
      setSave(dirty.current ? 'dirty' : 'saved');
      return true;
    } catch (e) { await failed(e); return false; }
  });
  const changeFields = (next: LocalField[]) => {
    latest.current = next;
    setFields(next);
    dirty.current = true;
    setSave('dirty');
    clearTimeout(timer.current);
    timer.current = setTimeout(() => void saveFields(), 600);
  };
  const setRecipients = async (list: { membership_id: string; role: string }[]) => {
    clearTimeout(timer.current);
    if (!(await saveFields())) return; // fields first: the server drops fields of anyone removed
    return run(async () => {
      setSave('saving'); setNotice(null);
      try {
        const e = await putDraft(id, 'recipients', { recipients: list }, rev.current);
        if (!dirty.current) adopt(e);
        else { // edited meanwhile: keep those edits, minus fields of anyone who no longer signs
          adopt(e, false);
          const ok = new Set(e.recipients.filter((r) => r.role === 'signer').map((r) => r.id));
          changeFields(latest.current.filter((f) => ok.has(f.recipient_id)));
        }
        setSave(dirty.current ? 'dirty' : 'saved');
      } catch (e) { await failed(e); }
    });
  };

  const send = async () => {
    setSending(true); setSendError(null);
    clearTimeout(timer.current);
    if (!(await saveFields())) { setSending(false); return; }
    try {
      if (env?.bulk) {
        const b = await run(() => api<{ id: string }>(`/api/v1/envelopes/${id}/send-bulk`, { method: 'POST', headers: { 'If-Match': `"${rev.current}"` }, body: { audience } }));
        navigate(`/documents/batches/${b.id}?sent=1`);
        return;
      }
      await run(() => api<Envelope>(`/api/v1/envelopes/${id}/send`, { method: 'POST', headers: { 'If-Match': `"${rev.current}"`, 'Idempotency-Key': crypto.randomUUID() } }));
      navigate(`/documents/${id}?sent=1`);
    } catch (e) {
      if (e instanceof ApiError && e.status === 412) await failed(e);
      else setSendError(problemText(e, 'Couldn’t send it. Try again.'));
      setSending(false);
    }
  };

  if (loadError) return <LoadError error={loadError} onRetry={() => void load()} what="this draft" />;
  if (!env) return <Skeleton variant="doc" label="Loading the draft" />;
  // A group send's only signer stands for each person it goes to.
  const shown: Envelope = env.bulk ? { ...env, recipients: env.recipients.map((r) => (r.role === 'signer' ? { ...r, name: 'Each person', email: '' } : r)) } : env;
  const list = [...shown.recipients].sort((a, b) => a.ord - b.ord);
  const noAudience = !audience.all && !audience.departments.length && !audience.groups.length && !audience.roles.length && !(audience.locations ?? []).length;
  const missing = signers(shown).filter((r) => !fields.some((f) => f.recipient_id === r.id && f.kind === 'signature'));
  const status = save === 'saving' ? 'Saving…' : save === 'dirty' ? 'Unsaved changes' : save === 'saved' ? 'Saved' : save === 'error' ? 'Not saved' : 'Draft';

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{env.title}</h1>
          <p>{env.pages.length} {env.pages.length === 1 ? 'page' : 'pages'} · {env.bulk ? 'Each person signs their own copy' : env.order_mode === 'sequential' ? 'Signed one after another' : 'Signed all at once'}{env.mode === 'strong' ? ' · Signers confirm it’s them' : ''}</p>
        </div>
        <span className={`sign-save sign-save-${save}`} role="status" aria-live="polite">{status}</span>
      </div>
      {notice && <Notice tone={notice.tone} title={notice.title} />}

      <div className="sign-flow">
        <section className="sign-step" aria-labelledby="step-rcp">
          <h2 id="step-rcp" className="sign-step-title"><span className="sign-step-no" aria-hidden="true">1</span> Recipients</h2>
          {env.bulk ? (
            <div className="stack">
              <AudiencePicker value={audience} onChange={setAudience} legend="Who receives it" summary="Sends a copy to" />
              <p className="muted sign-hint">Each person gets their own copy to sign, with the fields you place for “Each person”.</p>
              <Recipients env={shown} list={list.filter((r) => r.role === 'cc')} busy={save === 'saving'} copiesOnly
                onSet={(l) => void setRecipients(l.filter((x) => x.role === 'cc'))} />
            </div>
          ) : <Recipients env={env} list={list} busy={save === 'saving'} onSet={(l) => void setRecipients(l)} />}
        </section>

        <section className="sign-step" aria-labelledby="step-fields">
          <h2 id="step-fields" className="sign-step-title"><span className="sign-step-no" aria-hidden="true">2</span> Fields</h2>
          <FieldPlacer env={shown} fields={fields} onChange={changeFields} pdf={pdf} pdfError={pdfError} />
        </section>

        <section className="sign-step" aria-labelledby="step-send">
          <h2 id="step-send" className="sign-step-title"><span className="sign-step-no" aria-hidden="true">3</span> Review and send</h2>
          <div className="stack">
            {env.bulk && <p className="sign-bulk-summary">{noAudience ? 'Choose who receives it above.' : `${audienceSummary(audience, dir)}: each person signs their own copy.`}</p>}
            {list.length === 0 ? <p className="muted">Add the people who sign first.</p> : (
              <ol className="card rows sign-review" aria-label="Who it goes to">
                {list.map((r) => (
                  <li key={r.id}>
                    <span className="row-meta">{r.role === 'signer' && env.order_mode === 'sequential' && !env.bulk ? `${signers(env).findIndex((s) => s.id === r.id) + 1}.` : '·'}</span>
                    <div className="row-main">
                      <p className="row-title">{r.name}</p>
                      <p className="row-sub">{r.role === 'signer' ? (() => { const n = fields.filter((f) => f.recipient_id === r.id).length; return `Signs · ${n} ${n === 1 ? 'field' : 'fields'}`; })() : 'Gets a copy when everyone has signed'}</p>
                    </div>
                  </li>
                ))}
              </ol>
            )}
            {missing.length > 0 && <Notice tone="warning" title={`Place a signature field for ${missing.map((r) => r.name).join(', ')}.`} />}
            {sendError && <Notice tone="danger" title={sendError} />}
            <div className="sign-actions">
              <Button variant="secondary" onPress={() => navigate('/documents')}>Save and close</Button>
              <Button onPress={() => void send()} isDisabled={sending || signers(env).length === 0 || (env.bulk && noAudience)}><Send aria-hidden="true" size="0.9375rem" /> {sending ? 'Sending…' : env.bulk ? 'Send to everyone chosen' : 'Send for signature'}</Button>
            </div>
          </div>
        </section>
      </div>
    </>
  );
}

function Recipients({ env, list, busy, onSet, copiesOnly = false }: { env: Envelope; list: Recipient[]; busy: boolean; onSet: (l: { membership_id: string; role: string }[]) => void; copiesOnly?: boolean }) {
  const [members] = useItems<Member>('/api/v1/members');
  const [pick, setPick] = useState(0); // remounts the picker so it clears after each pick
  const wire = (rs: Recipient[]) => rs.map((r) => ({ membership_id: r.membership_id, role: r.role }));
  const taken = new Set(copiesOnly ? list.map((r) => r.membership_id) : list.map((r) => r.membership_id));
  const options = (members ?? []).filter((m) => m.status === 'active' && !taken.has(m.id)).map((m) => ({ id: m.id, label: `${m.display_name} (${m.email})` }));
  const move = (i: number, d: -1 | 1) => { const n = [...list]; [n[i], n[i + d]] = [n[i + d]!, n[i]!]; onSet(wire(n)); };
  let signerNo = 0;

  return (
    <div className="stack">
      {list.length > 0 && (
        <ol className="card rows sign-recipients" aria-label="Recipients">
          {list.map((r, i) => {
            const no = r.role === 'signer' ? ++signerNo : null;
            return (
              <li key={r.id}>
                <span className="sign-order" aria-hidden="true">{env.order_mode === 'sequential' && no ? no : ''}</span>
                {r.role === 'signer' ? <span className="sign-swatch sign-swatch-lg" style={{ background: hueOf(env.recipients, r.id) }} aria-hidden="true">{initials(r.name)}</span> : <span className="sign-swatch sign-swatch-lg sign-swatch-cc" aria-hidden="true">{initials(r.name)}</span>}
                <div className="row-main">
                  <p className="row-title">{r.name}</p>
                  <p className="row-sub">{r.email}</p>
                </div>
                {!copiesOnly && <RoleSelect name={r.name} value={r.role} onChange={(role) => onSet(wire(list.map((x) => (x.id === r.id ? { ...x, role } : x))))} />}
                <span className="sign-row-actions">
                  <Button variant="ghost" className="btn-sm btn-icon" aria-label={`Move ${r.name} up`} isDisabled={busy || i === 0} onPress={() => move(i, -1)}><ArrowUp aria-hidden="true" size="0.875rem" /></Button>
                  <Button variant="ghost" className="btn-sm btn-icon" aria-label={`Move ${r.name} down`} isDisabled={busy || i === list.length - 1} onPress={() => move(i, 1)}><ArrowDown aria-hidden="true" size="0.875rem" /></Button>
                  <Button variant="ghost" className="btn-sm btn-icon" aria-label={`Remove ${r.name}`} isDisabled={busy} onPress={() => onSet(wire(list.filter((x) => x.id !== r.id)))}><Trash2 aria-hidden="true" size="0.875rem" /></Button>
                </span>
              </li>
            );
          })}
        </ol>
      )}
 <Select key={pick} label={copiesOnly ? 'Send a copy to' : 'Add a person'} placeholder={members === null ? 'Loading people…' : options.length ? 'Choose someone in your workspace…' : 'Everyone is already added'}
        isDisabled={busy || members === null || options.length === 0} options={options} selectedKey={null}
        onSelectionChange={(k) => { if (!k) return; setPick((n) => n + 1); onSet([...wire(list), { membership_id: String(k), role: copiesOnly ? 'cc' : 'signer' }]); }}
        description={copiesOnly ? 'Optional: people who get each signed copy, such as HR.' : env.order_mode === 'sequential' ? 'People sign in this order. Use the arrows to change it.' : 'Everyone is asked at the same time.'} />
    </div>
  );
}

/** A compact role picker for one row (the row's name is in its label). */
function RoleSelect({ name, value, onChange }: { name: string; value: 'signer' | 'cc'; onChange: (r: 'signer' | 'cc') => void }) {
  return (
    <AriaSelect aria-label={`Role for ${name}`} selectedKey={value} onSelectionChange={(k) => k && k !== value && onChange(k as 'signer' | 'cc')} className="sign-role">
      <AriaButton className="field-input select-trigger"><SelectValue /><ChevronDown aria-hidden="true" size="1rem" /></AriaButton>
      <Popover className="popover">
        <ListBox className="listbox" items={ROLES}>{(o) => <ListBoxItem id={o.id} className="listbox-item">{o.label}</ListBoxItem>}</ListBox>
      </Popover>
    </AriaSelect>
  );
}
