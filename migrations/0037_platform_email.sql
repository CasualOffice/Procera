-- 0037 platform email delivery settings (BPMS-065, CFG-006; docs/20 §3 "Email", docs/06 §4).
-- One row, on the system plane: the SMTP relay the delivery worker uses when PROCERA_SMTP_URL is not
-- set (the environment always wins). The row carries the nil (system) workspace id so it sits under
-- forced RLS like every other table with workspace_id, and its password is envelope-sealed under the
-- system workspace's KEK (AAD platform_email_config:<id>); it is registered with the key rewrap job
-- (procera_keys::REGISTRY) so a KEK rotation never strands it. No API ever returns the password.

CREATE TABLE platform_email_config (
  workspace_id uuid NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000'
    CHECK (workspace_id = '00000000-0000-0000-0000-000000000000'),
  id uuid NOT NULL,
  host text NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
  port int NOT NULL CHECK (port BETWEEN 1 AND 65535),
  -- none is accepted for a relay on this machine only (validated in the service as well).
  tls_mode text NOT NULL CHECK (tls_mode IN ('starttls_required', 'implicit_tls', 'none')),
  username text CHECK (length(username) <= 320),
  password_sealed bytea,
  from_address text NOT NULL CHECK (length(from_address) BETWEEN 3 AND 320),
  from_name text CHECK (length(from_name) <= 100),
  reply_to text CHECK (length(reply_to) <= 320),
  revision bigint NOT NULL DEFAULT 1,
  updated_by uuid REFERENCES users(id),
  updated_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id),
  CHECK (password_sealed IS NULL OR username IS NOT NULL)
);
SELECT procera_enable_workspace_rls('platform_email_config');
