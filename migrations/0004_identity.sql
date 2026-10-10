-- 0004 identity (BPMS-021/022, ADR-0020 JWT + rotating refresh tokens, docs/06 §3.2, §3.6). Global (system-plane) tables: users are
-- global and join workspaces through memberships (docs/12 §2), so these tables carry no workspace_id.

CREATE TABLE users (
  id uuid PRIMARY KEY,
  email citext NOT NULL UNIQUE,
  email_verified_at timestamptz,
  display_name text NOT NULL CHECK (length(display_name) BETWEEN 1 AND 200),
  locale text NOT NULL DEFAULT 'en',
  timezone text NOT NULL DEFAULT 'UTC',
  status text NOT NULL DEFAULT 'active' CHECK (status IN ('active','locked','disabled')),
  subject_ref uuid NOT NULL UNIQUE,            -- pseudonymous id used in audit (docs/14 §2)
  token_version int NOT NULL DEFAULT 1,        -- bump to invalidate all access tokens (deactivation, password reset)
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE user_credentials (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  kind text NOT NULL CHECK (kind IN ('password','totp','webauthn','recovery_code')),
  secret bytea NOT NULL,                        -- PHC string | sealed TOTP secret | SHA-256 of recovery code
  confirmed boolean NOT NULL DEFAULT true,
  last_used_step bigint,                        -- TOTP replay protection
  used_at timestamptz,                          -- recovery codes are single-use
  created_at timestamptz NOT NULL DEFAULT now(),
  last_used_at timestamptz
);
CREATE UNIQUE INDEX user_one_password ON user_credentials (user_id) WHERE kind = 'password';
CREATE UNIQUE INDEX user_one_active_totp ON user_credentials (user_id) WHERE kind = 'totp' AND confirmed;
CREATE INDEX ON user_credentials (user_id, kind);

-- Refresh tokens (opaque, hashed, rotated on every use). A family = one sign-in; reuse of a rotated
-- token revokes the whole family (docs/06 §3.4, SEC-14). Access tokens are stateless JWTs.
CREATE TABLE refresh_tokens (
  token_hash bytea PRIMARY KEY,
  family_id uuid NOT NULL,
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  plane text NOT NULL CHECK (plane IN ('workspace','system')),
  active_workspace_id uuid,
  auth_time timestamptz NOT NULL,
  amr text[] NOT NULL,
  ip inet,
  user_agent text,
  created_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL,              -- sliding (12 h)
  family_expires_at timestamptz NOT NULL,       -- absolute (30 d)
  used_at timestamptz,                          -- rotated
  revoked_at timestamptz
);
CREATE INDEX ON refresh_tokens (family_id);
CREATE INDEX ON refresh_tokens (user_id) WHERE revoked_at IS NULL AND used_at IS NULL;

-- Login throttling per account and per IP, shared by all replicas (docs/06 §3.6).
CREATE TABLE auth_throttle (
  key text PRIMARY KEY,                         -- 'email:<lowercased>' | 'ip:<addr>'
  failures int NOT NULL DEFAULT 0,
  blocked_until timestamptz,
  updated_at timestamptz NOT NULL DEFAULT now()
);
