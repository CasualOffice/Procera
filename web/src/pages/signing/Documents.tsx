import { FileSignature, Plus } from 'lucide-react';
import { useState } from 'react';
import { useItems } from '../../lib/data';
import { shortDate } from '../../lib/format';
import { Link, navigate } from '../../lib/router';
import { useSession } from '../../lib/session';
import { type Envelope, STATE_COLOR, STATE_LABEL } from '../../lib/signing';
import { Button } from '../../ui/components/Button';
import { Empty, LoadError, Skeleton } from '../../ui/States';
import { Tag } from '../../ui/Tag';
import { progressText } from './shared';
import './sender.css';

/**
 * Documents (BPMS-107, docs/05 §3.22 "Tracking"): the envelopes I sent for signature, newest first,
 * with where each one stands. People who can see every envelope switch to "Everyone's".
 */
export function Documents() {
  const { can } = useSession();
  const all = can('document:view_all');
  const send = can('document:send_for_signature');
  const [scope, setScope] = useState<'sent' | 'all'>(send ? 'sent' : 'all');
  const [items, reload, error] = useItems<Envelope>(`/api/v1/envelopes${scope === 'all' ? '?scope=all' : ''}`);

  return (
    <>
      <div className="page-head">
        <div>
          <h1>Documents</h1>
          <p>Send a PDF to people in your workspace to sign, in order or all at once, and keep the signed copy with its certificate.</p>
        </div>
        {send && <Button onPress={() => navigate('/documents/new')}><Plus aria-hidden="true" size="1rem" /> Send for signature</Button>}
      </div>
      {all && send && (
        <div className="toolbar">
          <div className="segmented" role="group" aria-label="Show">
            <button type="button" aria-pressed={scope === 'sent'} onClick={() => setScope('sent')}>Sent by me</button>
            <button type="button" aria-pressed={scope === 'all'} onClick={() => setScope('all')}>Everyone’s</button>
          </div>
        </div>
      )}
      {error ? <LoadError error={error} onRetry={reload} what="documents" />
        : items === null ? <Skeleton variant="table" rows={4} label="Loading documents" />
          : items.length === 0 ? (
            <Empty icon={FileSignature} title={scope === 'all' ? 'Nothing has been sent for signature yet' : 'You haven’t sent anything for signature'}
              action={send ? <Button onPress={() => navigate('/documents/new')}><Plus aria-hidden="true" size="1rem" /> Send for signature</Button> : undefined}>
              Upload a PDF, choose who signs and in what order, and place where each person signs. Everyone is told when it’s their turn, and you get the signed copy.
            </Empty>
          ) : (
            <div className="table-wrap">
              <table className="table" aria-label="Documents sent for signature">
                <thead><tr><th>Document</th><th>Status</th><th>Signatures</th><th className="col-optional">Sent</th></tr></thead>
                <tbody>
                  {items.map((e) => (
                    <tr key={e.id}>
                      <td>
                        <span className="cell-entity">
                          <span className="doc-icon tag-gray"><FileSignature aria-hidden="true" size="0.875rem" /></span>
                          <span className="cell-stack">
                            <Link to={e.state === 'draft' ? `/documents/${e.id}/edit` : `/documents/${e.id}`}>{e.title}</Link>
                            <span className="cell-sub">{scope === 'all' ? `From ${e.sender_name}` : e.recipients.length === 1 ? e.recipients[0]!.name : `${e.recipients.length} recipients`}</span>
                          </span>
                        </span>
                      </td>
                      <td><Tag color={STATE_COLOR[e.state]} dot>{STATE_LABEL[e.state]}</Tag></td>
                      <td className="cell-muted">{e.state === 'draft' ? 'Not sent' : progressText(e)}</td>
                      <td className="col-optional cell-muted">{e.sent_at ? shortDate(e.sent_at) : '—'}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
    </>
  );
}
