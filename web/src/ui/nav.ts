import { Bell, BookOpen, ClipboardCheck, FilePlus2, History, House, Megaphone, ScrollText, Settings, SquarePen, UserPlus, Users, type LucideIcon } from 'lucide-react';

export type NavKey = 'home' | 'inbox' | 'library' | 'announcements' | 'members' | 'manage-announcements' | 'campaigns' | 'settings' | 'audit' | 'none';
export interface NavItem { key: NavKey; label: string; to: string; icon: LucideIcon; keywords?: string }
export interface NavGroup { label: string; items: NavItem[] }

/**
 * Permission-shaped navigation (docs/05 §2.1): items the member cannot use are hidden, not greyed.
 * One source for the sidebar and the command palette's "Go to" commands.
 */
export function navGroups(can: (p: string) => boolean): NavGroup[] {
  const any = (...ps: string[]) => ps.some(can);
  const work: (NavItem | false)[] = [
    { key: 'home', label: 'Home', to: '/', icon: House, keywords: 'start dashboard tasks for you' },
    { key: 'inbox', label: 'Notifications', to: '/notifications', icon: Bell, keywords: 'inbox alerts unread' },
    any('policy:view', 'sop:view') && { key: 'library', label: 'Library', to: '/library', icon: BookOpen, keywords: 'documents policies sops' },
    can('announcement:view') && { key: 'announcements', label: 'Announcements', to: '/announcements', icon: Megaphone, keywords: 'news feed posts' },
  ];
  const admin: (NavItem | false)[] = [
    any('member:invite', 'role:view') && { key: 'members', label: 'Members', to: '/admin/members', icon: Users, keywords: 'people users invite roles' },
  ];
  return [
    { label: 'Workspace', items: work.filter(Boolean) as NavItem[] },
    { label: 'Administration', items: admin.filter(Boolean) as NavItem[] },
  ].filter((g) => g.items.length);
}

export interface CreateAction { id: string; label: string; to: string; icon: LucideIcon; keywords?: string }

/** "Create" commands, permission-shaped (docs/05 §2.1 "(+) New"). */
export function createActions(can: (p: string) => boolean): CreateAction[] {
  return [
    can('policy:create') && { id: 'new-policy', label: 'New policy', to: '/library?new=policy', icon: FilePlus2, keywords: 'create document write' },
    can('sop:create') && { id: 'new-sop', label: 'New SOP', to: '/library?new=sop', icon: ScrollText, keywords: 'create procedure document write' },
    can('member:invite') && { id: 'invite', label: 'Invite people', to: '/admin/members?invite=1', icon: UserPlus, keywords: 'add member user' },
  ].filter(Boolean) as CreateAction[];
}
