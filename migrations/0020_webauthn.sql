-- 0020 WebAuthn passkeys as a second factor and for step-up (BPMS-022, docs/06 §3.6, docs/12 §3/§11).
-- Expand-only: nullable columns on the global user_credentials table plus a challenge table. Like
-- 0004, these are system-plane tables (users are global), so they carry no workspace_id.

-- For kind = 'webauthn', `secret` holds the credential's COSE public key (not secret, but the column
-- is shared with the other kinds) and these columns describe the credential.
ALTER TABLE user_credentials
  ADD COLUMN label text CHECK (label IS NULL OR length(label) BETWEEN 1 AND 64),
  ADD COLUMN credential_id bytea CHECK (credential_id IS NULL OR length(credential_id) BETWEEN 16 AND 1023),
  ADD COLUMN sign_count bigint CHECK (sign_count IS NULL OR sign_count BETWEEN 0 AND 4294967295),
  ADD COLUMN transports text[],
  ADD COLUMN backup_eligible boolean,
  ADD COLUMN backup_state boolean,
  ADD COLUMN user_verified boolean,
  ADD CONSTRAINT webauthn_has_credential CHECK (kind <> 'webauthn' OR (credential_id IS NOT NULL AND label IS NOT NULL AND sign_count IS NOT NULL));
-- A credential id is globally unique (the authenticator generates it); one account per credential.
CREATE UNIQUE INDEX user_credentials_webauthn_id ON user_credentials (credential_id) WHERE kind = 'webauthn';

-- Single-use ceremony challenges (WebAuthn §13.4.3): 32 random bytes, 5 minutes, consumed on use.
CREATE TABLE webauthn_challenges (
  id uuid PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  purpose text NOT NULL CHECK (purpose IN ('register','mfa','step_up')),
  challenge bytea NOT NULL UNIQUE CHECK (length(challenge) >= 16),
  created_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL,
  used_at timestamptz
);
CREATE INDEX ON webauthn_challenges (user_id, purpose) WHERE used_at IS NULL;
