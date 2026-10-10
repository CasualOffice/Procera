-- 0007 settings (BPMS-027, docs/20 §1, §4-5). The catalogue lives in code (crates/settings);
-- these tables store only values that differ from the code default.

-- Platform values and floors use catalogue keys; the 0005 seed used provisional names.
DELETE FROM platform_settings;
ALTER TABLE platform_settings ADD COLUMN revision bigint NOT NULL DEFAULT 1;
INSERT INTO platform_settings (key, value, is_floor) VALUES
  ('auth.password.min_length',      '12',                                   true),
  ('auth.password.breach_check',    'true',                                 true),
  ('auth.mfa.required_roles',       '["workspace.owner","workspace.admin"]', true),
  ('auth.mfa.allowed_methods',      '["totp","webauthn"]',                  true),
  ('auth.session.idle_timeout',     '28800',                                true),
  ('auth.session.absolute_timeout', '2592000',                              true),
  ('auth.stepup.window',            '900',                                  true),
  ('api.tokens.max_ttl',            '31536000',                             true),
  ('files.max_size',                '1073741824',                           true),
  ('files.scan.required',           'false',                                true),
  ('access.admin_content_access_log', 'true',                               true);

CREATE TABLE workspace_settings (
  workspace_id uuid NOT NULL,
  key text NOT NULL,
  value jsonb NOT NULL,                          -- validated against the catalogue before write
  revision bigint NOT NULL DEFAULT 1,
  updated_by uuid REFERENCES users(id),          -- NULL = applied from the configuration profile
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, key)
);

-- Immutable change history (docs/20 §4). Platform changes are recorded under the system workspace.
CREATE TABLE settings_revisions (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  layer text NOT NULL CHECK (layer IN ('platform', 'platform_floor', 'workspace', 'profile')),
  key text NOT NULL,
  before jsonb,
  after jsonb NOT NULL,
  reason text,
  author_id uuid REFERENCES users(id),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, id)
);
CREATE INDEX ON settings_revisions (workspace_id, key, created_at DESC);

SELECT procera_enable_workspace_rls('workspace_settings');
SELECT procera_enable_workspace_rls('settings_revisions');

DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'procera_app') THEN
    REVOKE UPDATE, DELETE, TRUNCATE ON settings_revisions FROM procera_app;
  END IF;
END $$;
