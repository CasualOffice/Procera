import { KeyRound } from 'lucide-react';
import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api, setAccessToken } from '../lib/api';
import { PasskeyCancelled, getPasskey, passkeysSupported } from '../lib/passkey';
import { useSession } from '../lib/session';
import { Button } from './components/Button';
import { FormDialog } from './components/FormDialog';
import { Notice } from './components/Notice';
import { TextField } from './components/TextField';

/**
 * Re-authentication for sensitive actions (docs/12 §11). Calls `onDone` after a fresh token is set.
 * With MFA: a passkey and/or an authenticator code, whichever the account has. Without: the password.
 */
export function StepUpDialog({ isOpen, onClose, onDone }: { isOpen: boolean; onClose: () => void; onDone: () => void }) {
  const { user } = useSession();
  const [value, setValue] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const mfa = user?.mfa_enabled;
  const methods = user?.mfa_methods ?? [];
  const passkey = mfa && methods.includes('webauthn') && passkeysSupported();
  const code = mfa ? methods.includes('totp') : false;
  const field = !mfa || code;

  const finish = (token: string) => { setAccessToken(token); setValue(''); setError(null); onClose(); onDone(); };
  const submit = async () => {
    setError(null);
    try {
      const r = await api<{ access_token: string }>('/api/v1/session/step-up', { method: 'POST', body: mfa ? { code: value.trim() } : { password: value } });
      finish(r.access_token);
    } catch (e) {
      setError(e instanceof ApiError && (e.code === 'invalid_code' || e.code === 'invalid_credentials') ? 'That didn’t work. Try again.' : e instanceof Error ? e.message : 'Could not verify.');
    }
  };
  const usePasskey = async () => {
    setError(null); setBusy(true);
    try {
      const options = await api<{ public_key: Record<string, unknown> }>('/api/v1/session/step-up/webauthn/options', { method: 'POST' });
      const credential = await getPasskey(options);
      const r = await api<{ access_token: string }>('/api/v1/session/step-up', { method: 'POST', body: { credential } });
      finish(r.access_token);
    } catch (e) {
      setError(e instanceof PasskeyCancelled ? 'The passkey request was cancelled or timed out.' : e instanceof ApiError && e.code === 'passkey_rejected' ? 'That passkey didn’t work. Try again.' : e instanceof Error ? e.message : 'Could not verify.');
    } finally { setBusy(false); }
  };

  const description = !mfa
    ? 'This action needs a recent verification. Enter your password.'
    : passkey && code
      ? 'This action needs a recent verification. Use your passkey, or enter a code from your authenticator app.'
      : passkey
        ? 'This action needs a recent verification. Use your passkey.'
        : 'This action needs a recent verification. Enter a code from your authenticator app.';
  return (
    <FormDialog isOpen={isOpen} onOpenChange={(o) => !o && onClose()} title="Confirm it’s you" width={420} description={description}
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button>{field && <Button variant={passkey ? 'secondary' : 'primary'} onPress={() => void submit()} isDisabled={!value}>Verify</Button>}</>}>
      {error && <Notice tone="danger" title={error} />}
      {passkey && (
        <Button className="btn-block" isDisabled={busy} autoFocus onPress={() => void usePasskey()}>
          <KeyRound aria-hidden="true" size="1rem" /> {busy ? 'Waiting for your passkey…' : 'Use a passkey'}
        </Button>
      )}
      {passkey && field && <p className="auth-or" aria-hidden="true"><span>or</span></p>}
      {field && (
        <Form onSubmit={(e) => { e.preventDefault(); void submit(); }}>
          <TextField label={mfa ? 'Authentication code' : 'Password'} type={mfa ? 'text' : 'password'} value={value} onChange={setValue} autoFocus={!passkey} autoComplete={mfa ? 'one-time-code' : 'current-password'} />
        </Form>
      )}
    </FormDialog>
  );
}

/**
 * Runs `fn`; if the server answers `step_up_required`, asks the person to confirm it's them and
 * runs it again. Other errors go to `onError`. Render `dialog` once in the component.
 */
export function useStepUp(onError: (e: unknown) => void): { guard: (fn: () => Promise<void>) => Promise<void>; dialog: React.ReactNode } {
  const [pending, setPending] = useState<null | (() => Promise<void>)>(null);
  const guard = async (fn: () => Promise<void>) => {
    try {
      await fn();
    } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setPending(() => fn);
      else onError(e);
    }
  };
  const retry = () => {
    const fn = pending;
    setPending(null);
    if (fn) fn().catch(onError);
  };
  const dialog = <StepUpDialog isOpen={pending !== null} onClose={() => setPending(null)} onDone={retry} />;
  return { guard, dialog };
}
