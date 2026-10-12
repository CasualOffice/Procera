import { BookOpen } from 'lucide-react';
import type { PolicyRef } from '../lib/requests';
import { Link } from '../lib/router';

/** "Policy: Travel and expenses (v1.2)", linking to the document (the governing policy or SOP of a request type). */
export function PolicyLine({ policy }: { policy: PolicyRef }) {
  return (
    <p className="req-policy">
      <BookOpen aria-hidden="true" size="0.875rem" /> {policy.kind === 'sop' ? 'SOP' : 'Policy'}:{' '}
      <Link to={`/library/${policy.artifact_id}`}>{policy.title} (v{policy.version})</Link>
    </p>
  );
}
