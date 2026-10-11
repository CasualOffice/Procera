import { ApiError } from './api';

/** Directory shapes (docs/08 "Directory"). `member_count` counts active members only. */
export interface Department { id: string; parent_id: string | null; name: string; path: string[]; head_membership_id: string | null; member_count: number }
export interface Group { id: string; name: string; description: string | null; kind: string; member_count: number }
export interface MemberDirectory {
  title: string | null; department_id: string | null; manager_membership_id: string | null; location: string | null;
  field_sources: Record<string, string>; group_ids: string[];
  resolved_manager: { membership_id: string | null; via: 'explicit' | 'department_head' | 'none' };
}

/** "Operations › Support" for a department. */
export function deptPath(id: string | null | undefined, all: Department[]): string | null {
  const d = all.find((x) => x.id === id);
  if (!d) return null;
  return d.path.map((p) => all.find((x) => x.id === p)?.name ?? '…').join(' › ');
}

/** Departments in tree order (each parent followed by its children, siblings by name). */
export function treeOrder(all: Department[]): Department[] {
  const kids = new Map<string | null, Department[]>();
  for (const d of all) kids.set(d.parent_id, [...(kids.get(d.parent_id) ?? []), d]);
  for (const list of kids.values()) list.sort((a, b) => a.name.localeCompare(b.name));
  const out: Department[] = [];
  const walk = (parent: string | null) => { for (const d of kids.get(parent) ?? []) { out.push(d); walk(d.id); } };
  walk(null);
  return out;
}

/** The server's explanation when it has one, else a plain fallback. */
export function problemText(e: unknown, fallback = 'That didn’t work. Try again.'): string {
  return e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : fallback;
}

/** Client-side check that mirrors the server's name rule (1–120 characters, no '/'). */
export function nameProblem(name: string, what: string): string | null {
  const n = name.trim();
  if (!n) return `Give the ${what} a name.`;
  if (n.length > 120) return 'Use 120 characters or fewer.';
  if (n.includes('/')) return 'Names can’t contain “/”.';
  return null;
}
