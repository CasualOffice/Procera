import { useEffect, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api, authPost } from '../lib/api';
import { PASSWORD_HINT, passwordProblem } from '../lib/password';
import { Link, navigate, useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { Skeleton } from '../ui/States';

type State = 'checking' | 'ready' | 'invalid' | 'offline';

/**
 * The reset link from the email (`/reset-password?token=…`, docs/06 §3.6.1). The token leaves the
 * address bar as soon as it is read. Setting the password signs the account out everywhere; the
 * person then signs in with the new password and their usual second step.
 */
export function ResetPassword() {
  const { query } = useRoute();
  const { status, signOut } = useSession();
  const [token] = useState(() => query.get('token') ?? '');
  const [state, setState] = useState<State>(token ? 'checking' : 'invalid');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [tried, setTried] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Keep the token out of the address bar and history (History API: `navigate` would move focus).
  useEffect(() => { if (window.location.search.includes('token=')) window.history.replaceState(null, '', '/reset-password'); }, []);
  useEffect(() => { document.title = 'Choose a new password · Procera'; }, []);
  useEffect(() => {
    if (!token) return;
    let live = true;
    api<{ valid: boolean }>(`/api/v1/auth/password/reset/check?token=${encodeURIComponent(token)}`)
      .then((r) => { if (live) setState(r.valid ? 'ready' : 'invalid'); })
      .catch(() => { if (live) setState('offline'); });
    return () => { live = false; };
  }, [token]);

  const problems = { password: passwordProblem(password), confirm: confirm !== password ? 'The passwords don’t match.' : null };
  const show = (k: keyof typeof problems) => (tried || (k === 'confirm' && confirm.length > 0) ? problems[k] : null);

  const submit = async () => {
    setTried(true);
    setError(null);
    if (problems.password || problems.confirm || busy) return;
    setBusy(true);
    try {
      await authPost('/api/v1/auth/password/reset', { token, password });
    } catch (e) {
      setBusy(false);
      if (e instanceof ApiError && e.code === 'reset_link_invalid') { setState('invalid'); return; }
      setError(e instanceof ApiError && e.code === 'password_policy'
        ? `Choose another password: ${e.problem.detail ?? 'it doesn’t meet the policy'}.`
        : e instanceof ApiError && e.code === 'rate_limited' ? 'Too many attempts. Wait a minute and try again.'
          : 'The new password wasn’t saved. Check your connection and try again.');
      return;
    }
    // Every session ended with the reset, including one in this browser.
    if (status === 'ready' || status === 'choose_workspace') await signOut();
    navigate('/sign-in?reset=done', { replace: true });
  };

  if (state === 'checking') return <Skeleton rows={3} label="Checking your reset link" />;
  if (state === 'invalid' || state === 'offline') {
    return (
      <>
        <h1>{state === 'invalid' ? 'This reset link doesn’t work' : 'We couldn’t check your link'}</h1>
        <Notice tone={state === 'invalid' ? 'warning' : 'danger'} title={state === 'invalid' ? 'The link is invalid, expired or already used' : 'The server could not be reached'}>
          {state === 'invalid'
            ? 'Reset links work once and expire after 30 minutes, and a newer link replaces older ones. Ask for a new one.'
            : 'Check your connection and open the link from your email again.'}
        </Notice>
        <div className="actions">
          <Link className="btn btn-primary btn-md link-btn btn-block" to="/forgot-password">Get a new link</Link>
          <Link className="btn btn-ghost btn-md link-btn" to="/sign-in">Back to sign in</Link>
        </div>
      </>
    );
  }

  return (
    <>
      <h1>Choose a new password</h1>
      <p className="lede">After you save it, you’ll be signed out everywhere. Sign in again with the new password and your usual second step.</p>
      {error && <Notice tone="danger" title={error} />}
      <Form className="stack" validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
        <TextField label="New password" type="password" value={password} onChange={setPassword} isRequired autoComplete="new-password" autoFocus
          description={PASSWORD_HINT} isInvalid={Boolean(show('password'))} errorMessage={show('password') ?? undefined} />
        <TextField label="Confirm new password" type="password" value={confirm} onChange={setConfirm} isRequired autoComplete="new-password"
          isInvalid={Boolean(show('confirm'))} errorMessage={show('confirm') ?? undefined} />
        <div className="actions">
          <Button type="submit" className="btn-block" isDisabled={busy}>{busy ? 'Saving…' : 'Set new password'}</Button>
        </div>
      </Form>
      <p className="auth-foot">Remembered it? <Link to="/sign-in">Sign in</Link>.</p>
    </>
  );
}
