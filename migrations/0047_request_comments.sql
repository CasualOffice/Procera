-- 0047 (BPMS-214): request comments, @mentions and watchers (FR-R02, FX-R02, docs/08 Requests).
-- Comments are an append-only conversation on a request. An internal comment is hidden from the
-- person who started the request (approvers and admins talking among themselves). Mentions name
-- members who can already see the request; they are notified. Watchers follow a request and hear
-- about new comments and its outcome; anyone who can see a request may watch it.

CREATE TABLE request_comments (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  instance_id uuid NOT NULL,
  author_membership_id uuid NOT NULL,
  body text NOT NULL CHECK (length(body) BETWEEN 1 AND 4000),
  internal boolean NOT NULL DEFAULT false,
  mentions uuid[] NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, instance_id) REFERENCES requests (workspace_id, instance_id),
  FOREIGN KEY (workspace_id, author_membership_id) REFERENCES memberships (workspace_id, id)
);
CREATE INDEX request_comments_by_request ON request_comments (workspace_id, instance_id, created_at);
SELECT procera_enable_workspace_rls('request_comments');

CREATE TABLE request_watchers (
  workspace_id uuid NOT NULL,
  instance_id uuid NOT NULL,
  membership_id uuid NOT NULL,
  created_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, instance_id, membership_id),
  FOREIGN KEY (workspace_id, instance_id) REFERENCES requests (workspace_id, instance_id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id)
);
SELECT procera_enable_workspace_rls('request_watchers');
