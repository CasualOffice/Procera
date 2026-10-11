import { KeyRound, LockKeyhole, LogOut, MailCheck } from 'lucide-react';
import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { useStepUp } from '../ui/StepUp';

type Action = 'reset-link' | 'require-password-change' | 'sign-out-everywhere' | 'reset-mfa';

/** What each control is called, what it does, and what to say when it's done (docs/06 §3.6.1). */
const ACTIONS: Record<Action, { label: string; icon: typeof KeyRound; sub: string; title: (n: string) => string; body: (n: string) => string; done: (n: string) => string; danger?: boolean }> = {
  'reset-link': {
    label: 'Send reset link', icon: MailCheck,
    sub: 'Email a link to choose a new password. You never see or set it.',
    title: (n) => `Send ${n} a password reset link?`,
    body: (n) => `${n} gets an email with a link that works once for 30 minutes. Their two-step verification stays on.`,
    done: (n) => `A reset link is on its way to ${n}.`,
  },
  'require-password-change': {
    label: 'Require password change', icon: LockKeyhole,
    sub: 'They must choose a new password the next time they sign in.',
    title: (n) => `Require ${n} to change their password?`,
    body: (n) => `The next time ${n} signs in, they choose a new password before anything else. They are told by email.`,
    done: (n) => `${n} will choose a new password at their next sign-in.`,
  },
  'sign-out-everywhere': {
    label: 'Sign out everywhere', icon: LogOut,
    sub: 'End every session on every device at once.',
    title: (n) => `Sign ${n} out everywhere?`,
    body: (n) => `${n} is signed out on every device right away and has to sign in again. They are told by email.`,
    done: (n) => `${n} is signed out on every device.`,
    danger: true,
  },
  'reset-mfa': {
    label: 'Reset two-step verification', icon: KeyRound,
    sub: 'For a lost phone or security key: removes their authenticator app, passkeys and recovery codes.',
    title: (n) => `Reset ${n}’s two-step verification?`,
    body: (n) => `${n}’s authenticator app, passkeys and recovery codes stop working. They set up two-step verification again at their next sign-in where their role requires it. Their password doesn’t change.`,
    done: (n) => `${n}’s two-step verification is reset.`,
    danger: true,
  },
};
const ORDER: Action[] = ['reset-link', 'require-password-change', 'sign-out-everywhere', 'reset-mfa'];

/**
 * Person › Account security, for holders of `member:security.manage`. Each action asks for a reason
 * and a recent verification, and is recorded in the audit log. Accounts are global: all but the reset
 * link work only when this is the person's only workspace; otherwise a system administrator does it.
 */
export function AccountSecurity({ membershipId, name }: { membershipId: string; name: string }) {
  const [action, setAction] = useState<Action | null>(null);
  const [reason, setReason] = useState('');
  const [tried, setTried] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ tone: 'success' | 'warning' | 'danger'; title: string; body?: string } | null>(null);
  const onError = (e: unknown) => {
    setBusy(false);
    if (e instanceof ApiError && e.code === 'other_workspaces') {
      setNotice({ tone: 'warning', title: 'A system administrator must do this', body: `${name} also uses this account outside this workspace (another workspace or the system console), so only a system administrator can change their password state, sessions or two-step verification. You can still send a reset link.` });
    } else if (e instanceof ApiError && e.code === 'member_inactive') {
      setNotice({ tone: 'danger', title: `${name} is deactivated in this workspace.` });
    } else {
      setNotice({ tone: 'danger', title: e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'That didn’t work. Try again.' });
    }
  };
  const { guard, dialog } = useStepUp(onError);

  const open = (a: Action) => { setAction(a); setReason(''); setTried(false); setNotice(null); };
  const confirm = () => {
    setTried(true);
    if (!action || !reason.trim() || busy) return;
    const a = action;
    const why = reason.trim();
    setAction(null);
    setBusy(true);
    void guard(async () => {
      await api(`/api/v1/members/${membershipId}/security/${a}`, { method: 'POST', body: { reason: why } });
      setBusy(false);
      setNotice({ tone: 'success', title: ACTIONS[a].done(name) });
    }).finally(() => setBusy(false));
  };
  const meta = action ? ACTIONS[action] : null;
  const missing = tried && !reason.trim();

  return (
    <section className="account-section person-security" aria-labelledby="sec-account-security">
      <div className="section-head"><h2 id="sec-account-security">Account security</h2></div>
      <p className="account-lede">Help {name} get back into their account. Each action needs a reason and a recent verification, and is recorded in the audit log.</p>
      {notice && <Notice tone={notice.tone} title={notice.title}>{notice.body}</Notice>}
      <ul className="card rows" aria-label="Account security actions">
        {ORDER.map((a) => {
          const { label, icon: Icon, sub } = ACTIONS[a];
          return (
            <li key={a}>
              <span className="row-icon"><Icon aria-hidden="true" size="1rem" /></span>
              <div className="row-main">
                <p className="row-title">{label}</p>
                <p className="row-sub">{sub}</p>
              </div>
              <Button variant="secondary" className="btn-sm" isDisabled={busy} onPress={() => open(a)}>{label}</Button>
            </li>
          );
        })}
      </ul>
      <FormDialog isOpen={meta !== null} onOpenChange={(o) => !o && setAction(null)} title={meta?.title(name) ?? ''} width={460}
        description={meta?.body(name)}
        footer={<><Button variant="secondary" onPress={() => setAction(null)}>Cancel</Button><Button variant={meta?.danger ? 'danger' : 'primary'} onPress={confirm}>{meta?.label ?? ''}</Button></>}>
        <Form validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); confirm(); }}>
          <TextField label="Reason" value={reason} onChange={setReason} isRequired maxLength={500} autoFocus
            description="For the audit log, e.g. “Lost phone, confirmed by video call”."
            isInvalid={missing} errorMessage={missing ? 'Give a reason.' : undefined} />
        </Form>
      </FormDialog>
      {dialog}
    </section>
  );
}
