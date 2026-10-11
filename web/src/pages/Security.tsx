import { Copy, Download, KeyRound, Plus, ShieldCheck, Smartphone, Trash2 } from 'lucide-react';
import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { useItems } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { PasskeyCancelled, createPasskey, passkeysSupported } from '../lib/passkey';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { ConfirmDialog } from '../ui/components/Dialog';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { StepUpDialog } from '../ui/StepUp';
import { Tag } from '../ui/Tag';

interface Passkey { id: string; name: string; created_at: string; last_used_at: string | null; synced: boolean; user_verified: boolean; transports: string[] }

/** A sensible default name from the device the person is using right now. */
function deviceName(): string {
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return 'iPhone';
  if (/iPad/.test(ua)) return 'iPad';
  if (/Android/.test(ua)) return 'Android phone';
  if (/Mac/.test(ua)) return 'Mac';
  if (/Windows/.test(ua)) return 'Windows PC';
  return 'Security key';
}

/**
 * Account › Security (BPMS-022, docs/12 §3): two-step verification status and the person's
 * passkeys. Adding a passkey needs a signed-in session; removing one asks them to confirm it's them.
 */
export function Security() {
  const { user, reload } = useSession();
  const [items, refresh, error] = useItems<Passkey>('/api/v1/me/mfa/webauthn');
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [addError, setAddError] = useState<string | null>(null);
  const [codes, setCodes] = useState<string[] | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [removeError, setRemoveError] = useState<string | null>(null);
  const [stepUp, setStepUp] = useState<(() => Promise<void>) | null>(null);
  const supported = passkeysSupported();
  const totp = user?.mfa_methods.includes('totp') ?? false;

  const add = async () => {
    setBusy(true); setAddError(null);
    try {
      const credential = await createPasskey();
      const r = await api<{ passkey: Passkey; recovery_codes: string[] | null }>('/api/v1/me/mfa/webauthn', { method: 'POST', body: { name: name.trim(), credential } });
      setAdding(false);
      setNotice(`“${r.passkey.name}” is ready. You can use it the next time you sign in.`);
      if (r.recovery_codes) setCodes(r.recovery_codes);
      refresh();
      await reload();
    } catch (e) {
      setAddError(
        e instanceof PasskeyCancelled ? 'The passkey request was cancelled or timed out. Try again when you’re ready.'
          : e instanceof ApiError && e.code === 'passkey_exists' ? 'This device already has a passkey for your account.'
            : e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not add the passkey. Try again.',
      );
    } finally { setBusy(false); }
  };

  const remove = async (p: Passkey) => {
    const run = async () => {
      await api(`/api/v1/me/mfa/webauthn/${p.id}`, { method: 'DELETE' });
      setNotice(`“${p.name}” was removed.`);
      refresh();
      await reload();
    };
    setRemoveError(null);
    try { await run(); } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(() => async () => { try { await run(); } catch (err) { setRemoveError(err instanceof Error ? err.message : 'Could not remove the passkey.'); } });
      else setRemoveError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not remove the passkey.');
    }
  };

  const download = () => {
    if (!codes) return;
    const blob = new Blob([`Procera recovery codes\n\n${codes.join('\n')}\n\nEach code works once.\n`], { type: 'text/plain' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'procera-recovery-codes.txt';
    a.click();
    URL.revokeObjectURL(a.href);
  };

  const count = items?.length ?? 0;
  return (
    <>
      <div className="page-head">
        <div>
          <h1>Security</h1>
          <p>How you sign in to Procera. These settings belong to your account and apply in every workspace.</p>
        </div>
      </div>
      {notice && <Notice tone="success" title={notice} />}

      <section className="account-section" aria-labelledby="sec-mfa">
        <div className="section-head"><h2 id="sec-mfa">Two-step verification</h2></div>
        <ul className="card rows">
          <li>
            <span className="row-icon"><ShieldCheck aria-hidden="true" size="1rem" /></span>
            <div className="row-main">
              <p className="row-title">Status</p>
              <p className="row-sub">{user?.mfa_enabled ? 'Sign-ins need a second step after your password.' : 'Only your password protects your account.'}</p>
            </div>
            {user?.mfa_enabled ? <Tag color="green" dot>On</Tag> : <Tag color="gray" dot>Off</Tag>}
          </li>
          <li>
            <span className="row-icon"><Smartphone aria-hidden="true" size="1rem" /></span>
            <div className="row-main">
              <p className="row-title">Authenticator app</p>
              <p className="row-sub">6-digit codes from an app such as 1Password or Google Authenticator.</p>
            </div>
            <span className="row-meta">{totp ? 'Set up' : 'Not set up'}</span>
          </li>
        </ul>
      </section>

      <section className="account-section" aria-labelledby="sec-passkeys">
        <div className="section-head">
          <h2 id="sec-passkeys">Passkeys</h2>
          {count > 0 && <span className="account-count">{count}</span>}
          {supported && (
            <Button variant="secondary" className="btn-sm account-add" onPress={() => { setName(deviceName()); setAddError(null); setAdding(true); }}>
              <Plus aria-hidden="true" size="0.875rem" /> Add a passkey
            </Button>
          )}
        </div>
        <p className="account-lede">Confirm sign-ins with your fingerprint, face, screen lock or a security key instead of typing a code. A passkey only works on this site, so a look-alike page can’t use it.</p>
        {!supported && <Notice tone="info" title="This browser can’t create passkeys">Use an up-to-date Chrome, Edge, Firefox or Safari to add one.</Notice>}
        {removeError && <Notice tone="danger" title={removeError} />}
        {items === null ? <Skeleton rows={2} label="Loading passkeys" />
          : error ? <LoadError error={error} onRetry={refresh} what="your passkeys" />
            : count === 0 ? (
              <div className="card"><Empty compact icon={KeyRound} title="No passkeys yet">Add one to sign in with this device instead of a code.</Empty></div>
            ) : (
              <ul className="card rows" aria-label="Your passkeys">
                {items.map((p) => {
                  const last = !totp && count === 1;
                  return (
                    <li key={p.id}>
                      <span className="row-icon"><KeyRound aria-hidden="true" size="1rem" /></span>
                      <div className="row-main">
                        <p className="row-title">{p.name}{p.synced && <> <Tag color="blue">Synced</Tag></>}</p>
                        <p className="row-sub">Added {shortDate(p.created_at)} · {p.last_used_at ? `Last used ${timeAgo(p.last_used_at)}` : 'Not used yet'}</p>
                      </div>
                      <ConfirmDialog
                        destructive
                        title={`Remove “${p.name}”?`}
                        confirmLabel="Remove passkey"
                        onConfirm={() => void remove(p)}
                        trigger={<Button variant="ghost" className="btn-sm" aria-label={`Remove ${p.name}`}><Trash2 aria-hidden="true" size="0.875rem" /> Remove</Button>}
                      >
                        <p>You won’t be able to sign in or confirm actions with this passkey any more.</p>
                        {last && <p>It’s your only second step, so two-step verification turns off and your recovery codes stop working. Workspaces that require it will ask you to set it up again.</p>}
                      </ConfirmDialog>
                    </li>
                  );
                })}
              </ul>
            )}
      </section>

      <FormDialog isOpen={adding} onOpenChange={(o) => !o && !busy && setAdding(false)} title="Add a passkey" width={440}
        description="Give it a name you’ll recognise, then follow your browser’s prompt."
        footer={<><Button variant="secondary" onPress={() => setAdding(false)} isDisabled={busy}>Cancel</Button><Button onPress={() => void add()} isDisabled={busy || !name.trim()}>{busy ? 'Waiting for your device…' : 'Continue'}</Button></>}>
        {addError && <Notice tone="danger" title={addError} />}
        <Form onSubmit={(e) => { e.preventDefault(); void add(); }}>
          <TextField label="Passkey name" value={name} onChange={setName} maxLength={64} autoFocus description="For example: Work laptop, YubiKey, Pixel phone." />
        </Form>
      </FormDialog>

      <FormDialog isOpen={codes !== null} onOpenChange={() => undefined} title="Save your recovery codes" width={460}
        description="If you lose your passkey, each code lets you sign in once. Store them somewhere safe. You won’t see them again."
        footer={<Button onPress={() => setCodes(null)}>I’ve saved them</Button>}>
        <ul className="recovery" aria-label="Recovery codes">{codes?.map((c) => <li key={c}><code>{c}</code></li>)}</ul>
        <div className="actions">
          <Button variant="secondary" onPress={download}><Download aria-hidden="true" size="0.9375rem" /> Download</Button>
          <Button variant="secondary" onPress={() => void navigator.clipboard?.writeText(codes?.join('\n') ?? '')}><Copy aria-hidden="true" size="0.9375rem" /> Copy</Button>
        </div>
      </FormDialog>

      <StepUpDialog isOpen={stepUp !== null} onClose={() => setStepUp(null)} onDone={() => { const f = stepUp; setStepUp(null); void f?.(); }} />
    </>
  );
}
