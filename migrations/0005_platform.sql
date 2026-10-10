-- 0005 system plane (BPMS-020, FR-S01..S05, S09; docs/12 §2, §4.1). These tables belong to the system
-- plane and are managed across workspaces, so they deliberately do NOT use a `workspace_id` column
-- (that name is reserved for RLS-isolated workspace data; the guard view enforces it).

CREATE TABLE system_role_grants (
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role text NOT NULL CHECK (role IN ('system.owner','system.admin','system.operator','system.auditor')),
  granted_by uuid REFERENCES users(id),
  granted_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, role)
);

-- One-time install bootstrap tokens (FR-S01): single use, 24 h, stored hashed.
CREATE TABLE bootstrap_tokens (
  token_hash bytea PRIMARY KEY,
  created_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL,
  used_at timestamptz,
  used_by uuid REFERENCES users(id)
);

CREATE TABLE workspaces (
  id uuid PRIMARY KEY,
  slug citext NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,61}[a-z0-9]$'),
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
  profile text NOT NULL CHECK (profile IN ('starter','standard','enterprise','regulated')),
  state text NOT NULL CHECK (state IN ('provisioning','active','suspended_ro','suspended_locked','pending_deletion','deleted')),
  isolation_tier text NOT NULL DEFAULT 'shared' CHECK (isolation_tier IN ('shared','dedicated_schema','dedicated_db')),
  region text NOT NULL DEFAULT 'default',
  owner_invite_email citext,                    -- first owner, until the invite is accepted (BPMS-023)
  suspended_reason text,
  deletion_due_at timestamptz,
  revision bigint NOT NULL DEFAULT 1,
  created_at timestamptz NOT NULL DEFAULT now(),
  created_by uuid NOT NULL REFERENCES users(id)
);
CREATE INDEX ON workspaces (state);

CREATE TABLE workspace_quotas (
  target_workspace_id uuid PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
  max_members int CHECK (max_members > 0),
  max_active_instances int CHECK (max_active_instances > 0),
  max_storage_bytes bigint CHECK (max_storage_bytes > 0),
  api_rpm int CHECK (api_rpm > 0),
  ai_tokens_month bigint CHECK (ai_tokens_month >= 0),
  soft_ratio numeric(3,2) NOT NULL DEFAULT 0.80 CHECK (soft_ratio > 0 AND soft_ratio <= 1),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE platform_settings (
  key text PRIMARY KEY,
  value jsonb NOT NULL,
  is_floor boolean NOT NULL DEFAULT false,      -- workspaces may tighten but never loosen (FR-S05)
  updated_by uuid REFERENCES users(id),
  updated_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO platform_settings (key, value, is_floor) VALUES
  ('auth.password_min_length', '12', true),
  ('auth.mfa_required_for_workspace_admins', 'true', true),
  ('auth.access_token_ttl_seconds', '600', false),
  ('auth.refresh_absolute_days', '30', true);
