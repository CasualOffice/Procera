import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { ApiError, api, authPost, refresh, setAccessToken } from './api';
import { getPasskey } from './passkey';
import { navigate } from './router';

export interface User {
  id: string;
  email: string;
  display_name: string;
  mfa_enabled: boolean;
  /** Enrolled second factors: `totp`, `webauthn`. */
  mfa_methods: string[];
}
export interface Workspace {
  workspace_id: string;
  slug: string;
  name: string;
  state: string;
}

type Status = 'loading' | 'signed_out' | 'choose_workspace' | 'ready';

/** What a sign-in step asks for next; empty when signed in. */
export interface SignInStep { mfaToken?: string; methods?: string[]; changeToken?: string }
type LoginResponse = { status: string; access_token?: string; mfa_token?: string; methods?: string[]; change_token?: string };

interface Session {
  status: Status;
  user: User | null;
  workspaces: Workspace[];
  workspace: Workspace | null;
  permissions: Set<string>;
  /** My membership in the current workspace. */
  membershipId: string | null;
  /** Why entering the workspace failed (e.g. `mfa_required`), shown on the chooser. */
  entryError: string | null;
  can: (permission: string) => boolean;
  /**
   * Returns the MFA token and the accepted factors when a second factor is needed, or a change token
   * when an administrator requires a new password first (docs/06 §3.6.1).
   */
  signIn: (email: string, password: string) => Promise<SignInStep>;
  /** Sets the required new password with the change token; continues like a password sign-in. */
  completePasswordChange: (changeToken: string, password: string) => Promise<SignInStep>;
  completeMfa: (mfaToken: string, code: string) => Promise<void>;
  /** Second step with a passkey (throws `PasskeyCancelled` if the prompt is dismissed). */
  completeMfaWithPasskey: (mfaToken: string) => Promise<void>;
  selectWorkspace: (id: string) => Promise<void>;
  signOut: () => Promise<void>;
  reload: () => Promise<void>;
}

const Ctx = createContext<Session | null>(null);

export function useSession(): Session {
  const s = useContext(Ctx);
  if (!s) throw new Error('useSession outside SessionProvider');
  return s;
}

export function SessionProvider({ children }: { children: React.ReactNode }) {
  const [status, setStatus] = useState<Status>('loading');
  const [user, setUser] = useState<User | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [permissions, setPermissions] = useState<Set<string>>(new Set());
  const [membershipId, setMembershipId] = useState<string | null>(null);
  const autoSelected = useRef(false);
  const [entryError, setEntryError] = useState<string | null>(null);

  /** Loads who I am and where I am from the current access token. */
  const load = useCallback(async () => {
    const me = await api<{ user: User; token: { workspace: string | null } }>('/api/v1/me');
    const mine = await api<{ items: Workspace[] }>('/api/v1/me/workspaces');
    setUser(me.user);
    setWorkspaces(mine.items);
    const current = mine.items.find((w) => w.workspace_id === me.token.workspace) ?? null;
    if (current) {
      try {
        const p = await api<{ permissions: string[]; membership_id: string }>('/api/v1/me/permissions');
        setPermissions(new Set(p.permissions));
        setMembershipId(p.membership_id);
        setEntryError(null);
        setWorkspace(current);
        setStatus('ready');
        return;
      } catch (e) {
        if (!(e instanceof ApiError)) throw e;
        setEntryError(e.code);
        // e.g. mfa_required for this role: fall through to the chooser, which explains it.
      }
    }
    setWorkspace(null);
    setPermissions(new Set());
    setMembershipId(null);
    const only = mine.items.length === 1 ? mine.items[0] : undefined;
    if (only && !current && !autoSelected.current) {
      autoSelected.current = true; // once: if entry fails (e.g. MFA required), stay on the chooser
      await select(only.workspace_id);
      return;
    }
    setStatus('choose_workspace');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const select = useCallback(
    async (id: string) => {
      const r = await api<{ access_token: string }>('/api/v1/session/workspace', { method: 'POST', body: { workspace_id: id } });
      setAccessToken(r.access_token);
      await load();
    },
    [load],
  );

  // The workspace became read-only or locked while signed in (a 423 from the API): re-read it so
  // the shell shows why, instead of each action failing on its own.
  useEffect(() => {
    let last = 0; // a locked workspace answers 423 to the reload itself: at most one re-read per 5 s
    const on = () => { if (Date.now() - last < 5000) return; last = Date.now(); void load().catch(() => undefined); };
    window.addEventListener('procera:workspace-state', on);
    return () => window.removeEventListener('procera:workspace-state', on);
  }, [load]);

  useEffect(() => {
    (async () => {
      try {
        if (await refresh()) await load();
        else setStatus('signed_out');
      } catch {
        setStatus('signed_out');
      }
    })();
  }, [load]);

  /** The next step of a sign-in, or the session when it is complete. */
  const next = useCallback(
    async (r: LoginResponse): Promise<SignInStep> => {
      if (r.status === 'mfa_required') return { mfaToken: r.mfa_token, methods: r.methods ?? [] };
      if (r.status === 'password_change_required') return { changeToken: r.change_token };
      autoSelected.current = false;
      setAccessToken(r.access_token ?? null);
      await load();
      return {};
    },
    [load],
  );

  const value = useMemo<Session>(
    () => ({
      status,
      user,
      workspaces,
      workspace,
      permissions,
      membershipId,
      entryError,
      can: (p) => permissions.has(p),
      async signIn(email, password) {
        return next(await authPost<LoginResponse>('/api/v1/auth/login', { email, password }));
      },
      async completePasswordChange(changeToken, password) {
        return next(await authPost<LoginResponse>('/api/v1/auth/password/expired', { change_token: changeToken, password }));
      },
      async completeMfa(mfaToken, code) {
        const r = await authPost<{ access_token: string }>('/api/v1/auth/mfa', { mfa_token: mfaToken, code: code.trim() });
        autoSelected.current = false;
        setAccessToken(r.access_token);
        await load();
      },
      async completeMfaWithPasskey(mfaToken) {
        const options = await authPost<{ public_key: Record<string, unknown> }>('/api/v1/auth/mfa/webauthn/options', { mfa_token: mfaToken });
        const credential = await getPasskey(options);
        const r = await authPost<{ access_token: string }>('/api/v1/auth/mfa', { mfa_token: mfaToken, credential });
        autoSelected.current = false;
        setAccessToken(r.access_token);
        await load();
      },
      async selectWorkspace(id) {
        await select(id);
        // Entering can still fail after the token switch (e.g. MFA required for the role): surface it.
        const p = await api<{ permissions: string[] }>('/api/v1/me/permissions').catch((e: unknown) => e);
        if (p instanceof ApiError) throw p;
      },
      async signOut() {
        await authPost('/api/v1/auth/logout').catch(() => undefined);
        setAccessToken(null);
        setUser(null);
        setWorkspace(null);
        setWorkspaces([]);
        setPermissions(new Set());
    setMembershipId(null);
        setEntryError(null);
        autoSelected.current = false;
        // A deliberate sign-out doesn't come back to this page afterwards.
        navigate('/sign-in', { replace: true });
        setStatus('signed_out');
      },
      reload: load,
    }),
    [status, user, workspaces, workspace, permissions, membershipId, entryError, load, select, next],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
