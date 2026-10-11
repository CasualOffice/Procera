import { BriefcaseBusiness, Check } from 'lucide-react';
import { useState } from 'react';
import { ApiError, api } from '../lib/api';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { useStepUp } from '../ui/StepUp';
import type { Role } from '../lib/roles';

/**
 * Roles for common jobs (docs/05 §2.1; A-43): one click creates a custom role with a sensible set
 * of permissions on top of Member, editable afterwards like any custom role. Department leads are
 * meant to be granted for their department or collection.
 */
export interface Preset { key: string; name: string; who: string; does: string[]; scope: string; permissions: string[] }

export const PRESETS: Preset[] = [
  {
    key: 'job.hr_lead', name: 'HR lead', who: 'HR, People team',
    does: ['Writes and publishes policies', 'Posts announcements', 'Runs acknowledgement campaigns', 'Invites people and keeps departments current'],
    scope: 'Usually the whole workspace',
    permissions: ['policy:view', 'policy:create', 'policy:edit_draft', 'policy:publish', 'announcement:view', 'announcement:create', 'announcement:publish', 'campaign:view', 'campaign:manage', 'member:view', 'member:invite', 'directory:view', 'directory:manage', 'template:browse', 'template:install'],
  },
  {
    key: 'job.finance_lead', name: 'Finance lead', who: 'CFO, finance managers',
    does: ['Owns finance policies and procedures', 'Reviews and approves changes', 'Sees campaign progress'],
    scope: 'Grant for the Finance department or collection',
    permissions: ['policy:view', 'policy:create', 'policy:edit_draft', 'policy:review', 'policy:approve', 'policy:publish', 'sop:view', 'sop:create', 'sop:edit_draft', 'sop:review', 'sop:approve', 'sop:publish', 'campaign:view', 'member:view', 'directory:view'],
  },
  {
    key: 'job.engineering_lead', name: 'Engineering lead', who: 'CTO, engineering managers',
    does: ['Owns engineering procedures and standards', 'Reviews and approves changes', 'Sees campaign progress'],
    scope: 'Grant for the Engineering department or collection',
    permissions: ['policy:view', 'policy:review', 'policy:approve', 'sop:view', 'sop:create', 'sop:edit_draft', 'sop:review', 'sop:approve', 'sop:publish', 'campaign:view', 'member:view', 'directory:view'],
  },
  {
    key: 'job.manager', name: 'Manager', who: 'Team leads, line managers',
    does: ['Sees their team and its acknowledgements', 'Sees campaign progress'],
    scope: 'Grant for their department',
    permissions: ['member:view', 'directory:view', 'campaign:view', 'work:view_team'],
  },
  {
    key: 'job.communications', name: 'Communications', who: 'Internal comms, office managers',
    does: ['Writes, approves and publishes announcements', 'Sees who has read them'],
    scope: 'Usually the whole workspace',
    permissions: ['announcement:view', 'announcement:create', 'announcement:publish', 'announcement:manage', 'campaign:view', 'directory:view'],
  },
  {
    key: 'job.security_it', name: 'Security & IT', who: 'IT admins, security team',
    does: ['Resets sign-in and two-step for people', 'Reads and exports the audit log', 'Sees files, roles and security settings'],
    scope: 'The whole workspace',
    permissions: ['member:view', 'member:security.manage', 'role:view', 'audit:view', 'audit:export', 'file:view_all', 'monitoring:view_dashboards', 'workspace:settings.view'],
  },
];

/** Pick a job; the role is created (or reported as already there) and the caller offers to grant it. */
export function PresetDialog({ isOpen, roles, onClose, onCreated }: { isOpen: boolean; roles: Role[]; onClose: () => void; onCreated: (role: Role) => void }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { guard, dialog } = useStepUp((e) => { setBusy(null); setError(e instanceof ApiError ? (e.problem.detail ?? e.message) : 'Could not add the role'); });
  const add = (p: Preset) => guard(async () => {
    setError(null); setBusy(p.key);
    const role = await api<Role>('/api/v1/roles', { method: 'POST', body: { key: p.key, name: p.name, permissions: p.permissions } });
    setBusy(null);
    onCreated(role);
  });
  return (
    <>
      <FormDialog isOpen={isOpen} onOpenChange={(o) => !o && onClose()} title="Add a role for a job" width={640}
        description="Each one adds to what every member can already do. You can change its permissions afterwards."
        footer={<Button variant="secondary" onPress={onClose}>Close</Button>}>
        {error && <Notice tone="danger" title="Couldn’t add the role">{error}</Notice>}
        <ul className="presets" aria-label="Roles for common jobs">
          {PRESETS.map((p) => {
            const existing = roles.find((r) => r.key === p.key);
            return (
              <li key={p.key} className="preset">
                <span className="doc-icon tag-purple"><BriefcaseBusiness aria-hidden="true" size="0.875rem" /></span>
                <div className="preset-main">
                  <p className="preset-name">{p.name} <span className="preset-who">· {p.who}</span></p>
                  <ul className="preset-does">{p.does.map((d) => <li key={d}>{d}</li>)}</ul>
                  <p className="preset-scope">{p.scope}</p>
                </div>
                {existing
                  ? <span className="preset-added"><Check aria-hidden="true" size="0.875rem" /> Added</span>
                  : <Button variant="secondary" className="btn-sm" aria-label={`Add ${p.name}`} isDisabled={busy !== null} onPress={() => void add(p)}>{busy === p.key ? 'Adding…' : 'Add'}</Button>}
              </li>
            );
          })}
        </ul>
      </FormDialog>
      {dialog}
    </>
  );
}
