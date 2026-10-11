import { useEffect, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { passwordProblem } from '../lib/password';
import { Link, navigate, useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';

const RESTART = 'Restart Procera to print a new one-time token to its log. A new token is issued only until setup is finished.';

type Outcome = null | 'already' | 'signin';

/**
 * First-run setup (`/setup`, FR-S01, E2E-R1-01): the one-time bootstrap token from the server log
 * creates the first system owner. There are no default credentials. Next, the owner turns on
 * two-step verification, which the system console requires (docs/06 §3.6), and opens the console.
 */
export function Setup() {
  const { query } = useRoute();
  const { signIn } = useSession();
  const [token, setToken] = useState(() => query.get('token') ?? '');
  const [email, setEmail] = useState('');
  const [name, setName] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [tried, setTried] = useState(false);
  const [tokenError, setTokenError] = useState<string | null>(null);
  const [error, setError] = useState<{ title: string; body?: string } | null>(null);
  const [outcome, setOutcome] = useState<Outcome>(null);
  const [busy, setBusy] = useState(false);

  // Keep the token out of the address bar and history once it is in the form.
  // (History API directly: `navigate` would move focus away from the form.)
  useEffect(() => { if (window.location.search.includes('token=')) window.history.replaceState(null, '', '/setup'); }, []);
  useEffect(() => { document.title = 'Set up · Procera'; }, []);

  const problems = {
    token: !token.trim() ? 'Enter the setup token.' : null,
    email: !/^[^\s@]+@[^\s@]+$/.test(email.trim()) ? 'Enter your email address.' : null,
    name: !name.trim() ? 'Enter your name.' : null,
    password: passwordProblem(password, email),
    confirm: confirm !== password ? 'The passwords don’t match.' : null,
  };
  const show = (k: keyof typeof problems) => (tried || (k === 'confirm' && confirm.length > 0) ? problems[k] : null);

  const submit = async () => {
    setTried(true);
    setError(null);
    setTokenError(null);
    if (Object.values(problems).some(Boolean) || busy) return;
    setBusy(true);
    try {
      await api('/sysapi/v1/bootstrap', { method: 'POST', body: { token: token.trim(), email: email.trim(), display_name: name.trim(), password } });
    } catch (e) {
      setBusy(false);
      if (e instanceof ApiError && e.code === 'already_bootstrapped') { setOutcome('already'); return; }
      if (e instanceof ApiError && e.code === 'bootstrap_invalid') {
        setTokenError('This token is invalid, expired or already used.');
        setError({ title: 'This setup token doesn’t work', body: `Each token works once and expires after 24 hours. ${RESTART}` });
        return;
      }
      // The token is spent before the account is created (single use), so a rejected account needs a new token.
      if (e instanceof ApiError && (e.code === 'password_policy' || e.code === 'email_taken')) {
        setError({ title: e.code === 'email_taken' ? 'An account with this email already exists' : `The password was rejected: ${e.problem.detail ?? 'it doesn’t meet the policy'}`, body: `The setup token is now used. ${RESTART}` });
        return;
      }
      setError({ title: e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'The server could not be reached. Check your connection and try again.' });
      return;
    }
    // Signed in, the system console asks for two-step verification before anything else.
    try {
      const r = await signIn(email.trim(), password);
      navigate(r.mfaToken ? '/sign-in?next=/system' : '/system', { replace: true });
    } catch {
      setBusy(false);
      setOutcome('signin');
    }
  };

  if (outcome === 'already') {
    return (
      <>
        <h1>Procera is already set up</h1>
        <p className="lede">A system owner already exists, so first-run setup is closed. Sign in with your account, or ask your system owner for access.</p>
        <div className="actions"><Link className="btn btn-primary btn-md link-btn" to="/sign-in">Go to sign in</Link></div>
      </>
    );
  }
  if (outcome === 'signin') {
    return (
      <>
        <h1>Your system owner account is ready</h1>
        <Notice tone="success" title="Setup is complete">Sign in with the email and password you just chose. You’ll then turn on two-step verification.</Notice>
        <div className="actions"><Link className="btn btn-primary btn-md link-btn" to="/sign-in?next=/system">Sign in</Link></div>
      </>
    );
  }

  return (
    <>
      <h1>Set up Procera</h1>
      <p className="lede">Create the system owner account. It manages workspaces on this platform, not their content.</p>
      <ol className="mfa-steps" aria-label="Setup steps">
        <li>Create the system owner with the one-time setup token.</li>
        <li>Turn on two-step verification with an authenticator app.</li>
        <li>Open the system console and create the first workspace.</li>
      </ol>
      {error && <Notice tone="danger" title={error.title}>{error.body}</Notice>}
      <Form className="stack" validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
        <TextField label="Setup token" value={token} onChange={(v) => { setToken(v); setTokenError(null); }} isRequired autoComplete="off" spellCheck="false" autoFocus={!token}
          description="Printed in the server log the first time Procera starts. Valid for 24 hours."
          isInvalid={Boolean(tokenError ?? show('token'))} errorMessage={tokenError ?? show('token') ?? undefined} />
        <TextField label="Work email" type="email" value={email} onChange={setEmail} isRequired autoComplete="username" autoFocus={Boolean(token)}
          isInvalid={Boolean(show('email'))} errorMessage={show('email') ?? undefined} />
        <TextField label="Your name" value={name} onChange={setName} isRequired autoComplete="name"
          isInvalid={Boolean(show('name'))} errorMessage={show('name') ?? undefined} />
        <TextField label="Password" type="password" value={password} onChange={setPassword} isRequired autoComplete="new-password"
          description="At least 12 characters. A short sentence works well."
          isInvalid={Boolean(show('password'))} errorMessage={show('password') ?? undefined} />
        <TextField label="Confirm password" type="password" value={confirm} onChange={setConfirm} isRequired autoComplete="new-password"
          isInvalid={Boolean(show('confirm'))} errorMessage={show('confirm') ?? undefined} />
        <div className="actions">
          <Button type="submit" className="btn-block" isDisabled={busy}>{busy ? 'Setting up…' : 'Create system owner'}</Button>
        </div>
      </Form>
      <p className="auth-foot">Already set up? <Link to="/sign-in">Sign in</Link>.</p>
    </>
  );
}
