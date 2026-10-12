import { Building2, Check, MapPin, Shield, Users } from 'lucide-react';
import { useItems } from '../lib/data';
import { useSession } from '../lib/session';

export interface Audience { all: boolean; departments: string[]; groups: string[]; roles: string[]; /** Directory locations (E2E-R1-09). */ locations?: string[] }
export const EVERYONE: Audience = { all: true, departments: [], groups: [], roles: [] };
export const audienceOf = (v: unknown): Audience => ({ ...EVERYONE, all: false, ...(v as Partial<Audience>) });
export const isEmptyAudience = (a: Audience) => !a.all && !a.departments.length && !a.groups.length && !a.roles.length && !(a.locations ?? []).length;

interface Department { id: string; name: string; path: string[]; member_count: number }
interface Group { id: string; name: string; member_count: number }
interface Role { key: string; name: string }
interface Location { name: string; members: number }

/** Directory lookups for audience names; each list loads only with its permission. */
export function useDirectory() {
  const { can } = useSession();
  const [departments] = useItems<Department>(can('directory:view') ? '/api/v1/departments' : null);
  const [groups] = useItems<Group>(can('directory:view') ? '/api/v1/groups' : null);
  const [roles] = useItems<Role>(can('role:view') ? '/api/v1/roles' : null);
  const [locations] = useItems<Location>(can('directory:view') ? '/api/v1/locations' : null);
  return { departments: departments ?? [], groups: groups ?? [], roles: roles ?? [], locations: locations ?? [], loading: departments === null || groups === null || roles === null || locations === null };
}

/** "Everyone", or the chosen names: "Finance, Support and 2 more". */
export function audienceSummary(a: Audience, dir: ReturnType<typeof useDirectory>): string {
  if (a.all) return 'Everyone';
  const names = [
    ...a.departments.map((id) => dir.departments.find((d) => d.id === id)?.name ?? 'a department'),
    ...a.groups.map((id) => dir.groups.find((g) => g.id === id)?.name ?? 'a group'),
    ...a.roles.map((k) => dir.roles.find((r) => r.key === k)?.name ?? k),
    ...(a.locations ?? []),
  ];
  if (!names.length) return 'Nobody yet';
  return names.length <= 2 ? names.join(' and ') : `${names.slice(0, 2).join(', ')} and ${names.length - 2} more`;
}

function Chips<T>({ label, icon: Icon, items, selected, id, name, hint, onToggle }: { label: string; icon: typeof Users; items: T[]; selected: string[]; id: (t: T) => string; name: (t: T) => string; hint?: (t: T) => string; onToggle: (id: string) => void }) {
  if (!items.length) return null;
  return (
    <div className="aud-row" role="group" aria-label={label}>
      <span className="aud-label"><Icon aria-hidden="true" size="0.875rem" /> {label}</span>
      <div className="aud-chips">
        {items.map((t) => {
          const on = selected.includes(id(t));
          return (
            <button key={id(t)} type="button" className="aud-chip" aria-pressed={on} onClick={() => onToggle(id(t))}>
              {on && <Check aria-hidden="true" size="0.75rem" strokeWidth={2.5} />}
              {name(t)}
              {hint && <span className="aud-chip-hint">{hint(t)}</span>}
            </button>
          );
        })}
      </div>
    </div>
  );
}

/**
 * Audience picker (docs/05 §3.23-3.24): everyone, or any mix of departments, groups, roles and locations.
 * A department includes its sub-departments (server rule). Choices are toggle buttons (aria-pressed).
 */
export function AudiencePicker({ value, onChange, reach, legend = 'Audience', summary = 'Sends to' }: { value: Audience; onChange: (a: Audience) => void; reach?: React.ReactNode; legend?: string; summary?: string }) {
  const dir = useDirectory();
  const toggle = (field: 'departments' | 'groups' | 'roles' | 'locations') => (id: string) => {
    const cur = value[field] ?? [];
    onChange({ ...value, all: false, [field]: cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id] });
  };
  const depth = (d: Department) => Math.max(0, d.path.length - 1);
  const noDirectory = !dir.loading && !dir.departments.length && !dir.groups.length && !dir.roles.length && !dir.locations.length;
  return (
    <fieldset className="aud">
      <legend className="field-label">{legend}</legend>
      <div className="segmented aud-mode" role="group" aria-label="Who receives it">
        <button type="button" aria-pressed={value.all} onClick={() => onChange(EVERYONE)}>Everyone</button>
        <button type="button" aria-pressed={!value.all} onClick={() => onChange({ ...value, all: false })}>Specific people</button>
      </div>
      {!value.all && (
        noDirectory ? <p className="aud-note">There are no departments, groups, roles or locations to choose from yet.</p> : (
          <div className="aud-pick">
            <Chips label="Departments" icon={Building2} items={dir.departments} selected={value.departments} id={(d) => d.id} name={(d) => `${'· '.repeat(depth(d))}${d.name}`} hint={(d) => String(d.member_count)} onToggle={toggle('departments')} />
            <Chips label="Groups" icon={Users} items={dir.groups} selected={value.groups} id={(g) => g.id} name={(g) => g.name} hint={(g) => String(g.member_count)} onToggle={toggle('groups')} />
            <Chips label="Roles" icon={Shield} items={dir.roles} selected={value.roles} id={(r) => r.key} name={(r) => r.name} onToggle={toggle('roles')} />
            <Chips label="Locations" icon={MapPin} items={dir.locations} selected={value.locations ?? []} id={(l) => l.name} name={(l) => l.name} hint={(l) => String(l.members)} onToggle={toggle('locations')} />
          </div>
        )
      )}
      <p className="aud-summary" aria-live="polite">{summary}: <strong>{audienceSummary(value, dir)}</strong>{reach}</p>
    </fieldset>
  );
}
