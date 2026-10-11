-- 0036 account recovery and admin account controls (OQ-15, A-39, docs/06 §3.6.1, BPMS-021).
-- Global (system-plane) tables like users and refresh_tokens: accounts are global, so these carry no
-- workspace_id. Recovery emails go to the system workspace's mail queue (email_messages with the nil
-- workspace id, sealed under its own KEK, as 0011 planned for platform mail), never a tenant's queue.

-- Password reset links: 32 random bytes, stored only as SHA-256, single use, 30 minutes. A newer
-- request voids older ones (used_at is set on the older rows, with voided = true).
CREATE TABLE password_resets (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash bytea NOT NULL UNIQUE,
  created_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL,
  used_at timestamptz,
  voided boolean NOT NULL DEFAULT false,         -- replaced by a newer link (not used by the person)
  requested_by uuid,                             -- NULL = the person (self-service); else an admin's subject_ref
  requester_ip inet
);
CREATE INDEX password_resets_user_recent ON password_resets (user_id, created_at DESC);

-- An admin's "require a password change at next sign-in". Cleared when the password changes.
ALTER TABLE users ADD COLUMN must_change_password boolean NOT NULL DEFAULT false;

-- The new permission `member:security.manage` (docs/12 §5) for the built-in owner and admin roles of
-- workspaces created before it existed. New workspaces get it from the templates. roles is under
-- forced RLS, so each workspace is updated inside its own scope.
DO $$
DECLARE w uuid;
BEGIN
  FOR w IN SELECT id FROM workspaces LOOP
    PERFORM set_config('app.workspace_id', w::text, true);
    UPDATE roles SET permissions = array_append(permissions, 'member:security.manage'), revision = revision + 1
     WHERE is_builtin AND key IN ('workspace.owner', 'workspace.admin')
       AND NOT ('member:security.manage' = ANY (permissions));
    INSERT INTO access_revisions (workspace_id, revision) VALUES (w, 1)
      ON CONFLICT (workspace_id) DO UPDATE SET revision = access_revisions.revision + 1;
  END LOOP;
  PERFORM set_config('app.workspace_id', '', true);
END $$;
