import { MailCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { shortDate } from '../lib/format';
import { Link, navigate, useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';

interface Preview { email: string; workspace: string; inviter: string | null; expires_at: string; account_exists: boolean }

/**
 * The invitation link from the email (`/invite?token=…`, BPMS-023, A-24): a new person creates their
 * account and lands in the workspace; someone with an account signs in and joins with one click.
 */
export function Invite() {
  const { query } = useRoute();
  const token = query.get('token') ?? '';
  const { status, user, signIn, signOut, selectWorkspace } = useSession();
  const [preview, setPreview] = useState<Preview | null>(null);
  const [invalid, setInvalid] = useState(false);
  const [name, setName] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!token) { setInvalid(true); return; }
    api<Preview>(`/api/v1/invitations/preview?token=${encodeURIComponent(token)}`).then(setPreview).catch(() => setInvalid(true));
  }, [token]);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try { await fn(); } catch (e) {
      setError(e instanceof ApiError
        ? e.code === 'invitation_invalid' ? 'This invitation has expired, was withdrawn or was already used.'
          : e.code === 'already_member' ? 'You’re already a member of this workspace.'
            : e.message
        : 'Something went wrong. Try again.');
    } finally { setBusy(false); }
  };

  /** After joining: enter the new workspace (the chooser explains it if the role needs two-step verification). */
  const enter = async (workspaceId: string) => {
    await selectWorkspace(workspaceId).catch(() => undefined);
    navigate('/', { replace: true });
  };

  if (invalid) {
    return (
      <>
        <h1>This invitation link doesn’t work</h1>
        <p className="lede">It may have expired, been withdrawn, or already been used. Ask the person who invited you to send a new one.</p>
        <div className="actions"><Link className="btn btn-secondary btn-md" to="/sign-in">Go to sign in</Link></div>
      </>
    );
  }
  if (!preview || status === 'loading') {
    return (
      <div className="sk-doc" role="status" aria-label="Loading invitation">
        <span className="sk sk-title" />
        {[88, 64].map((w) => <span key={w} className="sk sk-line" style={{ width: `${w}%` }} />)}
        <span className="sk sk-field" /><span className="sk sk-field" />
      </div>
    );
  }

  const heading = <h1>Join {preview.workspace}</h1>;
  const lede = (
    <p className="lede">
      {preview.inviter ? `${preview.inviter} invited ` : 'You were invited as '}<strong>{preview.email}</strong>. The link works until {shortDate(preview.expires_at)}.
    </p>
  );

  // Signed in already.
  if (status !== 'signed_out' && user) {
    if (user.email.toLowerCase() !== preview.email.toLowerCase()) {
      return (
        <>
          {heading}
          <Notice tone="warning" title={`You’re signed in as ${user.email}`}>This invitation is for {preview.email}. Sign out, then open the link again and sign in with that address.</Notice>
          <div className="actions"><Button variant="secondary" onPress={() => void signOut()}>Sign out</Button></div>
        </>
      );
    }
    return (
      <>
        {heading}
        {lede}
        {error && <Notice tone="danger" title={error} />}
        <div className="actions">
          <Button className="btn-block" isDisabled={busy} onPress={() => void run(async () => {
            const r = await api<{ workspace_id: string }>('/api/v1/invitations/accept', { method: 'POST', body: { token } });
            await enter(r.workspace_id);
          })}><MailCheck aria-hidden="true" size="1rem" /> {busy ? 'Joining…' : `Join ${preview.workspace}`}</Button>
        </div>
      </>
    );
  }

  // Signed out, with an account: sign in, then come back here to join.
  if (preview.account_exists) {
    return (
      <>
        {heading}
        {lede}
        <p>You already have a Procera account. Sign in to accept.</p>
        <div className="actions">
          <Button className="btn-block" onPress={() => navigate(`/sign-in?next=${encodeURIComponent(`/invite?token=${token}`)}`)}>Sign in to accept</Button>
        </div>
      </>
    );
  }

  // Signed out, new to Procera: create the account, which also accepts the invitation.
  const mismatch = confirm.length > 0 && confirm !== password;
  return (
    <>
      {heading}
      {lede}
      {error && <Notice tone="danger" title={error} />}
      <Form className="stack" onSubmit={(e) => {
        e.preventDefault();
        if (mismatch) return;
        void run(async () => {
          const r = await api<{ workspace_id: string }>('/api/v1/invitations/register', { method: 'POST', body: { token, display_name: name.trim(), password } });
          const s = await signIn(preview.email, password);
          if (!s.mfaToken) await enter(r.workspace_id);
        });
      }}>
        <TextField label="Work email" value={preview.email} isReadOnly autoComplete="username" />
        <TextField label="Your name" value={name} onChange={setName} isRequired autoComplete="name" autoFocus />
        <TextField label="Password" type="password" value={password} onChange={setPassword} isRequired autoComplete="new-password" description="At least 12 characters. A short sentence works well." />
        <TextField label="Confirm password" type="password" value={confirm} onChange={setConfirm} isRequired autoComplete="new-password" isInvalid={mismatch} errorMessage="The passwords don’t match." />
        <div className="actions">
          <Button type="submit" className="btn-block" isDisabled={busy || mismatch}>{busy ? 'Creating your account…' : 'Create account and join'}</Button>
        </div>
      </Form>
      <p className="auth-foot">Already have an account with this email? <Link to={`/sign-in?next=${encodeURIComponent(`/invite?token=${token}`)}`}>Sign in instead</Link>.</p>
    </>
  );
}
