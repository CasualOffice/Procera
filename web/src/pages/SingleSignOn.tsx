import { Check, Copy, ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { shortDate } from '../lib/format';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Checkbox } from '../ui/components/Checkbox';
import { ConfirmDialog } from '../ui/components/Dialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { Skeleton } from '../ui/States';
import { StepUpDialog } from '../ui/StepUp';

interface Config { issuer: string; client_id: string; has_secret: boolean; domains: string[]; enabled: boolean; required: boolean; revision: number; updated_at: string }

const errText = (e: unknown, fallback: string) => (e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : fallback);

/**
 * Admin › Single sign-on (BPMS-140, docs/06 §3.3): the workspace's OpenID Connect provider. People
 * whose verified email is in a trusted domain sign in through it; requiring it refuses password
 * sessions for everyone but owners (break-glass).
 */
export function SingleSignOn() {
  const { can } = useSession();
  const manage = can('workspace:settings.manage');
  const [data, setData] = useState<{ config: Config | null; callback_url: string } | null>(null);
  const [issuer, setIssuer] = useState('');
  const [clientId, setClientId] = useState('');
  const [secret, setSecret] = useState('');
  const [domains, setDomains] = useState('');
  const [enabled, setEnabled] = useState(true);
  const [required, setRequired] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string } | null>(null);
  const [stepUp, setStepUp] = useState<(() => void) | null>(null);
  const [copied, setCopied] = useState(false);

  const load = async () => {
    const d = await api<{ config: Config | null; callback_url: string }>('/api/v1/sso');
    setData(d);
    if (d.config) {
      setIssuer(d.config.issuer); setClientId(d.config.client_id); setDomains(d.config.domains.join(', '));
      setEnabled(d.config.enabled); setRequired(d.config.required);
    }
  };
  useEffect(() => { void load().catch((e) => setNotice({ tone: 'danger', title: errText(e, 'Couldn’t load single sign-on.') })); }, []);

  const act = async (f: () => Promise<void>, fallback: string) => {
    setBusy(true); setNotice(null);
    try { await f(); } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(() => () => void act(f, fallback));
      else setNotice({ tone: 'danger', title: errText(e, fallback) });
    } finally { setBusy(false); }
  };
  const save = () => act(async () => {
    const body: Record<string, unknown> = { issuer: issuer.trim(), client_id: clientId.trim(), domains: domains.split(/[,\s]+/).filter(Boolean), enabled, required };
    if (secret.trim()) body.client_secret = secret.trim();
    const headers = data?.config ? { 'If-Match': `"${data.config.revision}"` } : undefined;
    await api('/api/v1/sso', { method: 'PUT', headers, body });
    setSecret('');
    await load();
    setNotice({ tone: 'success', title: 'Saved. The provider answered, so people can sign in with it.' });
  }, 'Couldn’t save.');

  if (!data) return <Skeleton variant="doc" label="Loading single sign-on" />;
  const c = data.config;
  return (
    <>
      <div className="page-head">
        <div>
          <h1>Single sign-on</h1>
          <p>Let people sign in with your organisation’s identity provider (Okta, Microsoft Entra ID, Google, Keycloak, any OpenID Connect provider).</p>
        </div>
      </div>
      {notice && <Notice tone={notice.tone} title={notice.title} />}
      <section className="card sso-card" aria-labelledby="sso-provider">
        <h2 id="sso-provider" className="sso-h">Provider</h2>
        <p className="muted">In your provider, create an OpenID Connect web application with this redirect URL:</p>
        <div className="token-once">
          <code aria-label="Redirect URL">{data.callback_url}</code>
          <Button variant="secondary" onPress={() => { void navigator.clipboard?.writeText(data.callback_url); setCopied(true); }}>{copied ? <><Check aria-hidden="true" size="0.9375rem" /> Copied</> : <><Copy aria-hidden="true" size="0.9375rem" /> Copy</>}</Button>
        </div>
        <Form className="stack" onSubmit={(e) => { e.preventDefault(); void save(); }}>
          <TextField label="Issuer URL" value={issuer} onChange={setIssuer} isRequired isReadOnly={!manage} placeholder="https://login.example.com/realms/acme" description="Procera reads its /.well-known/openid-configuration before saving." />
          <TextField label="Client ID" value={clientId} onChange={setClientId} isRequired isReadOnly={!manage} />
          <TextField label="Client secret" type="password" value={secret} onChange={setSecret} isReadOnly={!manage} placeholder={c?.has_secret ? 'Saved; leave empty to keep it' : ''} description="Stored encrypted; never shown again." />
          <TextField label="Trusted email domains" value={domains} onChange={setDomains} isReadOnly={!manage} placeholder="acme.com, acme.co.uk" description="People whose verified email is in these domains can sign in through the provider. Separate with commas." />
          <Checkbox isSelected={enabled} onChange={setEnabled} isDisabled={!manage}>Single sign-on is on</Checkbox>
          <Checkbox isSelected={required} onChange={setRequired} isDisabled={!manage || !enabled}>Require it: members can’t use passwords</Checkbox>
          {required && <p className="sso-note"><ShieldCheck aria-hidden="true" size="0.875rem" /> Owners can still sign in with their password and two-step verification, so a provider outage can’t lock you out.</p>}
          {manage && (
            <div className="actions sso-actions">
              {c && (
                <ConfirmDialog destructive title="Remove single sign-on?" confirmLabel="Remove" onConfirm={() => void act(async () => { await api('/api/v1/sso', { method: 'DELETE' }); setIssuer(''); setClientId(''); setDomains(''); setRequired(false); setEnabled(true); await load(); setNotice({ tone: 'success', title: 'Single sign-on removed. Everyone signs in with a password again.' }); }, 'Couldn’t remove it.')}
                  trigger={<Button variant="ghost">Remove</Button>}>
                  <p>People who only ever signed in through the provider will need to set a password (Forgot password).</p>
                </ConfirmDialog>
              )}
              <Button type="submit" isDisabled={busy || !issuer.trim() || !clientId.trim()}>{busy ? 'Checking the provider…' : 'Save'}</Button>
            </div>
          )}
        </Form>
        {c && <p className="muted sso-updated">Last changed {shortDate(c.updated_at)}.</p>}
      </section>
      <StepUpDialog isOpen={stepUp !== null} onClose={() => setStepUp(null)} onDone={() => { const f = stepUp; setStepUp(null); f?.(); }} />
    </>
  );
}
