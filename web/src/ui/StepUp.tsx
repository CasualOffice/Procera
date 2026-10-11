import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api, setAccessToken } from '../lib/api';
import { useSession } from '../lib/session';
import { Button } from './components/Button';
import { FormDialog } from './components/FormDialog';
import { Notice } from './components/Notice';
import { TextField } from './components/TextField';

/** Re-authentication for sensitive actions (docs/12 §11). Calls `onDone` after a fresh token is set. */
export function StepUpDialog({ isOpen, onClose, onDone }: { isOpen: boolean; onClose: () => void; onDone: () => void }) {
  const { user } = useSession();
  const [value, setValue] = useState('');
  const [error, setError] = useState<string | null>(null);
  const mfa = user?.mfa_enabled;
  const submit = async () => {
    setError(null);
    try {
      const r = await api<{ access_token: string }>('/api/v1/session/step-up', { method: 'POST', body: mfa ? { code: value.trim() } : { password: value } });
      setAccessToken(r.access_token); setValue(''); onClose(); onDone();
    } catch (e) {
      setError(e instanceof ApiError && (e.code === 'invalid_code' || e.code === 'invalid_credentials') ? 'That didn’t work. Try again.' : e instanceof Error ? e.message : 'Could not verify.');
    }
  };
  return (
    <FormDialog isOpen={isOpen} onOpenChange={(o) => !o && onClose()} title="Confirm it’s you" width={420}
      description={mfa ? 'This action needs a recent verification. Enter a code from your authenticator app.' : 'This action needs a recent verification. Enter your password.'}
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()} isDisabled={!value}>Verify</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <Form onSubmit={(e) => { e.preventDefault(); void submit(); }}>
        <TextField label={mfa ? 'Authentication code' : 'Password'} type={mfa ? 'text' : 'password'} value={value} onChange={setValue} autoFocus autoComplete={mfa ? 'one-time-code' : 'current-password'} />
      </Form>
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
