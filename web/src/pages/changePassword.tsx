import { LockKeyhole } from 'lucide-react';
import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api, setAccessToken } from '../lib/api';
import { PASSWORD_HINT, passwordProblem } from '../lib/password';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';

/**
 * Account › Security › Password (docs/06 §3.6.1): change the password while signed in. The current
 * password is checked (wrong attempts count toward the sign-in lockout); every other session ends,
 * this one continues with the access token the server returns.
 */
export function PasswordSection({ onChanged }: { onChanged: (message: string) => void }) {
  const { user } = useSession();
  const [open, setOpen] = useState(false);
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [confirm, setConfirm] = useState('');
  const [tried, setTried] = useState(false);
  const [wrong, setWrong] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const problems = {
    current: !current ? 'Enter your current password.' : wrong ? 'That isn’t your current password.' : null,
    next: passwordProblem(next, user?.email ?? ''),
    confirm: confirm !== next ? 'The passwords don’t match.' : null,
  };
  const show = (k: keyof typeof problems) => (tried || (k === 'current' && wrong) || (k === 'confirm' && confirm.length > 0) ? problems[k] : null);

  const reset = () => { setCurrent(''); setNext(''); setConfirm(''); setTried(false); setWrong(false); setError(null); };
  const close = () => { if (!busy) { setOpen(false); reset(); } };

  const submit = async () => {
    setTried(true);
    setError(null);
    if (problems.current || problems.next || problems.confirm || busy) return;
    setBusy(true);
    try {
      const r = await api<{ access_token: string }>('/api/v1/me/password', { method: 'POST', body: { current_password: current, new_password: next } });
      setAccessToken(r.access_token);
      setOpen(false);
      reset();
      onChanged('Your password is changed. You were signed out on your other devices.');
    } catch (e) {
      if (e instanceof ApiError && e.code === 'wrong_password') setWrong(true);
      else setError(e instanceof ApiError && e.code === 'password_policy'
        ? `Choose another password: ${e.problem.detail ?? 'it doesn’t meet the policy'}.`
        : e instanceof ApiError && e.code === 'rate_limited' ? 'Too many wrong attempts. Wait a few minutes and try again.'
          : 'The password wasn’t changed. Try again.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="account-section" aria-labelledby="sec-password">
      <div className="section-head"><h2 id="sec-password">Password</h2></div>
      <ul className="card rows">
        <li>
          <span className="row-icon"><LockKeyhole aria-hidden="true" size="1rem" /></span>
          <div className="row-main">
            <p className="row-title">Password</p>
            <p className="row-sub">Changing it signs you out on your other devices. Two-step verification stays as it is.</p>
          </div>
          <Button variant="secondary" className="btn-sm" onPress={() => setOpen(true)}>Change password</Button>
        </li>
      </ul>
      <FormDialog isOpen={open} onOpenChange={(o) => (o ? setOpen(true) : close())} title="Change password" width={440}
        description="Enter your current password, then the new one twice."
        footer={<><Button variant="secondary" onPress={close} isDisabled={busy}>Cancel</Button><Button onPress={() => void submit()} isDisabled={busy}>{busy ? 'Saving…' : 'Update password'}</Button></>}>
        {error && <Notice tone="danger" title={error} />}
        <Form className="stack" validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
          <TextField label="Current password" type="password" value={current} onChange={(v) => { setCurrent(v); setWrong(false); }} isRequired autoComplete="current-password" autoFocus
            isInvalid={Boolean(show('current'))} errorMessage={show('current') ?? undefined} />
          <TextField label="New password" type="password" value={next} onChange={setNext} isRequired autoComplete="new-password"
            description={PASSWORD_HINT} isInvalid={Boolean(show('next'))} errorMessage={show('next') ?? undefined} />
          <TextField label="Confirm new password" type="password" value={confirm} onChange={setConfirm} isRequired autoComplete="new-password"
            isInvalid={Boolean(show('confirm'))} errorMessage={show('confirm') ?? undefined} />
          <button type="submit" hidden tabIndex={-1} />
        </Form>
      </FormDialog>
    </section>
  );
}
