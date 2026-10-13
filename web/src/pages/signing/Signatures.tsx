import { ArrowUpRight, Download, FileSignature } from 'lucide-react';
import { download } from '../../lib/api';
import { useItems } from '../../lib/data';
import { shortDate, timeAgo } from '../../lib/format';
import { Link } from '../../lib/router';
import { type InboxItem, STATE_COLOR, STATE_LABEL } from '../../lib/signing';
import { Button } from '../../ui/components/Button';
import { SectionTabs } from '../../ui/SectionTabs';
import { Empty, LoadError, Skeleton } from '../../ui/States';
import { Tag } from '../../ui/Tag';
import './signing.css';

/** What a row says about where things stand for me. */
function status(i: InboxItem): { label: string; color: 'blue' | 'gray' | 'green' | 'red' | 'orange' } {
  if (i.to_sign) return { label: 'Your turn', color: 'blue' };
  if (i.state === 'signed' && i.envelope_state === 'sent') return { label: 'You signed', color: 'green' };
  if (i.state === 'pending' && i.envelope_state === 'sent' && i.role === 'signer') return { label: 'Waiting for others first', color: 'gray' };
  if (i.role === 'cc' && i.envelope_state === 'sent') return { label: 'You’ll get a copy', color: 'gray' };
  return { label: STATE_LABEL[i.envelope_state], color: STATE_COLOR[i.envelope_state] };
}

/**
 * My work › Signatures (BPMS-107, docs/05 §3.24): documents waiting for my signature first, then the
 * rest I'm on (waiting for someone before me, signed, ended), with the signed copy once complete.
 */
export function Signatures() {
  const [items, reload, error] = useItems<InboxItem>('/api/v1/me/signatures');
  const toSign = (items ?? []).filter((i) => i.to_sign);
  const rest = (items ?? []).filter((i) => !i.to_sign);
  return (
    <div className="work">
      <div className="page-head work-head">
        <div><h1>My work</h1><p>Documents waiting for your signature, and the ones you’ve signed.</p></div>
      </div>
      <SectionTabs label="Work views" tabs={[
        { label: 'Open', to: '/work', current: false },
        { label: 'Done', to: '/work?state=done', current: false },
        { label: toSign.length ? `Signatures (${toSign.length})` : 'Signatures', to: '/work/signatures', current: true },
      ]} />
      {error ? <LoadError error={error} onRetry={reload} what="your documents" /> : items === null ? <Skeleton rows={3} label="Loading your documents" /> : items.length === 0 ? (
        <div className="card"><Empty icon={FileSignature} title="Nothing to sign">When someone sends you a document to sign, it appears here and you get an email.</Empty></div>
      ) : (
        <>
          {toSign.length > 0 && <Rows title="Waiting for your signature" items={toSign} />}
          {rest.length > 0 && <Rows title={toSign.length ? 'Everything else' : 'Your documents'} items={rest} />}
        </>
      )}
    </div>
  );
}

function Rows({ title, items }: { title: string; items: InboxItem[] }) {
  return (
    <section className="sig-section" aria-label={title}>
      <h2 className="sig-section-title">{title}</h2>
      <ul className="card rows" aria-label={title}>
        {items.map((i) => {
          const st = status(i);
          return (
            <li key={i.recipient_id}>
              <span className="row-icon"><FileSignature aria-hidden="true" size="1rem" /></span>
              <div className="row-main">
                <p className="row-title"><Link to={`/sign/${i.recipient_id}`}>{i.title}</Link> <Tag color={st.color} dot>{st.label}</Tag></p>
                <p className="row-sub">
                  From {i.sender_name}{i.sent_at ? ` · sent ${timeAgo(i.sent_at)}` : ''}
                  {i.to_sign && i.expires_at ? ` · sign by ${shortDate(i.expires_at)}` : ''}
                  {i.signed_at ? ` · you signed ${shortDate(i.signed_at)}` : ''}
                </p>
              </div>
              {i.envelope_state === 'completed' ? (
                <Button variant="secondary" className="btn-sm" aria-label={`Download the signed copy of ${i.title}`}
                  onPress={() => void download(`/api/v1/envelopes/${i.envelope_id}/document?signed=true`, `${i.title} (signed).pdf`)}>
                  <Download aria-hidden="true" size="0.875rem" /> Signed copy
                </Button>
              ) : (
                <Link className={i.to_sign ? 'btn btn-primary btn-sm link-btn' : 'btn btn-secondary btn-sm link-btn'} to={`/sign/${i.recipient_id}`}>
                  {i.to_sign ? 'Review and sign' : 'Open'} <ArrowUpRight aria-hidden="true" size="0.875rem" />
                </Link>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
