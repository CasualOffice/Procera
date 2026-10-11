import { KeyRound } from 'lucide-react';
import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError } from '../lib/api';
import { PASSWORD_HINT, passwordProblem } from '../lib/password';
import { PasskeyCancelled, passkeysSupported } from '../lib/passkey';
import { Link, useRoute } from '../lib/router';
import { type SignInStep, useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';

/**
 * Sign in (docs/06 §3): password, then a passkey, a TOTP code or a recovery code when MFA is on.
 * When an administrator required a new password, the password step is followed by "Choose a new
 * password" (docs/06 §3.6.1), and then two-step verification as usual.
 */
export function SignIn() {
  const { signIn, completeMfa, completeMfaWithPasskey, completePasswordChange } = useSession();
  const { query } = useRoute();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [code, setCode] = useState('');
  const [mfaToken, setMfaToken] = useState<string | null>(null);
  const [methods, setMethods] = useState<string[]>([]);
  const [changeToken, setChangeToken] = useState<string | null>(null);
  const [newPassword, setNewPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [tried, setTried] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const step = (r: SignInStep) => {
    if (r.changeToken) { setChangeToken(r.changeToken); setTried(false); return; }
    setChangeToken(null);
    if (r.mfaToken) { setMethods(r.methods ?? []); setMfaToken(r.mfaToken); }
  };
  const restart = (message: string | null = null) => {
    setMfaToken(null); setMethods([]); setCode(''); setChangeToken(null); setNewPassword(''); setConfirm(''); setPassword(''); setError(message);
  };

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      if (e instanceof ApiError && e.code === 'change_token_invalid') { restart('That step expired. Sign in again to choose your new password.'); return; }
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
              : e.code === 'throttled' || e.code === 'rate_limited'
                ? 'Too many attempts. Wait a minute and try again.'
                : e.code === 'password_policy'
                  ? `Choose another password: ${e.problem.detail ?? 'it doesn’t meet the policy'}.`
                  : e.message
          : 'Something went wrong. Try again.',
      );
    } finally {
      setBusy(false);
    }
  };

  if (changeToken) {
    const problems = { password: passwordProblem(newPassword, email), confirm: confirm !== newPassword ? 'The passwords don’t match.' : null };
    const show = (k: keyof typeof problems) => (tried || (k === 'confirm' && confirm.length > 0) ? problems[k] : null);
    return (
      <>
        <h1>Choose a new password</h1>
        <p className="lede">Your administrator asked you to choose a new password before you continue. Two-step verification, if you use it, comes next.</p>
        {error && <Notice tone="danger" title={error} />}
        <Form className="stack" validationBehavior="aria" onSubmit={(e) => {
          e.preventDefault();
          setTried(true);
          if (problems.password || problems.confirm || busy) return;
          void run(async () => step(await completePasswordChange(changeToken, newPassword)));
        }}>
          <TextField label="New password" type="password" value={newPassword} onChange={setNewPassword} isRequired autoComplete="new-password" autoFocus
            description={PASSWORD_HINT} isInvalid={Boolean(show('password'))} errorMessage={show('password') ?? undefined} />
          <TextField label="Confirm new password" type="password" value={confirm} onChange={setConfirm} isRequired autoComplete="new-password"
            isInvalid={Boolean(show('confirm'))} errorMessage={show('confirm') ?? undefined} />
          <div className="actions">
            <Button type="submit" className="btn-block" isDisabled={busy}>{busy ? 'Saving…' : 'Save and continue'}</Button>
            <Button variant="ghost" onPress={() => restart()}>Use a different account</Button>
          </div>
        </Form>
      </>
    );
  }

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
            <Button variant="ghost" onPress={() => restart()}>Use a different account</Button>
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
      {query.get('reset') === 'done' && !error && <Notice tone="success" title="Your password is changed">Sign in with your new password. You were signed out everywhere else.</Notice>}
      {error && <Notice tone="danger" title={error} />}
      <Form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => step(await signIn(email, password)));
        }}
      >
        <TextField label="Work email" type="email" value={email} onChange={setEmail} isRequired autoComplete="username" autoFocus />
        <TextField label="Password" type="password" value={password} onChange={setPassword} isRequired autoComplete="current-password" />
        <p className="auth-forgot"><Link to="/forgot-password">Forgot password?</Link></p>
        <div className="actions">
          <Button type="submit" className="btn-block" isDisabled={busy}>{busy ? 'Signing in…' : 'Sign in'}</Button>
        </div>
      </Form>
      <p className="auth-foot">Protected by two-step verification. Lost your authenticator? Ask your workspace admin.</p>
    </>
  );
}
