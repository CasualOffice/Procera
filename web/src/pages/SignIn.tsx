import { KeyRound } from 'lucide-react';
import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError } from '../lib/api';
import { PasskeyCancelled, passkeysSupported } from '../lib/passkey';
import { useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';

/** Sign in (docs/06 §3): password, then a passkey, a TOTP code or a recovery code when MFA is on. */
export function SignIn() {
  const { signIn, completeMfa, completeMfaWithPasskey } = useSession();
  const { query } = useRoute();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [code, setCode] = useState('');
  const [mfaToken, setMfaToken] = useState<string | null>(null);
  const [methods, setMethods] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(
        e instanceof PasskeyCancelled
          ? 'The passkey request was cancelled or timed out. Try again when you’re ready.'
          : e instanceof ApiError && e.code === 'passkey_rejected'
            ? 'That passkey didn’t work for this account. Try again, or use another way to verify.'
            : e instanceof ApiError
          ? e.code === 'invalid_credentials'
            ? 'That email and password don’t match an account.'
            : e.code === 'invalid_code'
              ? 'That code didn’t work. Codes change every 30 seconds; recovery codes work once.'
              : e.code === 'throttled'
                ? 'Too many attempts. Wait a minute and try again.'
                : e.message
          : 'Something went wrong. Try again.',
      );
    } finally {
      setBusy(false);
    }
  };

  if (mfaToken) {
    const passkey = methods.includes('webauthn') && passkeysSupported();
    const totp = methods.includes('totp');
    const lede = passkey
      ? totp
        ? 'Use your passkey, or enter the 6-digit code from your authenticator app or a recovery code.'
        : 'Use your passkey to confirm it’s you. If you can’t, enter a recovery code.'
      : totp
        ? 'Enter the 6-digit code from your authenticator app, or a recovery code.'
        : 'Enter a recovery code.';
    return (
      <>
        <h1>Two-step verification</h1>
        <p className="lede">{lede}</p>
        {error && <Notice tone="danger" title={error} />}
        {passkey && (
          <>
            <Button className="btn-block" isDisabled={busy} autoFocus onPress={() => void run(() => completeMfaWithPasskey(mfaToken))}>
              <KeyRound aria-hidden="true" size="1rem" /> Use a passkey
            </Button>
            <p className="auth-or" aria-hidden="true"><span>or</span></p>
          </>
        )}
        <Form className="stack" onSubmit={(e) => { e.preventDefault(); void run(() => completeMfa(mfaToken, code)); }}>
          <TextField label={totp ? 'Code' : 'Recovery code'} value={code} onChange={setCode} isRequired autoComplete="one-time-code" autoFocus={!passkey} />
          <div className="actions">
            <Button type="submit" variant={passkey ? 'secondary' : 'primary'} className="btn-block" isDisabled={busy}>Verify</Button>
            <Button variant="ghost" onPress={() => { setMfaToken(null); setMethods([]); setCode(''); setError(null); }}>Use a different account</Button>
          </div>
        </Form>
      </>
    );
  }

  return (
    <>
      <h1>Sign in to Procera</h1>
      <p className="lede">Welcome back. Sign in with your work account.</p>
      {query.get('mfa') === 'on' && !error && <Notice tone="success" title="Two-step verification is on">Sign in again. You’ll be asked for a code from your app.</Notice>}
      {error && <Notice tone="danger" title={error} />}
      <Form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            const r = await signIn(email, password);
            if (r.mfaToken) { setMethods(r.methods ?? []); setMfaToken(r.mfaToken); }
          });
        }}
      >
        <TextField label="Work email" type="email" value={email} onChange={setEmail} isRequired autoComplete="username" autoFocus />
        <TextField label="Password" type="password" value={password} onChange={setPassword} isRequired autoComplete="current-password" />
        <div className="actions">
          <Button type="submit" className="btn-block" isDisabled={busy}>{busy ? 'Signing in…' : 'Sign in'}</Button>
        </div>
      </Form>
      <p className="auth-foot">Protected by two-step verification. Lost access? Ask your workspace admin.</p>
    </>
  );
}
