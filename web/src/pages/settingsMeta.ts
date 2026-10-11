/**
 * Plain-language labels for the settings catalogue (docs/20 §2). The API returns keys, kinds and
 * values; this file only names them for people. A key missing here still renders, humanised.
 */
export interface Area { id: string; label: string; prefixes: string[] }

export const AREAS: Area[] = [
  { id: 'security', label: 'Security and sign-in', prefixes: ['auth.', 'api.'] },
  { id: 'access', label: 'Access', prefixes: ['access.'] },
  { id: 'governance', label: 'Documents and reviews', prefixes: ['governance.', 'policies.'] },
  { id: 'campaigns', label: 'Acknowledgement campaigns', prefixes: ['campaign.'] },
  { id: 'announcements', label: 'Announcements', prefixes: ['announcements.'] },
  { id: 'notifications', label: 'Notifications', prefixes: ['notify.'] },
  { id: 'directory', label: 'Directory', prefixes: ['directory.'] },
  { id: 'branding', label: 'Branding', prefixes: ['branding.'] },
  { id: 'locale', label: 'Language and region', prefixes: ['locale.'] },
  { id: 'files', label: 'Files', prefixes: ['files.'] },
  { id: 'requests', label: 'Requests', prefixes: ['exec.', 'monitoring.'] },
];

export const areaOf = (key: string) => AREAS.find((a) => a.prefixes.some((p) => key.startsWith(p)))?.id ?? 'other';

export const LABELS: Record<string, [string, string?]> = {
  'auth.mfa.required_roles': ['Two-step verification required for', 'People with these roles must use an authenticator app to sign in.'],
  'auth.mfa.allowed_methods': ['Allowed second factors'],
  'auth.password.min_length': ['Minimum password length', 'In characters.'],
  'auth.password.breach_check': ['Reject known breached passwords'],
  'auth.session.idle_timeout': ['Sign out after inactivity'],
  'auth.session.absolute_timeout': ['Sign out after, at the latest', 'Even if the person is active.'],
  'auth.stepup.window': ['Re-confirm identity for sensitive actions after', 'How long a fresh verification counts.'],
  'auth.allowed_email_domains': ['Allowed email domains', 'One per line. Leave empty to allow any domain.'],
  'auth.invite_only': ['Invitation only', 'People can join only through an invitation from an admin.'],
  'api.tokens.max_ttl': ['Longest lifetime for API tokens'],
  'access.delegation.max_depth': ['How far delegation can be passed on', 'Number of hops.'],
  'access.delegation.self_service_ooo': ['People can set their own out-of-office delegate'],
  'access.admin_content_access_log': ['Log when admins open content'],
  'governance.review.required_for': ['Independent review required for'],
  'governance.review.min_reviewers': ['Approvals needed before publishing'],
  'governance.reack.policy': ['Ask people to acknowledge again'],
  'governance.reack.window': ['Time to acknowledge a new version'],
  'governance.reason_for_change.required': ['Ask what changed when publishing a new version'],
  'policies.attest_on_join': ['New members acknowledge policies when they join'],
  'policies.numbering_pattern': ['Policy numbering', 'For example POL-{seq:4}.'],
  'campaign.audience.snapshot': ['Who a campaign includes'],
  'campaign.reminders.cadence': ['Reminders after launch', 'Days, separated by commas.'],
  'campaign.escalation.to_manager_after': ['Tell their manager after', 'Set to 0 to never escalate.'],
  'campaign.default_due': ['Default time to complete'],
  'campaign.ack_text': ['Acknowledgement statement', 'What people confirm when they acknowledge.'],
  'announcements.enabled': ['Announcements are on'],
  'announcements.approval_required': ['Approval before publishing'],
  'announcements.require_ack.default': ['Ask for acknowledgement by default'],
  'announcements.expiry.default': ['Default time in the feed'],
  'announcements.channels': ['Send announcements'],
  'notify.channels.enabled': ['Notification channels in use'],
  'directory.positions.enabled': ['Use positions in the directory'],
  'directory.deactivation.reassign_strategy': ['When someone leaves, give their work to'],
  'branding.accent': ['Accent colour'],
  'branding.theme_default': ['Default appearance'],
  'branding.email_sender_name': ['Email sender name', 'Leave empty to use the workspace name.'],
  'locale.default': ['Language', 'A language tag such as en or en-GB.'],
  'locale.timezone': ['Time zone', 'For example Europe/London.'],
  'locale.week_start': ['Week starts on'],
  'files.max_size': ['Largest file people can upload'],
  'files.scan.required': ['Scan uploads for malware'],
  'files.exif_strip': ['Remove location data from photos'],
  'files.orphan_purge_after': ['Delete unattached uploads after'],
  'exec.request_id_pattern': ['Request numbering', 'For example REQ-{yyyy}-{seq:6}.'],
  'exec.loop.max_iterations': ['Most times a step can repeat'],
  'exec.bulk_approve.allowed': ['Allow approving several requests at once'],
  'exec.cancel.allowed_for_initiator': ['People can cancel their own requests'],
  'monitoring.stuck_instance_threshold': ['Flag a request as stuck after'],
};

/** Human names for enum and set values. */
export const VALUES: Record<string, string> = {
  none: 'Nobody', controlled: 'Controlled documents', all: 'All documents', major_only: 'Major versions only', every_publish: 'Every new version',
  at_launch: 'People in the audience at launch', dynamic: 'Also people who join later',
  off: 'Never', for_workspace_wide: 'For workspace-wide posts', always: 'Always',
  fallback: 'The fallback owner', manager: 'Their manager', process_owner: 'The process owner',
  system: 'Follow the device', light: 'Light', dark: 'Dark',
  mon: 'Monday', tue: 'Tuesday', wed: 'Wednesday', thu: 'Thursday', fri: 'Friday', sat: 'Saturday', sun: 'Sunday',
  in_app: 'In the app', email: 'Email', push: 'Push', webhook: 'Webhook', chat: 'Chat',
  totp: 'Authenticator app', webauthn: 'Passkey or security key',
  'workspace.owner': 'Owner', 'workspace.admin': 'Admin', 'security.admin': 'Security admin', 'process.owner': 'Process owner', 'process.designer': 'Process designer',
  'compliance.officer': 'Compliance', 'case.manager': 'Case manager', investigator: 'Investigator', 'team.manager': 'Team manager', analyst: 'Analyst', reviewer: 'Reviewer', member: 'Member',
};

export const LAYER: Record<string, string> = { default: 'Product default', profile: 'Workspace profile', workspace: 'Set for this workspace', department: 'Set for a department', platform_floor: 'Platform minimum' };

export function humanise(key: string): string {
  const last = key.split('.').slice(-2).join(' ').replace(/_/g, ' ');
  return last.charAt(0).toUpperCase() + last.slice(1);
}
export const valueName = (v: string) => (v === '*' ? 'Everyone' : VALUES[v] ?? v);

const UNITS = [[86_400, 'day'], [3_600, 'hour'], [60, 'minute']] as const;
export function durationText(secs: number): string {
  if (secs === 0) return 'Never';
  for (const [n, u] of UNITS) if (secs % n === 0) { const v = secs / n; return `${v} ${u}${v === 1 ? '' : 's'}`; }
  return `${secs} seconds`;
}
export function sizeText(bytes: number): string {
  const mb = bytes / 1_048_576;
  return mb >= 1024 && mb % 1024 === 0 ? `${mb / 1024} GB` : `${Math.round(mb)} MB`;
}
