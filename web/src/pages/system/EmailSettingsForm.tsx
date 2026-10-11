import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../../lib/api';
import { toFailure } from '../../lib/data';
import { useSession } from '../../lib/session';
import type { EmailTestResult, EmailView, TlsMode } from '../../lib/system';
import { Button } from '../../ui/components/Button';
import { FormDialog } from '../../ui/components/FormDialog';
import { Notice } from '../../ui/components/Notice';
import { Select } from '../../ui/components/Select';
import { TextField } from '../../ui/components/TextField';
import { useStepUp } from '../../ui/StepUp';

const TLS: { id: TlsMode; label: string; port: number }[] = [
  { id: 'starttls_required', label: 'STARTTLS (required)', port: 587 },
  { id: 'implicit_tls', label: 'Implicit TLS', port: 465 },
  { id: 'none', label: 'None (only for a relay on this server)', port: 25 },
];
const LOCAL = ['localhost', '127.0.0.1', '::1', '[::1]'];
const isEmail = (v: string) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(v.trim()) || /^[^\s@]+@localhost$/.test(v.trim());

/**
 * The SMTP relay form (docs/20 §3 Email), shared by System › Email & platform and first-time setup.
 * The stored password is never shown: leaving the field blank keeps it. Saving needs a recent
 * verification (step-up) and a reason; `fixedReason` supplies one where asking would be noise.
 */
export function EmailSettingsForm({ view, onSaved, fixedReason, submitLabel = 'Save email settings', extra }: {
  view: EmailView;
  onSaved: (v: EmailView) => void;
  fixedReason?: string;
  submitLabel?: string;
  extra?: React.ReactNode;
}) {
  const c = view.config;
  const [host, setHost] = useState(c?.host ?? '');
  const [tls, setTls] = useState<TlsMode>(c?.tls_mode ?? 'starttls_required');
  const [port, setPort] = useState(String(c?.port ?? 587));
  const [portEdited, setPortEdited] = useState(Boolean(c));
  const [username, setUsername] = useState(c?.username ?? '');
  const [password, setPassword] = useState('');
  const [fromAddress, setFromAddress] = useState(c?.from_address ?? '');
  const [fromName, setFromName] = useState(c?.from_name ?? 'Procera');
  const [replyTo, setReplyTo] = useState(c?.reply_to ?? '');
  const [reason, setReason] = useState('');
  const [tried, setTried] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { guard, dialog } = useStepUp((e) => setError(toFailure(e).message));

  const n = Number(port);
  const problems = {
    host: !host.trim() ? 'Enter the mail server’s host name.' : /[\s/@]/.test(host.trim()) ? 'Enter a host name only, such as smtp.example.com.' : undefined,
    port: !Number.isInteger(n) || n < 1 || n > 65535 ? 'Enter a port between 1 and 65535.' : undefined,
    tls: tls === 'none' && !LOCAL.includes(host.trim().toLowerCase()) ? 'Unencrypted delivery is allowed only to a relay on this server (localhost).' : undefined,
    password: password && !username.trim() ? 'Enter the username that goes with this password.' : undefined,
    from: !isEmail(fromAddress) ? 'Enter the address emails are sent from.' : undefined,
    replyTo: replyTo.trim() && !isEmail(replyTo) ? 'Enter a valid email address, or leave it empty.' : undefined,
    reason: !fixedReason && !reason.trim() ? 'Say why you’re changing the email settings.' : undefined,
  };
  const show = (k: keyof typeof problems) => (tried ? problems[k] : undefined);

  const submit = () => {
    setTried(true);
    setError(null);
    if (Object.values(problems).some(Boolean) || busy) return;
    setBusy(true);
    const body = {
      host: host.trim(), port: n, tls_mode: tls, username: username.trim() || null, password: password || null,
      from_address: fromAddress.trim(), from_name: fromName.trim() || null, reply_to: replyTo.trim() || null,
      reason: fixedReason ?? reason.trim(),
    };
    void guard(async () => {
      try {
        const v = await api<EmailView>('/sysapi/v1/email', { method: 'PUT', headers: { 'if-match': `"${c?.revision ?? 0}"` }, body });
        setPassword('');
        setReason('');
        setTried(false);
        onSaved(v);
      } catch (e) {
        if (e instanceof ApiError && e.code === 'precondition_failed') setError('Someone else changed these settings. Reload the page to see the latest, then save again.');
        else throw e;
      }
    }).finally(() => setBusy(false));
  };

  return (
    <>
      {error && <Notice tone="danger" title="Couldn’t save the email settings">{error}</Notice>}
      <Form className="email-form" validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <div className="email-grid">
          <TextField label="SMTP host" value={host} onChange={setHost} isRequired autoComplete="off" spellCheck="false"
            description="Your mail provider’s server, such as smtp.example.com." isInvalid={Boolean(show('host'))} errorMessage={show('host')} />
          <TextField label="Port" value={port} onChange={(v) => { setPort(v); setPortEdited(true); }} isRequired inputMode="numeric"
            isInvalid={Boolean(show('port'))} errorMessage={show('port')} />
        </div>
        <Select label="Encryption" options={TLS} selectedKey={tls}
          onSelectionChange={(k) => { const t = TLS.find((x) => x.id === k); if (!t) return; setTls(t.id); if (!portEdited) setPort(String(t.port)); }}
          description={tls === 'starttls_required' ? 'Delivery stops if the server doesn’t offer encryption. Usually port 587.' : tls === 'implicit_tls' ? 'Encrypted from the first byte. Usually port 465.' : 'No encryption. Use only for a mail relay running on this server.'} />
        {show('tls') && <p className="field-error" role="alert">{show('tls')}</p>}
        <div className="email-grid">
          <TextField label="Username" value={username} onChange={setUsername} autoComplete="off" spellCheck="false"
            description="Leave empty if the server doesn’t ask for a sign-in." />
          <TextField label="Password" type="password" value={password} onChange={setPassword} autoComplete="new-password"
            description={c?.password_set ? 'Leave blank to keep the current password.' : 'Stored encrypted. It’s never shown again.'}
            isInvalid={Boolean(show('password'))} errorMessage={show('password')} />
        </div>
        <div className="email-grid">
          <TextField label="From address" type="email" value={fromAddress} onChange={setFromAddress} isRequired autoComplete="off"
            description="Must be an address your mail server is allowed to send from." isInvalid={Boolean(show('from'))} errorMessage={show('from')} />
          <TextField label="Sender name" value={fromName} onChange={setFromName} autoComplete="off"
            description="Shown as the sender. Workspaces can set their own." />
        </div>
        <TextField label="Reply-to address" type="email" value={replyTo} onChange={setReplyTo} autoComplete="off"
          description="Optional. Where replies go instead of the from address." isInvalid={Boolean(show('replyTo'))} errorMessage={show('replyTo')} />
        {!fixedReason && (
          <TextField label="Reason for the change" value={reason} onChange={setReason} isRequired autoComplete="off"
            description="Recorded in the system audit log." isInvalid={Boolean(show('reason'))} errorMessage={show('reason')} />
        )}
        <div className="actions">
          <Button type="submit" isDisabled={busy}>{busy ? 'Saving…' : submitLabel}</Button>
          {extra}
        </div>
      </Form>
      {dialog}
    </>
  );
}

/** Sends one test message now through the transport in use and shows a plain-language result. */
export function SendTestDialog({ onClose }: { onClose: () => void }) {
  const { user } = useSession();
  const [to, setTo] = useState(user?.email ?? '');
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const invalid = !isEmail(to);
  const send = async () => {
    if (invalid || busy) return;
    setBusy(true);
    setResult(null);
    try {
      const r = await api<EmailTestResult>('/sysapi/v1/email/test', { method: 'POST', body: { to: to.trim() } });
      setResult(r.sent ? { ok: true, text: `Test email sent to ${to.trim()}. Check that inbox, including spam.` } : { ok: false, text: r.error?.message ?? 'The test email wasn’t sent.' });
    } catch (e) {
      setResult({ ok: false, text: toFailure(e).message });
    } finally {
      setBusy(false);
    }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Send a test email" width={460}
      description="Sends one message now with the saved settings, so you can check delivery end to end."
      footer={<><Button variant="secondary" onPress={onClose}>Close</Button><Button onPress={() => void send()} isDisabled={busy || invalid}>{busy ? 'Sending…' : 'Send test email'}</Button></>}>
      {result && (result.ok ? <Notice tone="success" title={result.text} /> : <Notice tone="danger" title="The test email wasn’t sent">{result.text}</Notice>)}
      <Form className="quota-form" onSubmit={(e) => { e.preventDefault(); void send(); }}>
        <TextField label="Send to" type="email" value={to} onChange={setTo} isRequired autoFocus autoComplete="email"
          isInvalid={Boolean(to) && invalid} errorMessage={to && invalid ? 'Enter a valid email address.' : undefined} />
      </Form>
    </FormDialog>
  );
}
