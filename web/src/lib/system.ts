import type { TagColor } from '../ui/Tag';

/** System console types (`/sysapi/v1`, docs/08 §5). Metadata and counts only, never workspace content. */

export interface Quotas {
  max_members: number | null;
  max_active_instances: number | null;
  max_storage_bytes: number | null;
  api_rpm: number | null;
  ai_tokens_month: number | null;
}

export interface Usage { members: number; invited: number; storage_bytes: number | null; last_activity_at: string | null }

export interface ConsoleWorkspace {
  id: string;
  slug: string;
  name: string;
  profile: string;
  state: string;
  isolation_tier: string;
  region: string;
  owner_invite_email: string | null;
  suspended_reason: string | null;
  deletion_due_at: string | null;
  revision: number;
  created_at: string;
  quotas: Quotas;
  usage: Usage;
}

export interface QuotaDefaults extends Quotas { profile: string; revision: number; updated_at: string }

export interface SystemMe { user_id: string; roles: string[] }

export const STATE: Record<string, { label: string; color: TagColor }> = {
  provisioning: { label: 'Provisioning', color: 'blue' },
  active: { label: 'Active', color: 'green' },
  suspended_ro: { label: 'Read-only', color: 'yellow' },
  suspended_locked: { label: 'Locked', color: 'red' },
  pending_deletion: { label: 'Deletion scheduled', color: 'orange' },
  deleted: { label: 'Deleted', color: 'gray' },
};

export const PROFILES = ['starter', 'standard', 'enterprise', 'regulated'] as const;
export const profileLabel = (p: string) => p.charAt(0).toUpperCase() + p.slice(1);

/** Seats in use against the member limit: members plus open invitations (as enforced at invite). */
export function seats(w: ConsoleWorkspace): { used: number; limit: number | null; ratio: number | null } {
  const used = w.usage.members + w.usage.invited;
  const limit = w.quotas.max_members;
  return { used, limit, ratio: limit ? used / limit : null };
}

export function bytes(n: number | null): string {
  if (n === null) return 'Unlimited';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${Number.isInteger(v) ? v : v.toFixed(1)} ${units[i]}`;
}

/** Can this set of system roles do it? Mirrors `platform::authz` (docs/12 §4.1). */
export function sysCan(roles: string[], action: 'workspace.write' | 'audit.read' | 'platform.config'): boolean {
  const admin = roles.some((r) => r === 'system.owner' || r === 'system.admin');
  if (action === 'audit.read') return admin || roles.includes('system.auditor');
  return admin;
}
