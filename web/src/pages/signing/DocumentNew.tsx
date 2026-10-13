import { FileText, Upload, X } from 'lucide-react';
import { useRef, useState } from 'react';
import { Form, Label, Text, TextArea, TextField as AriaTextField } from 'react-aria-components';
import { api } from '../../lib/api';
import { navigate } from '../../lib/router';
import type { Envelope } from '../../lib/signing';
import { type FileInfo, formatBytes, uploadFile } from '../../lib/upload';
import { Button } from '../../ui/components/Button';
import { Checkbox } from '../../ui/components/Checkbox';
import { Notice } from '../../ui/components/Notice';
import { RadioCards } from '../../ui/components/RadioCards';
import { Select } from '../../ui/components/Select';
import { TextField } from '../../ui/components/TextField';
import { problemText } from './shared';
import './sender.css';

const EXPIRY = [7, 14, 30, 60, 90].map((d) => ({ id: String(d), label: `${d} days` }));
const ORDER = [
  { id: 'sequential', label: 'One after another', description: 'Each person is asked when the one before them has signed.' },
  { id: 'parallel', label: 'All at once', description: 'Everyone is asked straight away and can sign in any order.' },
];

/** True when the first bytes say PDF, whatever the file is called. */
async function looksLikePdf(f: File): Promise<boolean> {
  const head = new Uint8Array(await f.slice(0, 5).arrayBuffer());
  return String.fromCharCode(...head) === '%PDF-';
}

/**
 * Send for signature, step 1 (docs/05 §3.22): the document, its title and message, the signing order
 * and how signers confirm it's them. Continuing saves a draft and opens the draft editor, so a reload
 * never loses the work.
 */
export function DocumentNew() {
  const [file, setFile] = useState<{ name: string; size: number; info: FileInfo | null; progress: number } | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [title, setTitle] = useState('');
  const [titleTouched, setTitleTouched] = useState(false);
  const [message, setMessage] = useState('');
  const [order, setOrder] = useState('sequential');
  const [strong, setStrong] = useState(false);
  const [expiry, setExpiry] = useState('30');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);

  const pick = async (f: File | undefined) => {
    if (input.current) input.current.value = '';
    if (!f) return;
    setFileError(null);
    if (!(await looksLikePdf(f))) { setFile(null); setFileError(`“${f.name}” isn’t a PDF. Only PDF documents can be sent for signature.`); return; }
    if (!titleTouched) setTitle(f.name.replace(/\.pdf$/i, '').trim().slice(0, 200));
    setFile({ name: f.name, size: f.size, info: null, progress: 0 });
    try {
      const info = await uploadFile(f, (p) => setFile((s) => (s ? { ...s, progress: p } : s)));
      if (info.state !== 'clean') {
        setFile(null);
        setFileError(info.state === 'quarantined' ? `“${f.name}” was flagged by the malware scan, so it can’t be used.` : info.state === 'rejected' ? `“${f.name}” isn’t a file type that can be uploaded.` : `“${f.name}” couldn’t be scanned yet. Try again in a few minutes.`);
        return;
      }
      setFile({ name: f.name, size: f.size, info, progress: 1 });
    } catch (e) { setFile(null); setFileError(problemText(e, 'The upload failed. Try again.')); }
  };

  const ready = !!file?.info && title.trim().length > 0 && !busy;
  const create = async () => {
    if (!ready || !file?.info) return;
    setBusy(true); setError(null);
    try {
      const e = await api<Envelope>('/api/v1/envelopes', {
        method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() },
        body: { title: title.trim(), message: message.trim() || null, file_id: file.info.id, order, mode: strong ? 'strong' : 'simple', expires_in_days: Number(expiry) },
      });
      navigate(`/documents/${e.id}/edit`, { replace: true });
    } catch (e) { setError(problemText(e, 'Couldn’t save the draft. Try again.')); setBusy(false); }
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>Send for signature</h1>
          <p>Start with the document. Next you choose who signs and place where each person signs.</p>
        </div>
      </div>
      {error && <Notice tone="danger" title={error} />}
      <Form className="sign-flow" onSubmit={(e) => { e.preventDefault(); void create(); }}>
        <section className="sign-step" aria-labelledby="step-doc">
          <h2 id="step-doc" className="sign-step-title"><span className="sign-step-no" aria-hidden="true">1</span> Document</h2>
          <div className="stack">
            <div className="field">
              <span className="field-label" id="pdf-label">PDF document</span>
              <input ref={input} id="pdf-input" type="file" accept="application/pdf,.pdf" className="visually-hidden" aria-labelledby="pdf-label" onChange={(e) => void pick(e.target.files?.[0])} />
              {file ? (
                <div className="sign-file">
                  <span className="row-icon"><FileText aria-hidden="true" size="1rem" /></span>
                  <div className="row-main">
                    <p className="row-title">{file.name}</p>
                    <p className="row-sub">{formatBytes(file.size)} · {file.info ? 'Uploaded and scanned' : `Uploading… ${Math.round(file.progress * 100)}%`}</p>
                    {!file.info && <div className="upload-progress" role="progressbar" aria-label={`Uploading ${file.name}`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(file.progress * 100)}><span style={{ width: `${Math.round(file.progress * 100)}%` }} /></div>}
                  </div>
                  {file.info && <Button variant="ghost" className="btn-sm" aria-label={`Remove ${file.name}`} onPress={() => setFile(null)}><X aria-hidden="true" size="0.875rem" /> Remove</Button>}
                </div>
              ) : (
                <div className="sign-drop">
                  <Upload aria-hidden="true" size="1.25rem" />
                  <p>A PDF up to 500 pages. Signers see exactly this file; it can’t change once sent.</p>
                  <Button variant="secondary" onPress={() => input.current?.click()}>Choose a PDF</Button>
                </div>
              )}
              {fileError && <p className="field-error" role="alert">{fileError}</p>}
            </div>
            <TextField label="Title" isRequired value={title} maxLength={200} onChange={(v) => { setTitle(v); setTitleTouched(true); }} description="Signers see this in their email and in My work." />
            <AriaTextField className="field field-wide" value={message} onChange={setMessage} maxLength={2000}>
              <Label className="field-label">Message <span className="muted">(optional)</span></Label>
              <TextArea className="field-input sign-textarea" rows={3} />
              <Text slot="description" className="field-description">Why you’re asking, in a sentence or two.</Text>
            </AriaTextField>
          </div>
        </section>

        <section className="sign-step" aria-labelledby="step-how">
          <h2 id="step-how" className="sign-step-title"><span className="sign-step-no" aria-hidden="true">2</span> How it’s signed</h2>
          <div className="stack">
            <RadioCards label="Signing order" options={ORDER} value={order} onChange={setOrder} />
            <Checkbox isSelected={strong} onChange={setStrong}>Ask signers to confirm it’s them</Checkbox>
            <p className="muted sign-hint">Each signer signs in again just before signing (password and two-step, or a passkey). Use it for contracts and regulated records.</p>
            <Select label="Expires after" options={EXPIRY} selectedKey={expiry} onSelectionChange={(k) => k && setExpiry(String(k))} description="Anyone who hasn’t signed by then can’t any more." />
          </div>
        </section>

        <div className="sign-actions">
          <Button variant="secondary" onPress={() => navigate('/documents')}>Cancel</Button>
          <Button type="submit" isDisabled={!ready}>{busy ? 'Saving…' : 'Continue to recipients'}</Button>
        </div>
      </Form>
    </>
  );
}
