import { MailCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, authPost } from '../lib/api';
import { Link } from '../lib/router';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';

/**
 * "Forgot password?" (`/forgot-password`, docs/06 §3.6.1). The answer is the same whether or not an
 * account uses the address, so the page never tells who has an account.
 */
export function ForgotPassword() {
  const [email, setEmail] = useState('');
  const [tried, setTried] = useState(false);
  const [sent, setSent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { document.title = 'Reset your password · Procera'; }, []);

  const invalid = !/^[^\s@]+@[^\s@]+$/.test(email.trim());
  const submit = async () => {
    setTried(true);
    setError(null);
    if (invalid || busy) return;
    setBusy(true);
    try {
      await authPost('/api/v1/auth/password/forgot', { email: email.trim() });
      setSent(email.trim());
    } catch (e) {
      setError(e instanceof ApiError && e.code === 'rate_limited'
        ? 'Too many requests from this network. Wait a minute and try again.'
        : 'The request didn’t go through. Check your connection and try again.');
    } finally {
      setBusy(false);
    }
  };

  if (sent) {
    return (
      <>
        <h1>Check your email</h1>
        <Notice tone="success" title="If an account uses this address, we sent it a reset link">
          We sent the link to <strong>{sent}</strong> if it belongs to a Procera account. It works once and expires in 30 minutes. Didn’t get it? Check your spam folder, or ask again in a few minutes.
        </Notice>
        <div className="actions">
          <Link className="btn btn-primary btn-md link-btn btn-block" to="/sign-in">Back to sign in</Link>
          <Button variant="ghost" onPress={() => { setSent(null); setTried(false); }}><MailCheck aria-hidden="true" size="1rem" /> Use a different address</Button>
        </div>
      </>
    );
  }

  return (
    <>
      <h1>Reset your password</h1>
      <p className="lede">Enter the email you sign in with. We’ll send you a link to choose a new password. Two-step verification stays on.</p>
      {error && <Notice tone="danger" title={error} />}
      <Form className="stack" validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
        <TextField label="Work email" type="email" value={email} onChange={setEmail} isRequired autoComplete="username" autoFocus
          isInvalid={tried && invalid} errorMessage={tried && invalid ? 'Enter your email address.' : undefined} />
        <div className="actions">
          <Button type="submit" className="btn-block" isDisabled={busy}>{busy ? 'Sending…' : 'Send reset link'}</Button>
        </div>
      </Form>
      <p className="auth-foot">Remembered it? <Link to="/sign-in">Sign in</Link>.</p>
    </>
  );
}
