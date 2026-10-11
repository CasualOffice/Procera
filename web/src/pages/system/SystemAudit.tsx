import { AuditExplorer } from '../Audit';

/** System · System audit (docs/05 §3.20, FR-S09): the shared explorer, on the system-plane chain. */
export function SystemAudit() {
  return (
    <AuditExplorer source={{
      title: 'System audit',
      lede: 'Workspace lifecycle, quotas, platform settings, system roles and sign-ins, newest first. Records can’t be edited or deleted.',
      events: '/sysapi/v1/system-audit/events',
      verify: '/sysapi/v1/system-audit/verify',
      timeline: (type, id) => `/sysapi/v1/system-audit/events?object_type=${encodeURIComponent(type)}&object_id=${id}&page[size]=200`,
      newestFirst: true,
      actionHint: 'workspace.suspended',
    }} />
  );
}
