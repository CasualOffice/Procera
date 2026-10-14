import { ApiError } from './api';
import { TERMS, type Term, term } from './terms';

/** Roles and grants as the Access API returns them (docs/08 Roles, docs/12 §5-6). */
export interface Role { id: string; key: string; name: string; description: string | null; is_builtin: boolean; builtin_template: string | null; permissions: string[]; revision: number }
export type Scope = { kind: 'workspace' } | { kind: 'department' | 'collection' | 'process' | 'case'; id: string };
export interface Grant { id: string; role_key: string; membership_id: string | null; group_id: string | null; scope: Scope; expires_at: string | null; reason: string | null }
export interface Catalog { permissions: string[]; templates: { key: string; name: string; description: string; permissions: string[] }[] }
export interface RolePreview { added: string[]; removed: string[]; affected_members: string[] }

const AREA: Record<string, string> = {
  workspace: 'Workspace', member: 'Members', role: 'Roles', directory: 'Directory', collection: 'Collections', process: 'Processes',
  sop: 'SOPs', policy: 'Policies', announcement: 'Announcements', campaign: 'Campaigns', document: 'Documents and signing',
  form: 'Forms', rule: 'Rules', calendar: 'Calendars', template: 'Templates', request: 'Requests', work: 'Work', case: 'Cases',
  control: 'Controls', attestation: 'Attestations', report: 'Reports', connector: 'Connectors', file: 'Files', audit: 'Audit',
  monitoring: 'Monitoring', api_token: 'API tokens', ai: 'Assistant', time_off: 'Time off',
};
const ACTION: Record<string, string> = {
  'settings.view': 'View settings', 'settings.manage': 'Change settings', 'branding.manage': 'Change branding', 'billing.view': 'View billing',
  'delete.request': 'Request deletion', view_own: 'View own', view_all: 'View all', manage_roles: 'Grant roles', 'security.manage': 'Manage account security', edit_draft: 'Edit drafts',
  submit_review: 'Submit for review', manage_owners: 'Manage owners', view_analytics: 'View analytics', send_for_signature: 'Send for signature',
  publish_workspace_template: 'Publish workspace templates', suspend_resume: 'Suspend and resume', view_detail: 'View details',
  view_restricted: 'View restricted fields', view_assigned: 'View assigned', view_team: 'View team', add_evidence: 'Add evidence',
  view_evidence: 'View evidence', manage_campaign: 'Manage campaigns', manage_dashboards: 'Manage dashboards', view_dashboards: 'View dashboards',
  manage_alerts: 'Manage alerts', manage_secrets: 'Manage secrets', manage_retention: 'Manage retention', manage_quarantine: 'Manage quarantine',
  use_assistant: 'Use the assistant', manage_policies: 'Manage policies', adjust_balances: 'Adjust balances', manage_settings: 'Manage settings', investigate: 'Investigate',
};

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
export const areaOf = (p: string) => p.slice(0, p.indexOf(':'));
/** Areas named after a product term (SOPs, Requests, Cases…) use the workspace's term (CFG-008). */
export const areaLabel = (area: string) => ((TERMS as string[]).includes(area) ? term(area as Term, { plural: true, capitalised: true }) : AREA[area] ?? cap(area.replace(/_/g, ' ')));
export const actionLabel = (p: string) => { const a = p.slice(p.indexOf(':') + 1); return ACTION[a] ?? cap(a.replace(/[._]/g, ' ')); };
/** "Processes: Publish" — a permission in words, for previews and errors. */
export const permLabel = (p: string) => (p.includes(':') ? `${areaLabel(areaOf(p))}: ${actionLabel(p)}` : p);

/** The catalogue grouped by area, in catalogue order. */
export function groupByArea(perms: string[]): { area: string; label: string; perms: string[] }[] {
  const out = new Map<string, string[]>();
  for (const p of perms) out.set(areaOf(p), [...(out.get(areaOf(p)) ?? []), p]);
  return [...out].map(([area, ps]) => ({ area, label: areaLabel(area), perms: ps }));
}

/** Permissions only owners hold (in the owner template but not the admin one); custom roles can't include them. */
export function ownerOnly(c: Catalog | null): Set<string> {
  const owner = c?.templates.find((t) => t.key === 'workspace.owner')?.permissions ?? [];
  const admin = new Set(c?.templates.find((t) => t.key === 'workspace.admin')?.permissions ?? []);
  return new Set(owner.filter((p) => !admin.has(p)));
}

/** A role key from its name: the API wants 2-64 chars of a-z, 0-9, '_', '.', '-' that isn't a built-in key. */
export const roleKey = (name: string) => `custom.${name.toLowerCase().normalize('NFKD').replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '')}`.slice(0, 64);

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
export const people = (n: number) => plural(n, 'person', 'people');

/** The preview in plain language: "3 people gain …", "3 people lose …". */
export function previewSentences(p: RolePreview): string[] {
  const n = p.affected_members.length;
  if (!p.added.length && !p.removed.length) return ['Nothing changes.'];
  const lines: string[] = [];
  if (p.added.length) lines.push(`${people(n)} ${n === 1 ? 'gains' : 'gain'} ${plural(p.added.length, 'permission')}: ${p.added.map(permLabel).join(', ')}.`);
  if (p.removed.length) lines.push(`${people(n)} ${n === 1 ? 'loses' : 'lose'} ${plural(p.removed.length, 'permission')}: ${p.removed.map(permLabel).join(', ')}.`);
  if (!n) lines.push('Nobody holds this role yet, so no one’s access changes now.');
  return lines;
}

/** A server refusal in words the admin can act on. */
export function accessErrorText(e: unknown, fallback = 'That didn’t work. Try again.'): string {
  if (!(e instanceof ApiError)) return e instanceof Error ? e.message : fallback;
  const list = (d?: string) => (d ?? '').split(',').map((s) => s.trim()).filter(Boolean).map(permLabel).join(', ');
  switch (e.code) {
    case 'escalation': return `You can’t give permissions you don’t have yourself: ${list(e.problem.detail)}.`;
    case 'role_exists': return 'A role with this name already exists. Choose another name.';
    case 'builtin_immutable': return 'Built-in roles can’t be changed. Duplicate it to make a custom role.';
    case 'last_owner': return 'This is the only workspace owner. Make someone else an owner first.';
    case 'unknown_permission': return `This permission can’t be added to a custom role: ${list(e.problem.detail)}.`;
    case 'unknown_role': return 'That role no longer exists. Reload the page.';
    case 'not_found': return 'It no longer exists. Reload the page.';
    default: return e.problem.detail ?? e.problem.title ?? fallback;
  }
}
