import { CircleCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { Link, navigate } from '../lib/router';
import { Empty, Skeleton } from '../ui/States';

interface Ack { id: string; subject: { artifact_id: string } }

/**
 * `/acknowledgements/{id}`, the link in acknowledgement emails and notifications (BPMS-046): opens
 * the document the request is about, where the acknowledgement callout waits.
 */
export function AckLink({ id }: { id: string }) {
  const [missing, setMissing] = useState(false);
  useEffect(() => {
    api<{ items: Ack[] }>('/api/v1/me/acknowledgements')
      .then((r) => {
        const a = r.items.find((x) => x.id === id);
        if (a) navigate(`/library/${a.subject.artifact_id}`, { replace: true });
        else setMissing(true);
      })
      .catch(() => setMissing(true));
  }, [id]);
  if (!missing) return <div className="doc"><Skeleton variant="doc" label="Opening the document" /></div>;
  return (
    <Empty icon={CircleCheck} title="This request isn’t open for you" action={<Link className="btn btn-secondary btn-md" to="/">Go to Home</Link>}>
      It may have been withdrawn, or it was sent to someone else.
    </Empty>
  );
}
