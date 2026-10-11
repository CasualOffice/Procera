import { term } from '../lib/terms';

export type TagColor = 'gray' | 'brown' | 'orange' | 'yellow' | 'green' | 'blue' | 'purple' | 'pink' | 'red';

/** A soft coloured tag (Notion/Attio style). Meaning is always in the text, never colour alone. */
export function Tag({ color = 'gray', dot = false, children }: { color?: TagColor; dot?: boolean; children: React.ReactNode }) {
  return <span className={`tag tag-${color}${dot ? ' tag-dot' : ''}`}>{children}</span>;
}

export const STATE_TAG: Record<string, { label: string; color: TagColor }> = {
  draft: { label: 'Draft', color: 'gray' },
  in_review: { label: 'In review', color: 'yellow' },
  approved: { label: 'Approved', color: 'blue' },
  scheduled: { label: 'Scheduled', color: 'orange' },
  published: { label: 'Published', color: 'green' },
  deprecated: { label: 'Superseded', color: 'brown' },
  retired: { label: 'Retired', color: 'gray' },
};

export const KIND_TAG: Record<string, { label: string; color: TagColor }> = {
  policy: { label: 'Policy', color: 'purple' },
  // The workspace's term for SOPs (CFG-008); components that show it call useTerms().
  sop: { get label() { return term('sop', { capitalised: true }); }, color: 'blue' },
};
