import { Send } from 'lucide-react';
import { useEffect, useState } from 'react';
import { useResource } from '../../lib/data';
import { Link, navigate } from '../../lib/router';
import { useSession } from '../../lib/session';
import type { EmailView } from '../../lib/system';
import { Button } from '../../ui/components/Button';
import { Notice } from '../../ui/components/Notice';
import { LoadError, Skeleton } from '../../ui/States';
import { EmailSettingsForm, SendTestDialog } from './EmailSettingsForm';
import { type Created, NewWorkspaceDialog } from './NewWorkspaceDialog';

type Step = 'email' | 'workspace' | 'done';
const STEPS: { id: Step; label: string }[] = [
  { id: 'email', label: '1. Email (optional)' },
  { id: 'workspace', label: '2. First workspace' },
  { id: 'done', label: '3. Done' },
];

/**
 * First-time setup, after the system owner exists and has signed in with two-step verification
 * (`/system/setup`, FR-S01, BPMS-065): an optional email step with a test send, then the first
 * workspace, whose owner is invited by email.
 */
export function SystemSetup() {
  const { user } = useSession();
  const [step, setStep] = useState<Step>('email');
  const [view, reload, error] = useResource<EmailView>('/sysapi/v1/email');
  const [saved, setSaved] = useState<EmailView | null>(null);
  const [testing, setTesting] = useState(false);
  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<Created | null>(null);
  const current = saved ?? view;
  useEffect(() => { document.getElementById('setup-h')?.focus(); }, [step]);

  return (
    <>
      <div className="page-head"><div><h1>Finish setting up</h1><p>Two short steps. You can change everything later in the system console.</p></div></div>
      <ol className="setup-steps" aria-label="Setup steps">
        {STEPS.map((s) => <li key={s.id} aria-current={s.id === step ? 'step' : undefined}>{s.label}</li>)}
      </ol>

      {step === 'email' && (
        <section className="sys-card" aria-labelledby="setup-h">
          <div className="sys-card-head"><h2 id="setup-h" tabIndex={-1}>Set up email</h2></div>
          <p>Procera emails invitations, reminders and password resets. Without email, they wait in a queue until you set it up.</p>
          {error ? <LoadError error={error} onRetry={reload} what="email settings" /> : !current ? <Skeleton rows={4} label="Loading email settings" /> : current.source === 'environment' ? (
            <>
              <Notice tone="info" title="Email is already set by the environment">The server’s PROCERA_SMTP_URL setting is used, so there’s nothing to enter here.</Notice>
              <div className="actions">
                <Button variant="secondary" onPress={() => setTesting(true)}><Send aria-hidden="true" size="0.875rem" /> Send test email</Button>
                <Button onPress={() => setStep('workspace')}>Continue</Button>
              </div>
            </>
          ) : saved ? (
            <>
              <Notice tone="success" title="Email settings saved">Send yourself a test email to check delivery, then continue.</Notice>
              <div className="actions">
                <Button variant="secondary" onPress={() => setTesting(true)}><Send aria-hidden="true" size="0.875rem" /> Send test email</Button>
                <Button onPress={() => setStep('workspace')}>Continue</Button>
              </div>
            </>
          ) : (
            <EmailSettingsForm view={current} onSaved={setSaved} fixedReason="First-time setup" submitLabel="Save and continue"
              extra={<Button variant="ghost" onPress={() => setStep('workspace')}>Skip for now</Button>} />
          )}
        </section>
      )}

      {step === 'workspace' && (
        <section className="sys-card" aria-labelledby="setup-h">
          <div className="sys-card-head"><h2 id="setup-h" tabIndex={-1}>Create your first workspace</h2></div>
          <p>A workspace is one organisation’s space: its people, documents and settings. Its owner gets an email invitation to set it up, and that can be you.</p>
          <div className="actions">
            <Button onPress={() => setCreating(true)}>Create workspace</Button>
            <Button variant="ghost" onPress={() => navigate('/system/workspaces')}>Skip for now</Button>
          </div>
        </section>
      )}

      {step === 'done' && created && (
        <section className="sys-card" aria-labelledby="setup-h">
          <div className="sys-card-head"><h2 id="setup-h" tabIndex={-1}>{created.name} is ready</h2></div>
          <Notice tone="success" title={`We invited ${created.ownerEmail} to own ${created.name}`}>
            {current?.source === 'none'
              ? 'Email isn’t set up yet, so the invitation waits in the queue. Set up email in Email & platform and it goes out.'
              : created.ownerEmail.toLowerCase() === user?.email.toLowerCase()
                ? 'Open the invitation in your inbox to join the workspace and start setting it up.'
                : 'They join from the invitation in their inbox and take it from there.'}
          </Notice>
          <div className="actions">
            <Link className="btn btn-primary btn-md link-btn" to="/system/workspaces">Go to the system console</Link>
            {current?.source === 'none' && <Link className="btn btn-secondary btn-md link-btn" to="/system/email">Set up email</Link>}
          </div>
        </section>
      )}

      {testing && <SendTestDialog onClose={() => setTesting(false)} />}
      {creating && <NewWorkspaceDialog onClose={() => setCreating(false)} onCreated={(c) => { setCreating(false); setCreated(c); setStep('done'); }} />}
    </>
  );
}
