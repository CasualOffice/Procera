-- Minimal runtime schema for SP-01..05 (shape follows docs/04 §2.5–2.6, single workspace).
DROP SCHEMA IF EXISTS rt CASCADE;
CREATE SCHEMA rt;
CREATE TABLE rt.clock (id int PRIMARY KEY DEFAULT 1, now timestamptz NOT NULL);
INSERT INTO rt.clock VALUES (1, now());
CREATE TABLE rt.definitions (version int PRIMARY KEY, ir jsonb NOT NULL);
CREATE TABLE rt.instances (id uuid PRIMARY KEY, def_version int NOT NULL REFERENCES rt.definitions, state text NOT NULL, revision bigint NOT NULL DEFAULT 0, vars jsonb NOT NULL DEFAULT '{}', ended_at timestamptz);
CREATE TABLE rt.tokens (id uuid PRIMARY KEY, instance_id uuid NOT NULL REFERENCES rt.instances, node_id text NOT NULL,
  state text NOT NULL CHECK (state IN ('enter','waiting','resume','arrived','done','cancelled')), outcome text, split_id uuid, branch text, seq bigserial);
CREATE INDEX ON rt.tokens (instance_id) WHERE state IN ('enter','waiting','resume','arrived');
CREATE TABLE rt.splits (id uuid PRIMARY KEY, instance_id uuid NOT NULL, join_node text NOT NULL, expected int NOT NULL, arrived text[] NOT NULL DEFAULT '{}', released boolean NOT NULL DEFAULT false, parent_split uuid);
CREATE TABLE rt.work_items (id uuid PRIMARY KEY, instance_id uuid NOT NULL, token_id uuid NOT NULL, node_id text NOT NULL,
  state text NOT NULL CHECK (state IN ('offered','completed','cancelled','expired')), outcome text, assignee text, revision bigint NOT NULL DEFAULT 0);
CREATE INDEX ON rt.work_items (instance_id);
CREATE TABLE rt.jobs (id uuid PRIMARY KEY, kind text NOT NULL, instance_id uuid NOT NULL, token_id uuid, run_at timestamptz NOT NULL DEFAULT now(),
  state text NOT NULL DEFAULT 'queued' CHECK (state IN ('queued','leased','done','cancelled','dead')), lease_owner text, lease_until timestamptz,
  fence bigint NOT NULL DEFAULT 0, attempts int NOT NULL DEFAULT 0, idem_key text NOT NULL UNIQUE, payload jsonb NOT NULL DEFAULT '{}');
CREATE INDEX jobs_ready ON rt.jobs (run_at) WHERE state IN ('queued','leased');
CREATE TABLE rt.timers (id uuid PRIMARY KEY, instance_id uuid NOT NULL, token_id uuid NOT NULL, purpose text NOT NULL, due_at timestamptz NOT NULL,
  state text NOT NULL DEFAULT 'scheduled' CHECK (state IN ('scheduled','leased','fired','cancelled')), lease_until timestamptz, fence bigint NOT NULL DEFAULT 0, payload jsonb NOT NULL DEFAULT '{}');
CREATE INDEX timers_due ON rt.timers (due_at) WHERE state IN ('scheduled','leased');
CREATE TABLE rt.events (instance_id uuid NOT NULL, seq bigint NOT NULL, kind text NOT NULL, node_id text, payload jsonb NOT NULL DEFAULT '{}', PRIMARY KEY (instance_id, seq));
CREATE TABLE rt.outbox (id bigserial PRIMARY KEY, instance_id uuid NOT NULL, topic text NOT NULL, payload jsonb NOT NULL, published boolean NOT NULL DEFAULT false);
-- Simulated external system: applies an effect at most once per idempotency key; counts attempts.
CREATE TABLE rt.effects (idem_key text PRIMARY KEY, instance_id uuid NOT NULL, applied_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE rt.effect_attempts (id bigserial PRIMARY KEY, idem_key text NOT NULL);
CREATE TABLE rt.inbox_dedup (consumer text NOT NULL, message_id text NOT NULL, PRIMARY KEY (consumer, message_id));
CREATE TABLE rt.idempotency (actor text NOT NULL, key text NOT NULL, request_hash text NOT NULL, response jsonb, PRIMARY KEY (actor, key));
