import { Check, Copy, Download, ShieldCheck } from 'lucide-react';
import QRCode from 'qrcode';
import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';

type Step = 'intro' | 'scan' | 'codes';

/** Guided TOTP enrolment (docs/06 §3), shown when a workspace requires MFA for the member's role. */
export function MfaSetup({ workspaceName }: { workspaceName: string }) {
  const { signOut } = useSession();
  const [step, setStep] = useState<Step>('intro');
  const [secret, setSecret] = useState('');
  const [qr, setQr] = useState('');
  const [code, setCode] = useState('');
  const [codes, setCodes] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const begin = async () => {
    setError(null);
    try {
      const r = await api<{ secret: string; otpauth_uri: string }>('/api/v1/me/mfa/totp', { method: 'POST' });
      setSecret(r.secret);
      setQr(await QRCode.toDataURL(r.otpauth_uri, { margin: 1, width: 360, color: { dark: '#2F2E2A', light: '#FFFFFF' } }));
      setStep('scan');
    } catch (e) { setError(e instanceof Error ? e.message : 'Could not start setup.'); }
  };
  const confirm = async () => {
    setError(null);
    try {
      const r = await api<{ recovery_codes: string[] }>('/api/v1/me/mfa/totp/confirm', { method: 'POST', body: { code: code.trim() } });
      setCodes(r.recovery_codes);
      setStep('codes');
    } catch (e) { setError(e instanceof ApiError && e.code === 'invalid_code' ? 'That code didn’t match. Codes change every 30 seconds — try the current one.' : e instanceof Error ? e.message : 'Could not confirm.'); }
  };
  const download = () => {
    const blob = new Blob([`Procera recovery codes\n\n${codes.join('\n')}\n\nEach code works once.\n`], { type: 'text/plain' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'procera-recovery-codes.txt';
    a.click();
    URL.revokeObjectURL(a.href);
  };

  if (step === 'intro') {
    return (
      <>
        <span className="mfa-badge"><ShieldCheck aria-hidden="true" size={20} /></span>
        <h1>Turn on two-step verification</h1>
        <p className="lede">{workspaceName} asks people in your role to confirm sign-ins with a code from an authenticator app. It takes about a minute.</p>
        {error && <Notice tone="danger" title={error} />}
        <ol className="mfa-steps">
          <li>Scan a QR code with an app such as 1Password, Google Authenticator or Microsoft Authenticator.</li>
          <li>Enter the 6-digit code it shows.</li>
          <li>Save your recovery codes somewhere safe.</li>
        </ol>
        <div className="actions"><Button className="btn-block" onPress={() => void begin()}>Get started</Button></div>
      </>
    );
  }
  if (step === 'scan') {
    return (
      <>
        <h1>Scan this code</h1>
        <p className="lede">Open your authenticator app, add an account and scan the code. Then enter the 6-digit code it shows.</p>
        {error && <Notice tone="danger" title={error} />}
        <div className="mfa-qr"><img src={qr} alt="QR code for your authenticator app" width={180} height={180} /></div>
        <div className="mfa-secret">
          <span className="muted">Can’t scan? Enter this key:</span>
          <code>{secret.match(/.{1,4}/g)?.join(' ')}</code>
          <button type="button" className="icon-btn" aria-label={copied ? 'Key copied' : 'Copy key'} onClick={() => { void navigator.clipboard?.writeText(secret); setCopied(true); }}>
            {copied ? <Check aria-hidden="true" size={15} /> : <Copy aria-hidden="true" size={15} />}
          </button>
        </div>
        <Form className="stack" onSubmit={(e) => { e.preventDefault(); void confirm(); }}>
          <TextField label="6-digit code" value={code} onChange={setCode} autoComplete="one-time-code" inputMode="numeric" isRequired autoFocus />
          <div className="actions"><Button type="submit" className="btn-block" isDisabled={code.trim().length < 6}>Verify and turn on</Button></div>
        </Form>
      </>
    );
  }
  return (
    <>
      <span className="mfa-badge mfa-badge-done"><Check aria-hidden="true" size={20} /></span>
      <h1>Save your recovery codes</h1>
      <p className="lede">If you lose your phone, each of these codes lets you sign in once. Keep them somewhere safe — you won’t see them again.</p>
      <ul className="recovery" aria-label="Recovery codes">{codes.map((c) => <li key={c}><code>{c}</code></li>)}</ul>
      <div className="actions" style={{ marginBottom: 12 }}>
        <Button variant="secondary" onPress={download}><Download aria-hidden="true" size={15} /> Download</Button>
        <Button variant="secondary" onPress={() => void navigator.clipboard?.writeText(codes.join('\n'))}><Copy aria-hidden="true" size={15} /> Copy</Button>
      </div>
      <Button className="btn-block" onPress={async () => { await signOut(); navigate('/sign-in?mfa=on', { replace: true }); }}>I’ve saved them — sign in again</Button>
    </>
  );
}
