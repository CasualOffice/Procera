import { Send } from 'lucide-react';
import { useState } from 'react';
import { useResource } from '../../lib/data';
import { type EmailView, type PlatformBasics, sysCan } from '../../lib/system';
import { Button } from '../../ui/components/Button';
import { Notice } from '../../ui/components/Notice';
import { LoadError, Skeleton } from '../../ui/States';
import { useSystem } from '../../ui/SystemShell';
import { Tag } from '../../ui/Tag';
import { EmailSettingsForm, SendTestDialog } from './EmailSettingsForm';

const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;
const TLS_LABEL = { starttls_required: 'STARTTLS (required)', implicit_tls: 'Implicit TLS', none: 'None' } as const;

/** The delivery status chip: where emails go out from, in words. */
export function EmailStatus({ view }: { view: EmailView }) {
  if (view.source === 'environment') return <Tag color="blue" dot>Set by environment</Tag>;
  if (view.source === 'database') return <Tag color="green" dot>Configured</Tag>;
  return <Tag color="yellow" dot>Not set up — emails are queued</Tag>;
}

/**
 * System · Email & platform (docs/20 §3, BPMS-065): the SMTP relay with a test send, then what this
 * installation uses for its public address, file storage and malware scanning. Only email is
 * configurable here; the rest is shown as it is, with where each value comes from.
 */
export function SystemEmail() {
  const me = useSystem();
  const canConfig = sysCan(me.roles, 'platform.config');
  const [view, reload, error] = useResource<EmailView>('/sysapi/v1/email');
  const [basics, reloadBasics, basicsError] = useResource<PlatformBasics>('/sysapi/v1/platform');
  const [saved, setSaved] = useState<EmailView | null>(null);
  const [testing, setTesting] = useState(false);
  const current = saved ?? view;

  return (
    <>
      <div className="page-head">
        <div><h1>Email &amp; platform</h1><p>How Procera sends email, and what this installation uses for its address, files and malware scanning.</p></div>
      </div>

      <section className="sys-card" aria-labelledby="email-h">
        <div className="sys-card-head">
          <h2 id="email-h">Email delivery</h2>
          {current && <EmailStatus view={current} />}
        </div>
        {error ? <LoadError error={error} onRetry={reload} what="email settings" /> : !current ? <Skeleton rows={4} label="Loading email settings" /> : (
          <>
            {saved && <Notice tone="success" title="Email settings saved">The delivery worker uses them from its next pass; no restart needed.</Notice>}
            {current.source === 'none' && (
              <Notice tone="warning" title="Email isn’t set up, so nothing is sent">
                Invitations, reminders and password resets wait in the queue{current.queue.queued > 0 ? ` (${plural(current.queue.queued, 'email')} now)` : ''} and go out once delivery works.
              </Notice>
            )}
            {current.source === 'environment' ? (
              <>
                <Notice tone="info" title="Set by the environment">
                  The <code>PROCERA_SMTP_URL</code> setting on the server takes precedence over anything saved here. Change it where Procera is deployed, or remove it to manage email on this page.
                </Notice>
                <dl className="ws-facts sys-facts">
                  <dt>Sender</dt><dd>{current.environment?.from ?? 'Not set'}</dd>
                  <dt>Queue</dt><dd>{plural(current.queue.queued, 'email')} waiting{current.queue.failed > 0 ? `, ${plural(current.queue.failed, 'failed')}` : ''}</dd>
                </dl>
                {canConfig && <div className="actions"><Button variant="secondary" onPress={() => setTesting(true)}><Send aria-hidden="true" size="0.875rem" /> Send test email</Button></div>}
              </>
            ) : canConfig ? (
              <EmailSettingsForm view={current} onSaved={(v) => { setSaved(v); reloadBasics(); }}
                extra={current.source === 'database' && <Button variant="secondary" onPress={() => setTesting(true)}><Send aria-hidden="true" size="0.875rem" /> Send test email</Button>} />
            ) : current.config ? (
              <dl className="ws-facts sys-facts" aria-label="Email settings">
                <dt>SMTP host</dt><dd>{current.config.host}:{current.config.port}</dd>
                <dt>Encryption</dt><dd>{TLS_LABEL[current.config.tls_mode]}</dd>
                <dt>Username</dt><dd>{current.config.username ?? 'None'}</dd>
                <dt>Password</dt><dd>{current.config.password_set ? 'Set (never shown)' : 'Not set'}</dd>
                <dt>From</dt><dd>{current.config.from_name ? `${current.config.from_name} <${current.config.from_address}>` : current.config.from_address}</dd>
                <dt>Reply-to</dt><dd>{current.config.reply_to ?? 'None'}</dd>
              </dl>
            ) : <p className="muted">No email settings are saved. A system owner or admin can set them up.</p>}
          </>
        )}
        <details className="sys-help">
          <summary>Help email reach inboxes (SPF, DKIM, DMARC)</summary>
          <p>This is guidance only: Procera doesn’t sign messages itself. Add these DNS records at the domain of your from address:</p>
          <ul>
            <li><strong>SPF</strong>: a TXT record that lists your mail provider, for example <code>v=spf1 include:your-provider.example ~all</code>.</li>
            <li><strong>DKIM</strong>: turn on signing at your mail provider and publish the TXT or CNAME record it gives you.</li>
            <li><strong>DMARC</strong>: a TXT record at <code>_dmarc.your-domain</code>, starting with <code>v=DMARC1; p=none</code> while you check reports.</li>
          </ul>
        </details>
      </section>

      <section className="sys-card" aria-labelledby="basics-h">
        <div className="sys-card-head"><h2 id="basics-h">Platform basics</h2></div>
        {basicsError ? <LoadError error={basicsError} onRetry={reloadBasics} what="platform details" /> : !basics ? <Skeleton rows={2} label="Loading platform details" /> : (
          <dl className="ws-facts sys-facts" aria-label="Platform basics">
            <dt>Public address</dt>
            <dd>{basics.public_url}<span className="sys-source">{basics.public_url_source === 'environment' ? 'From PROCERA_PUBLIC_URL on the server.' : basics.public_url_source === 'command_line' ? 'From the --public-url option on the server.' : 'The built-in default. Set PROCERA_PUBLIC_URL so links in emails work for everyone.'}</span></dd>
            <dt>Sender name</dt>
            <dd>{basics.sender_name ?? 'Not set'}<span className="sys-source">The default for every workspace; each workspace can set its own in its branding.</span></dd>
          </dl>
        )}
      </section>

      <section className="sys-card" aria-labelledby="storage-h">
        <div className="sys-card-head"><h2 id="storage-h">File storage and malware scanning</h2></div>
        {basics && (
          <>
            <dl className="ws-facts sys-facts" aria-label="Storage and scanning">
              <dt>File storage</dt>
              <dd>{basics.storage.kind === 's3'
                ? <>S3-compatible bucket: <code>{basics.storage.path}</code><span className="sys-source">From PROCERA_STORAGE=s3 and PROCERA_S3_* on the server. Files are encrypted before they’re stored.</span></>
                : <>Local disk: <code>{basics.storage.path}</code><span className="sys-source">{basics.storage.source === 'environment' ? 'From PROCERA_FILES_DIR on the server.' : 'A temporary folder. Set PROCERA_FILES_DIR to keep files across restarts.'} For an S3-compatible bucket, set PROCERA_STORAGE=s3.</span></>}</dd>
              <dt>Malware scanning</dt>
              <dd>{basics.scanning.kind === 'clamd' ? <>ClamAV at <code>{basics.scanning.address}</code></> : 'Built-in check'}<span className="sys-source">{basics.scanning.kind === 'clamd' ? 'From PROCERA_CLAMD_ADDR on the server.' : 'Detects only the EICAR test file.'}</span></dd>
            </dl>
            {basics.storage.source === 'temporary' && <Notice tone="warning" title="Files are in a temporary folder">They can be lost when the server restarts. Set PROCERA_FILES_DIR to a lasting location.</Notice>}
            {basics.scanning.kind === 'eicar_only' && <Notice tone="warning" title="Uploads aren’t scanned for real malware">Connect a ClamAV server with PROCERA_CLAMD_ADDR.</Notice>}
          </>
        )}
      </section>
      {testing && <SendTestDialog onClose={() => setTesting(false)} />}
    </>
  );
}
