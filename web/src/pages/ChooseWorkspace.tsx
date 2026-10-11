import { ChevronRight } from 'lucide-react';
import { useState } from 'react';
import { Button as AriaButton } from 'react-aria-components';
import { ApiError } from '../lib/api';
import { navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';

/** Shown when the user belongs to several workspaces (or none). */
export function ChooseWorkspace() {
  const { workspaces, selectWorkspace, signOut, user } = useSession();
  const [error, setError] = useState<string | null>(null);
  return (
    <>
      <h1>Choose a workspace</h1>
      <p className="lede">Signed in as {user?.email}</p>
      {error && <Notice tone="danger" title={error} />}
      {workspaces.length === 0 ? (
        <Notice tone="info" title="You’re not in a workspace yet">Ask a workspace admin to invite you. The invitation email has a link that adds you.</Notice>
      ) : (
        <ul className="choices">
          {workspaces.map((w) => (
            <li key={w.workspace_id}>
              <AriaButton
                className="choice"
                onPress={async () => {
                  setError(null);
                  try {
                    await selectWorkspace(w.workspace_id);
                    navigate('/', { replace: true });
                  } catch (e) {
                    setError(e instanceof ApiError && e.code === 'mfa_required' ? 'This workspace requires two-step verification for your role. Turn it on, then sign in again.' : e instanceof Error ? e.message : 'Could not open the workspace.');
                  }
                }}
              >
                <Avatar name={w.name} tone="brand" square size={32} />
                <span className="choice-main"><strong>{w.name}</strong><span>{w.slug}</span></span>
                <ChevronRight aria-hidden="true" size={16} className="muted" />
              </AriaButton>
            </li>
          ))}
        </ul>
      )}
      <Button variant="ghost" onPress={async () => { await signOut(); navigate('/sign-in', { replace: true }); }}>Sign out</Button>
    </>
  );
}
