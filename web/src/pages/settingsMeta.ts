/**
 * Plain-language labels and placement for the settings catalogue (docs/20 §2, docs/05 §3.16,
 * docs/design/ia-competitive-analysis.md §6). The API returns keys, kinds and values; this file names
 * them for people and decides which category page shows each one. A key missing here still renders,
 * humanised, under its most plausible category's Advanced section.
 */
export type CategoryId = 'general' | 'security' | 'people' | 'documents' | 'acknowledgements' | 'notifications' | 'data';
export interface Category { id: CategoryId; label: string; about: string; sections: string[] }

/** The Settings index (§6.2): one page per category, in this order. */
export const CATEGORIES: Category[] = [
  { id: 'general', label: 'General & branding', about: 'Name, logo, colours, language, time zone and the words Procera uses', sections: ['Branding', 'Language and region'] },
  { id: 'security', label: 'Sign-in & security', about: 'Passwords, two-step verification, session length and admin access logging', sections: ['Sign-in'] },
  { id: 'people', label: 'People & directory', about: 'Who can join, what happens when someone leaves, positions', sections: ['Who can join', 'When someone leaves'] },
  { id: 'documents', label: 'Documents & reviews', about: 'Who must review before publishing, review cycles, numbering', sections: ['Reviews'] },
  { id: 'acknowledgements', label: 'Acknowledgements & announcements', about: 'When people must re-acknowledge, campaign reminders and escalation, announcement rules', sections: ['Acknowledgements', 'Campaigns', 'Announcements'] },
  { id: 'notifications', label: 'Notifications', about: 'Which channels Procera uses, digests and quiet hours', sections: ['Channels and digests', 'Quiet hours'] },
  { id: 'data', label: 'Data, retention & files', about: 'Upload limits, malware scanning, and how long records are kept', sections: ['Files', 'Retention'] },
];
export const categoryById = (id: string) => CATEGORIES.find((c) => c.id === id);

/** Where a key lives: its category, and either a section on that page or the page's Advanced disclosure. */
export interface Place { cat: CategoryId; section?: string; advanced?: boolean }
const B = (cat: CategoryId, section: string): Place => ({ cat, section });
const A = (cat: CategoryId): Place => ({ cat, advanced: true });

/** Every catalogue key, per the §6.2 table (B = everyday, A = under Advanced). */
export const PLACES: Record<string, Place> = {
  'branding.logo': B('general', 'Branding'),
  'branding.accent': B('general', 'Branding'),
  'branding.theme_default': B('general', 'Branding'),
  'branding.email_sender_name': B('general', 'Branding'),
  'locale.default': B('general', 'Language and region'),
  'locale.timezone': B('general', 'Language and region'),
  'locale.week_start': B('general', 'Language and region'),
  'terminology.overrides': A('general'),

  'auth.mfa.required_roles': B('security', 'Sign-in'),
  'auth.password.min_length': B('security', 'Sign-in'),
  'auth.password.breach_check': B('security', 'Sign-in'),
  'auth.session.idle_timeout': B('security', 'Sign-in'),
  'auth.session.absolute_timeout': A('security'),
  'auth.stepup.window': A('security'),
  'auth.mfa.allowed_methods': A('security'),
  'access.admin_content_access_log': A('security'),

  'auth.invite_only': B('people', 'Who can join'),
  'auth.allowed_email_domains': B('people', 'Who can join'),
  'directory.deactivation.reassign_strategy': B('people', 'When someone leaves'),
  'directory.positions.enabled': A('people'),

  'governance.review.required_for': B('documents', 'Reviews'),
  'governance.review.min_reviewers': B('documents', 'Reviews'),
  'governance.review_cadence.default': B('documents', 'Reviews'),
  'governance.reason_for_change.required': B('documents', 'Reviews'),
  'governance.review_cadence.controlled': A('documents'),
  'governance.verification.validity': A('documents'),
  'policies.numbering_pattern': A('documents'),

  'governance.reack.policy': B('acknowledgements', 'Acknowledgements'),
  'governance.reack.window': B('acknowledgements', 'Acknowledgements'),
  'policies.attest_on_join': B('acknowledgements', 'Acknowledgements'),
  'campaign.default_due': B('acknowledgements', 'Campaigns'),
  'campaign.reminders.cadence': B('acknowledgements', 'Campaigns'),
  'campaign.escalation.to_manager_after': B('acknowledgements', 'Campaigns'),
  'campaign.ack_text': B('acknowledgements', 'Campaigns'),
  'campaign.audience.snapshot': A('acknowledgements'),
  'announcements.enabled': B('acknowledgements', 'Announcements'),
  'announcements.approval_required': B('acknowledgements', 'Announcements'),
  'announcements.require_ack.default': B('acknowledgements', 'Announcements'),
  'announcements.expiry.default': B('acknowledgements', 'Announcements'),
  'announcements.channels': B('acknowledgements', 'Announcements'),

  'notify.channels.enabled': B('notifications', 'Channels and digests'),
  'notify.digest.schedule': B('notifications', 'Channels and digests'),
  'notify.quiet_hours.enabled': B('notifications', 'Quiet hours'),
  'notify.quiet_hours.start': B('notifications', 'Quiet hours'),
  'notify.quiet_hours.end': B('notifications', 'Quiet hours'),
  'notify.digest.hour': A('notifications'),
  'notify.critical_override_quiet': A('notifications'),

  'files.max_size': B('data', 'Files'),
  'files.scan.required': B('data', 'Files'),
  'retention.governance_record.keep_for': B('data', 'Retention'),
  'retention.operational.keep_for': B('data', 'Retention'),
  'files.exif_strip': A('data'),
  'files.orphan_purge_after': A('data'),
  'retention.draft.inactive_after': A('data'),
  'retention.draft.notice_before': A('data'),
};

/**
 * Keys for features that haven't shipped (§6.2 "H"): requests, monitoring, delegation, API tokens.
 * They are only left off the screen; their stored values are untouched.
 */
const HIDDEN_PREFIXES = ['exec.', 'monitoring.', 'access.delegation.'];
const HIDDEN_KEYS = new Set(['api.tokens.max_ttl']);
export const isHidden = (key: string) => HIDDEN_KEYS.has(key) || HIDDEN_PREFIXES.some((p) => key.startsWith(p));

/** Delivery channels that don't exist yet: only in-app and email ship today. */
const UNAVAILABLE_CHANNELS = new Set(['push', 'webhook', 'chat']);
/** Built-in roles for modules that haven't shipped (cases, reports): seeded in every workspace, but nothing uses them yet. */
export const UNSHIPPED_ROLES = new Set(['case.manager', 'investigator', 'analyst']);
/** A choice inside a setting that isn't offered (it stays in the stored value if it's already there). */
export function hiddenChoice(key: string, value: string): boolean {
  if (key === 'notify.channels.enabled' || key === 'announcements.channels') return UNAVAILABLE_CHANNELS.has(value);
  if (key === 'auth.mfa.required_roles') return UNSHIPPED_ROLES.has(value);
  return false;
}

/** A key the table doesn't know yet goes to the most plausible category's Advanced section, never dropped. */
const FALLBACK: [string, CategoryId][] = [
  ['branding.', 'general'], ['locale.', 'general'], ['terminology.', 'general'], ['auth.', 'security'], ['api.', 'security'], ['access.', 'security'],
  ['directory.', 'people'], ['governance.', 'documents'], ['policies.', 'documents'], ['campaign.', 'acknowledgements'], ['announcements.', 'acknowledgements'],
  ['notify.', 'notifications'], ['files.', 'data'], ['retention.', 'data'],
];
export const placeOf = (key: string): Place => PLACES[key] ?? A(FALLBACK.find(([p]) => key.startsWith(p))?.[1] ?? 'general');

/** The row anchor for a key, used by search results and old links. */
export const anchorOf = (key: string) => `set-${key.replace(/\./g, '-')}`;

/** Old one-page anchors (`#set-<area>`) → the category page that now holds that area. */
const OLD_AREAS: Record<string, CategoryId> = {
  security: 'security', access: 'security', governance: 'documents', campaigns: 'acknowledgements', announcements: 'acknowledgements',
  notifications: 'notifications', directory: 'people', branding: 'general', locale: 'general', files: 'data', retention: 'data',
};
/** Where an old `/admin/settings#set-…` link should go now, or null to stay on the index. */
export function redirectFor(hash: string): string | null {
  const h = hash.replace(/^#/, '');
  if (!h.startsWith('set-')) return null;
  const area = OLD_AREAS[h.slice(4)];
  if (area) return `/admin/settings/${area}`;
  const key = Object.keys(PLACES).find((k) => anchorOf(k) === h);
  return key && !isHidden(key) ? `/admin/settings/${PLACES[key]!.cat}#${h}` : null;
}

/** Other words people search with (§6.3 rule 5). */
export const SYNONYMS: Record<string, string> = {
  'auth.mfa.required_roles': 'mfa 2fa two-factor two-step authenticator totp',
  'auth.mfa.allowed_methods': 'mfa 2fa two-factor passkey webauthn totp security key',
  'auth.password.min_length': 'password length',
  'auth.password.breach_check': 'password breached pwned',
  'auth.session.idle_timeout': 'session timeout logout log out idle',
  'auth.session.absolute_timeout': 'session timeout logout log out',
  'auth.stepup.window': 'step-up reauthenticate confirm',
  'auth.allowed_email_domains': 'domain sso join email',
  'auth.invite_only': 'invite join signup sign up',
  'access.admin_content_access_log': 'audit logging admin access',
  'retention.governance_record.keep_for': 'retention gdpr delete keep records',
  'retention.operational.keep_for': 'retention gdpr delete logs email',
  'retention.draft.inactive_after': 'retention gdpr delete drafts',
  'retention.draft.notice_before': 'retention gdpr drafts warning',
  'files.scan.required': 'malware virus antivirus',
  'files.max_size': 'upload size limit',
  'files.exif_strip': 'exif gps location privacy photos',
  'terminology.overrides': 'rename words terms vocabulary',
  'locale.default': 'language locale',
  'locale.timezone': 'timezone tz',
  'branding.accent': 'color colour brand',
  'notify.digest.schedule': 'digest summary email',
  'notify.quiet_hours.enabled': 'do not disturb night',
};

/** Outcome-phrased label and a one-line help per key (§6.3 rule 1). */
export const LABELS: Record<string, [string, string?]> = {
  'auth.mfa.required_roles': ['Two-step verification required for', 'People with these roles must use an authenticator app to sign in.'],
  'auth.mfa.allowed_methods': ['Ways people can confirm the second step', 'Which second factors people may set up.'],
  'auth.password.min_length': ['Minimum password length', 'In characters.'],
  'auth.password.breach_check': ['Reject known breached passwords', 'Passwords found in public data breaches can’t be used.'],
  'auth.session.idle_timeout': ['Sign out after inactivity', 'People who haven’t done anything for this long sign in again.'],
  'auth.session.absolute_timeout': ['Sign out after, at the latest', 'Even if the person is active.'],
  'auth.stepup.window': ['Re-confirm identity for sensitive actions after', 'How long a fresh verification counts.'],
  'auth.allowed_email_domains': ['Allowed email domains', 'One per line. Leave empty to allow any domain.'],
  'auth.invite_only': ['Invitation only', 'People can join only through an invitation from an admin.'],
  'api.tokens.max_ttl': ['Longest lifetime for API tokens'],
  'access.delegation.max_depth': ['How far delegation can be passed on', 'Number of hops.'],
  'access.delegation.self_service_ooo': ['People can set their own out-of-office delegate'],
  'access.admin_content_access_log': ['Log when admins open content', 'Each time an admin opens someone else’s content, it’s recorded in the audit log.'],
  'governance.review.required_for': ['Independent review required for', 'Someone other than the author must approve these documents before they’re published.'],
  'governance.review.min_reviewers': ['Approvals needed before publishing', 'How many reviewers must approve.'],
  'governance.review_cadence.default': ['Review documents again every', 'How long a published document stays current before its owner must review it.'],
  'governance.review_cadence.controlled': ['Review controlled documents again every', 'Usually shorter than for other documents.'],
  'governance.verification.validity': ['A verification lasts', 'After this, the document shows as unverified until someone checks it again.'],
  'governance.reack.policy': ['Ask people to acknowledge again', 'Which new versions of a policy people must acknowledge again.'],
  'governance.reack.window': ['Time to acknowledge a new version'],
  'governance.reason_for_change.required': ['Ask what changed when publishing a new version', 'The note is shown in the version history.'],
  'policies.attest_on_join': ['New members acknowledge policies when they join'],
  'policies.numbering_pattern': ['Policy numbering', 'For example POL-{seq:4}.'],
  'campaign.audience.snapshot': ['Who a campaign includes', 'Whether people who join the audience after launch are asked too.'],
  'campaign.reminders.cadence': ['Reminders after launch', 'Days, separated by commas.'],
  'campaign.escalation.to_manager_after': ['Tell their manager after', 'Set to 0 to never escalate.'],
  'campaign.default_due': ['Default time to complete'],
  'campaign.ack_text': ['Acknowledgement statement', 'What people confirm when they acknowledge.'],
  'announcements.enabled': ['Announcements are on'],
  'announcements.approval_required': ['Approval before publishing'],
  'announcements.require_ack.default': ['Ask for acknowledgement by default'],
  'announcements.expiry.default': ['Default time in the feed'],
  'announcements.channels': ['Send announcements', 'Where announcements are delivered besides the feed.'],
  'notify.channels.enabled': ['Send notifications by', 'People can turn channels off for themselves.'],
  'notify.digest.schedule': ['Send a summary email', 'Groups non-urgent notifications into one email. People can change this for themselves.'],
  'notify.digest.hour': ['Send the summary at', 'Hour of the day, 0 to 23.'],
  'notify.quiet_hours.enabled': ['Hold notifications during quiet hours', 'Non-urgent notifications wait until quiet hours end.'],
  'notify.quiet_hours.start': ['Quiet hours start at', 'Hour of the day, 0 to 23.'],
  'notify.quiet_hours.end': ['Quiet hours end at', 'Hour of the day, 0 to 23.'],
  'notify.critical_override_quiet': ['Urgent notifications ignore quiet hours'],
  'directory.positions.enabled': ['Use positions in the directory', 'Positions are seats in the org chart, such as Head of Finance, that a person fills.'],
  'directory.deactivation.reassign_strategy': ['When someone leaves, give their work to'],
  'branding.accent': ['Accent colour'],
  'branding.logo': ['Logo', 'Shown in the sidebar for everyone in this workspace. PNG, JPEG, GIF or WebP, up to 512 KB.'],
  'branding.theme_default': ['Default appearance'],
  'branding.email_sender_name': ['Email sender name', 'Leave empty to use the workspace name.'],
  'locale.default': ['Language', 'A language tag such as en or en-GB.'],
  'locale.timezone': ['Time zone', 'For example Europe/London.'],
  'locale.week_start': ['Week starts on'],
  'terminology.overrides': ['Terminology', 'Rename product terms for this workspace, for example Request to Ticket. Applies to screens, emails and exports; the API keeps its own names.'],
  'files.max_size': ['Largest file people can upload'],
  'files.scan.required': ['Scan uploads for malware'],
  'files.exif_strip': ['Remove location data from photos'],
  'files.orphan_purge_after': ['Delete unattached uploads after', 'Uploads that were never attached to anything.'],
  'retention.governance_record.keep_for': ['Keep governance records for at least', 'Published versions, reviews, acknowledgements and their evidence. Never deleted automatically.'],
  'retention.operational.keep_for': ['Delete delivery logs after', 'Sent emails, read notifications and sent digests.'],
  'retention.draft.inactive_after': ['Delete drafts nobody has edited for', 'Editing a draft resets its clock.'],
  'retention.draft.notice_before': ['Warn the author before deleting a draft'],
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
  off: 'Never', for_workspace_wide: 'For workspace-wide posts', always: 'Always', daily: 'Daily', weekly: 'Weekly',
  fallback: 'The fallback owner', manager: 'Their manager', process_owner: 'The process owner',
  system: 'Follow the device', light: 'Light', dark: 'Dark',
  mon: 'Monday', tue: 'Tuesday', wed: 'Wednesday', thu: 'Thursday', fri: 'Friday', sat: 'Saturday', sun: 'Sunday',
  in_app: 'In the app', email: 'Email', push: 'Push', webhook: 'Webhook', chat: 'Chat',
  totp: 'Authenticator app', webauthn: 'Passkey or security key',
  'workspace.owner': 'Owner', 'workspace.admin': 'Admin', 'security.admin': 'Security admin', 'process.owner': 'Process owner', 'process.designer': 'Process designer',
  'compliance.officer': 'Compliance', 'case.manager': 'Case manager', investigator: 'Investigator', 'team.manager': 'Team manager', analyst: 'Analyst', reviewer: 'Reviewer', member: 'Member',
};

export const LAYER: Record<string, string> = { default: 'Product default', profile: 'Starting point', workspace: 'Set for this workspace', department: 'Set for a department', platform_floor: 'Platform minimum' };

export function humanise(key: string): string {
  const last = key.split('.').slice(-2).join(' ').replace(/_/g, ' ');
  return last.charAt(0).toUpperCase() + last.slice(1);
}
export const labelOf = (key: string): [string, string?] => LABELS[key] ?? [humanise(key)];
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
