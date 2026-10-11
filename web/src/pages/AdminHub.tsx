import { Link } from '../lib/router';
import { useSession } from '../lib/session';
import { adminNav } from '../ui/nav';

/** What each admin page is for, in one line (docs/05 §2.1: explain jargon where it appears). */
const ABOUT: Record<string, string> = {
  overview: 'What needs attention: overdue acknowledgements, reviews and security signals.',
  members: 'Invite people, see who has joined, and manage each person.',
  roles: 'What each role can do, custom roles, and who holds them.',
  people: 'Two-step verification, dormant accounts and privileged access at a glance.',
  templates: 'Starter packs: an employee handbook and policies to adapt and publish.',
  audit: 'Every action, in a tamper-evident record you can filter and export.',
  files: 'Uploads, storage used, malware scan results and how long files are kept.',
  retention: 'How long records are kept, and when drafts and old messages are deleted.',
  encryption: 'The keys that protect this workspace’s data, and their rotation.',
  settings: 'Name, branding, sign-in rules, notifications and other workspace options.',
};

/** Admin (docs/05 §2.1): one door for everything that configures the workspace, grouped. */
export function AdminHub() {
  const { can, workspace } = useSession();
  const groups = adminNav(can);
  return (
    <>
      <div className="page-head">
        <div><h1>Admin</h1><p>Set up and look after {workspace?.name ?? 'this workspace'}. Changes here are recorded in the audit log.</p></div>
      </div>
      <div className="admin-hub">
        {groups.map((g) => (
          <section key={g.label} className="admin-hub-group" aria-labelledby={`hub-${g.label}`}>
            <h2 id={`hub-${g.label}`}>{g.label}</h2>
            <ul>
              {g.items.map((i) => (
                <li key={i.key}>
                  <Link to={i.to} className="admin-hub-item">
                    <span className="admin-hub-icon"><i.icon aria-hidden="true" size="1rem" /></span>
                    <span><span className="admin-hub-title">{i.label}</span><span className="admin-hub-about">{ABOUT[i.key] ?? ''}</span></span>
                  </Link>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </>
  );
}
