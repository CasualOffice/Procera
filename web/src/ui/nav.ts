import { Archive, Bell, BookOpen, Network, UsersRound, ClipboardCheck, FileCheck, FilePlus2, FileText, Files, Gauge, History, House, KeyRound, LayoutTemplate, Megaphone, ScrollText, Settings, ShieldCheck, SlidersHorizontal, SquarePen, UserCog, UserPlus, Users, type LucideIcon } from 'lucide-react';
import { term } from '../lib/terms';

export type NavKey =
  | 'home' | 'inbox' | 'policies' | 'library' | 'my-documents' | 'announcements' | 'manage-announcements' | 'templates' | 'campaigns'
  | 'admin' | 'overview' | 'members' | 'departments' | 'groups' | 'roles' | 'people' | 'settings' | 'retention' | 'encryption' | 'files' | 'audit' | 'none';
export interface NavItem { key: NavKey; label: string; to: string; icon: LucideIcon; keywords?: string }
export interface NavGroup { label: string; items: NavItem[] }

const pick = (xs: (NavItem | false)[]) => xs.filter(Boolean) as NavItem[];

/**
 * Everyday sidebar (docs/05 §2.1, approved 2026-10-12): three to six items, permission-shaped.
 * Configuration lives behind the single Admin entry.
 */
export function workNav(can: (p: string) => boolean): NavItem[] {
  const any = (...ps: string[]) => ps.some(can);
  return pick([
    { key: 'home', label: 'Home', to: '/', icon: House, keywords: 'start dashboard tasks needs you' },
    any('policy:view', 'sop:view') && { key: 'library', label: 'Library', to: '/library', icon: BookOpen, keywords: 'documents policies sops applies to me' },
    can('announcement:view') && { key: 'announcements', label: 'Announcements', to: '/announcements', icon: Megaphone, keywords: 'news feed posts' },
    any('policy:create', 'sop:create') && { key: 'my-documents', label: 'My documents', to: '/library?view=mine', icon: FileText, keywords: 'drafts owned in review mine' },
    can('campaign:view') && { key: 'campaigns', label: 'Campaigns', to: '/admin/campaigns', icon: ClipboardCheck, keywords: 'acknowledgement attestation evidence' },
    adminNav(can).length > 0 && { key: 'admin', label: 'Admin', to: '/admin', icon: SlidersHorizontal, keywords: 'administration settings people configure' },
  ]);
}

/** Admin mode (docs/05 §2.1): grouped, two levels at most. */
export function adminNav(can: (p: string) => boolean): NavGroup[] {
  const any = (...ps: string[]) => ps.some(can);
  return [
    { label: 'Overview', items: pick([
      can('monitoring:view_dashboards') && { key: 'overview', label: 'Overview', to: '/admin/overview', icon: Gauge, keywords: 'dashboard attention overdue security' },
    ]) },
    { label: 'People', items: pick([
      any('member:invite', 'role:view') && { key: 'members', label: 'Members', to: '/admin/members', icon: Users, keywords: 'people users invite' },
      can('directory:manage') && { key: 'departments', label: 'Departments', to: '/admin/departments', icon: Network, keywords: 'org chart teams hierarchy head structure' },
      can('directory:manage') && { key: 'groups', label: 'Groups', to: '/admin/groups', icon: UsersRound, keywords: 'sets teams fire wardens auditors audience' },
      can('role:view') && { key: 'roles', label: 'Roles & access', to: '/admin/roles', icon: UserCog, keywords: 'permissions custom role grant scope expiry rbac' },
      can('monitoring:view_dashboards') && can('member:view') && { key: 'people', label: 'People & access', to: '/admin/people', icon: ShieldCheck, keywords: 'mfa two-step dormant admins grants security' },
    ]) },
    { label: 'Content & communication', items: pick([
      can('template:install') && { key: 'templates', label: 'Templates', to: '/templates', icon: LayoutTemplate, keywords: 'gallery starter pack handbook install' },
    ]) },
    { label: 'Data & security', items: pick([
      can('audit:view') && { key: 'audit', label: 'Audit log', to: '/admin/audit', icon: History, keywords: 'history events trail who changed' },
      can('file:view_all') && { key: 'files', label: 'Files', to: '/admin/files', icon: Files, keywords: 'uploads attachments storage quota quarantine malware' },
      can('workspace:settings.view') && { key: 'retention', label: 'Retention', to: '/admin/retention', icon: Archive, keywords: 'records keep delete drafts logs disposition' },
      can('workspace:settings.view') && { key: 'encryption', label: 'Encryption', to: '/admin/encryption', icon: KeyRound, keywords: 'keys rotate kek security crypto' },
    ]) },
    { label: 'Workspace', items: pick([
      can('workspace:settings.view') && { key: 'settings', label: 'Settings', to: '/admin/settings', icon: Settings, keywords: 'configuration name branding security preferences' },
    ]) },
  ].filter((g) => g.items.length);
}

const ADMIN_KEYS = new Set<NavKey>(['admin', 'overview', 'members', 'departments', 'groups', 'roles', 'people', 'templates', 'audit', 'files', 'retention', 'encryption', 'settings']);
/** Pages that show the admin sidebar. */
export const isAdminPage = (key: NavKey) => ADMIN_KEYS.has(key);

/**
 * Every destination for the command palette's "Go to": the sidebar plus pages reached through tabs
 * (Notifications, Applies to me, Manage announcements, My activity).
 */
export function navGroups(can: (p: string) => boolean): NavGroup[] {
  const extra = pick([
    { key: 'inbox', label: 'Notifications', to: '/notifications', icon: Bell, keywords: 'inbox alerts unread bell' },
    can('policy:view') && { key: 'policies', label: 'Policies that apply to me', to: '/policies', icon: FileCheck, keywords: 'rules handbook what changed applies to me' },
    (can('announcement:create') || can('announcement:publish') || can('announcement:manage')) && { key: 'manage-announcements', label: 'Manage announcements', to: '/admin/announcements', icon: SquarePen, keywords: 'compose write publish approve posts' },
    !can('audit:view') && can('audit:view_own') && { key: 'audit', label: 'My activity', to: '/admin/audit', icon: History, keywords: 'history my events audit' },
  ]);
  return [{ label: 'Workspace', items: [...workNav(can), ...extra] }, ...adminNav(can)].filter((g) => g.items.length);
}

export interface CreateAction { id: string; label: string; to: string; icon: LucideIcon; keywords?: string }

/** "Create" commands, permission-shaped (docs/05 §2.1 "(+) New"). */
export function createActions(can: (p: string) => boolean): CreateAction[] {
  return [
    can('policy:create') && { id: 'new-policy', label: 'New policy', to: '/library?new=policy', icon: FilePlus2, keywords: 'create document write' },
    can('sop:create') && { id: 'new-sop', label: `New ${term('sop')}`, to: '/library?new=sop', icon: ScrollText, keywords: 'create procedure document write' },
    can('template:install') && { id: 'from-template', label: 'Start from a template', to: '/templates', icon: LayoutTemplate, keywords: 'create starter pack handbook' },
    (can('announcement:create') || can('announcement:manage')) && { id: 'new-announcement', label: 'New announcement', to: '/admin/announcements?new=1', icon: SquarePen, keywords: 'compose post write' },
    can('campaign:manage') && { id: 'new-campaign', label: 'New campaign', to: '/admin/campaigns?new=1', icon: ClipboardCheck, keywords: 'acknowledgement attestation' },
    can('member:invite') && { id: 'invite', label: 'Invite people', to: '/admin/members?invite=1', icon: UserPlus, keywords: 'add member user' },
  ].filter(Boolean) as CreateAction[];
}
