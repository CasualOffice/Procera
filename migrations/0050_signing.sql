-- 0050 (BPMS-107, FR-DS01–07, docs/04 §2.7 envelope protocol): employee e-signing, v1.
-- An envelope is one PDF sent to members who sign it in order or in parallel (CC recipients only get
-- the completed copy). On send the document's SHA-256 and page sizes are frozen with its fields;
-- every signer is shown that exact file and their signature records its hash (ESIG-02). When the last
-- signer finishes, the server stamps the values onto the PDF, appends a certificate of completion and
-- stores the result (completed_file_id, completed_hash). Signatures are immutable.

CREATE TABLE envelopes (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  title text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
  message text CHECK (length(message) <= 2000),
  source_file_id uuid NOT NULL,
  document_hash bytea NOT NULL,                 -- SHA-256 of the PDF signers see
  pages jsonb NOT NULL,                         -- [[width, height], …] in PDF points
  order_mode text NOT NULL CHECK (order_mode IN ('sequential', 'parallel')),
  mode text NOT NULL DEFAULT 'simple' CHECK (mode IN ('simple', 'strong')),
  state text NOT NULL CHECK (state IN ('draft', 'sent', 'completed', 'declined', 'voided', 'expired')),
  created_by uuid NOT NULL,
  created_at timestamptz NOT NULL,
  sent_at timestamptz,
  expires_at timestamptz,
  completed_at timestamptz,
  completed_file_id uuid,
  completed_hash bytea,
  ended_reason text,                            -- why it was declined or voided
  revision int NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, source_file_id) REFERENCES files (workspace_id, id),
  FOREIGN KEY (workspace_id, completed_file_id) REFERENCES files (workspace_id, id),
  FOREIGN KEY (workspace_id, created_by) REFERENCES memberships (workspace_id, id)
);
SELECT procera_enable_workspace_rls('envelopes');
CREATE INDEX envelopes_by_creator ON envelopes (workspace_id, created_by, created_at DESC);

CREATE TABLE envelope_recipients (
  workspace_id uuid NOT NULL,
  envelope_id uuid NOT NULL,
  id uuid NOT NULL,
  ord int NOT NULL CHECK (ord >= 1),
  membership_id uuid NOT NULL,
  role text NOT NULL CHECK (role IN ('signer', 'cc')),
  state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'signed', 'declined')),
  notified_at timestamptz,
  viewed_at timestamptz,
  consented_at timestamptz,
  signed_at timestamptz,
  declined_at timestamptz,
  decline_reason text,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, envelope_id, membership_id),
  FOREIGN KEY (workspace_id, envelope_id) REFERENCES envelopes (workspace_id, id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id)
);
SELECT procera_enable_workspace_rls('envelope_recipients');
CREATE INDEX envelope_recipients_by_member ON envelope_recipients (workspace_id, membership_id);

CREATE TABLE envelope_fields (
  workspace_id uuid NOT NULL,
  envelope_id uuid NOT NULL,
  id uuid NOT NULL,
  recipient_id uuid NOT NULL,
  kind text NOT NULL CHECK (kind IN ('signature', 'initials', 'date_signed', 'name', 'text', 'checkbox')),
  page int NOT NULL CHECK (page >= 1),
  -- the field's box as fractions of the page, from its top-left corner
  x double precision NOT NULL CHECK (x >= 0 AND x < 1),
  y double precision NOT NULL CHECK (y >= 0 AND y < 1),
  w double precision NOT NULL CHECK (w > 0 AND x + w <= 1.0000001),
  h double precision NOT NULL CHECK (h > 0 AND y + h <= 1.0000001),
  required boolean NOT NULL DEFAULT true,
  label text CHECK (length(label) <= 100),
  value jsonb,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, envelope_id) REFERENCES envelopes (workspace_id, id),
  FOREIGN KEY (workspace_id, recipient_id) REFERENCES envelope_recipients (workspace_id, id)
);
SELECT procera_enable_workspace_rls('envelope_fields');

CREATE TABLE signatures (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  envelope_id uuid NOT NULL,
  recipient_id uuid NOT NULL,
  signer_subject_ref uuid NOT NULL,
  record_hash bytea NOT NULL,                   -- the document hash the signer was shown (ESIG-02)
  meaning text NOT NULL,
  mode text NOT NULL CHECK (mode IN ('simple', 'strong')),
  auth_method text NOT NULL,
  auth_time timestamptz NOT NULL,
  adopted_kind text NOT NULL CHECK (adopted_kind IN ('typed', 'drawn')),
  adopted_text text NOT NULL,                   -- the typed signature, or the signer's name for a drawn one
  adopted_image bytea,                          -- a drawn signature (PNG)
  ip text,
  user_agent text,
  signed_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, recipient_id),
  FOREIGN KEY (workspace_id, envelope_id) REFERENCES envelopes (workspace_id, id),
  FOREIGN KEY (workspace_id, recipient_id) REFERENCES envelope_recipients (workspace_id, id)
);
SELECT procera_enable_workspace_rls('signatures');

-- Evidence is frozen: signatures never change; a sent envelope keeps its document and fields.
CREATE FUNCTION procera_signature_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'signatures are immutable';
END $$;
CREATE TRIGGER signatures_immutable BEFORE UPDATE OR DELETE ON signatures
  FOR EACH ROW EXECUTE FUNCTION procera_signature_immutable();

CREATE FUNCTION procera_envelope_frozen() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF OLD.state <> 'draft' AND (NEW.source_file_id IS DISTINCT FROM OLD.source_file_id
      OR NEW.document_hash IS DISTINCT FROM OLD.document_hash OR NEW.pages IS DISTINCT FROM OLD.pages
      OR NEW.order_mode IS DISTINCT FROM OLD.order_mode OR NEW.mode IS DISTINCT FROM OLD.mode) THEN
    RAISE EXCEPTION 'a sent envelope keeps its document';
  END IF;
  IF OLD.state IN ('completed', 'declined', 'voided', 'expired') AND NEW.state IS DISTINCT FROM OLD.state THEN
    RAISE EXCEPTION 'an ended envelope never changes state';
  END IF;
  IF OLD.completed_file_id IS NOT NULL AND (NEW.completed_file_id IS DISTINCT FROM OLD.completed_file_id
      OR NEW.completed_hash IS DISTINCT FROM OLD.completed_hash) THEN
    RAISE EXCEPTION 'the completed document never changes';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER envelopes_frozen BEFORE UPDATE ON envelopes
  FOR EACH ROW EXECUTE FUNCTION procera_envelope_frozen();

CREATE FUNCTION procera_envelope_fields_frozen() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE st text;
BEGIN
  SELECT state INTO st FROM envelopes WHERE workspace_id = COALESCE(NEW.workspace_id, OLD.workspace_id)
    AND id = COALESCE(NEW.envelope_id, OLD.envelope_id);
  IF st <> 'draft' AND (TG_OP <> 'UPDATE' OR NEW.kind IS DISTINCT FROM OLD.kind OR NEW.page IS DISTINCT FROM OLD.page
      OR NEW.x IS DISTINCT FROM OLD.x OR NEW.y IS DISTINCT FROM OLD.y OR NEW.w IS DISTINCT FROM OLD.w
      OR NEW.h IS DISTINCT FROM OLD.h OR NEW.recipient_id IS DISTINCT FROM OLD.recipient_id
      OR NEW.required IS DISTINCT FROM OLD.required OR (OLD.value IS NOT NULL AND NEW.value IS DISTINCT FROM OLD.value)) THEN
    RAISE EXCEPTION 'a sent envelope keeps its fields; a filled value never changes';
  END IF;
  RETURN COALESCE(NEW, OLD);
END $$;
CREATE TRIGGER envelope_fields_frozen BEFORE INSERT OR UPDATE OR DELETE ON envelope_fields
  FOR EACH ROW EXECUTE FUNCTION procera_envelope_fields_frozen();

-- An envelope holds its source document and its signed copy (neither can be deleted while held).
ALTER TABLE file_links DROP CONSTRAINT file_links_object_type_check;
ALTER TABLE file_links ADD CONSTRAINT file_links_object_type_check
  CHECK (object_type IN ('artifact_version', 'audit_export', 'request', 'envelope'));
