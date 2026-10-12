import { UserCheck, UserX } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { api } from '../lib/api';
import type { FormField, RoutePreview } from '../lib/requests';
import { type Values, toInput } from './requestFields';

/**
 * "Who approves this" on the request form (BPMS-103): the server dry-runs the routing for the
 * answers so far (`POST /requests/preview`, nothing is saved) and says who decides first and which
 * later steps apply. It refreshes, debounced, as answers that can change the route change.
 */
export function WhoApproves({ typeKey, fields, values }: { typeKey: string; fields: FormField[]; values: Values }) {
  const routed = useMemo(() => fields.filter((f) => f.type !== 'file' && f.type !== 'text'), [fields]);
  const input = useMemo(() => JSON.stringify(toInput(routed, values)), [routed, values]);
  const [preview, setPreview] = useState<RoutePreview | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    const t = window.setTimeout(() => {
      api<RoutePreview>('/api/v1/requests/preview', { method: 'POST', body: { type: typeKey, input: JSON.parse(input) as Record<string, unknown> } })
        .then((p) => { if (live) { setPreview(p); setFailed(false); } })
        .catch(() => { if (live) setFailed(true); });
    }, 350);
    return () => { live = false; window.clearTimeout(t); };
  }, [typeKey, input]);

  const skipped = (preview?.steps ?? []).filter((s) => s.applies === false && s.condition);
  return (
    <section className={`req-who${preview?.unassigned ? ' req-who-nobody' : ''}`} aria-labelledby="req-who-title">
      <h2 id="req-who-title">{preview?.unassigned ? <UserX aria-hidden="true" size="1rem" /> : <UserCheck aria-hidden="true" size="1rem" />} Who approves this</h2>
      <p className="req-who-summary" aria-live="polite" data-testid="who-approves">
        {preview ? preview.summary : failed ? 'We couldn’t work out the approvers right now. Your request still goes to the right people.' : 'Working out who approves…'}
      </p>
      {skipped.length > 0 && (
        <ul className="req-who-skipped">
          {skipped.map((s) => <li key={s.node}>Not needed now: {s.who} approves only when {s.condition}.</li>)}
        </ul>
      )}
    </section>
  );
}
