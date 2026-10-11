import { ChevronRight } from 'lucide-react';
import { useState } from 'react';
import { Button as AriaButton } from 'react-aria-components';
import { ApiError } from '../lib/api';
import { navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { MfaSetup } from './MfaSetup';

/** Shown when the user belongs to several workspaces (or none). */
export function ChooseWorkspace() {
  const { workspaces, selectWorkspace, signOut, user, entryError } = useSession();
  const [error, setError] = useState<string | null>(null);
  const [needsMfa, setNeedsMfa] = useState<string | null>(entryError === 'mfa_required' && workspaces.length === 1 ? workspaces[0]!.name : null);
  if (needsMfa && !user?.mfa_enabled) return <MfaSetup workspaceName={needsMfa} />;
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
                    if (e instanceof ApiError && e.code === 'mfa_required') setNeedsMfa(w.name);
                    else setError(e instanceof Error ? e.message : 'Could not open the workspace.');
                  }
                }}
              >
                <Avatar name={w.name} tone="brand" square size="2rem" />
                <span className="choice-main"><strong>{w.name}</strong><span>{w.slug}</span></span>
                <ChevronRight aria-hidden="true" size="1rem" className="muted" />
              </AriaButton>
            </li>
          ))}
        </ul>
      )}
      <Button variant="ghost" onPress={async () => { await signOut(); navigate('/sign-in', { replace: true }); }}>Sign out</Button>
    </>
  );
}
