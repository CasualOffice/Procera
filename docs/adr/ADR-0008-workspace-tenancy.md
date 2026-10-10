# ADR-0008 — Workspace is the tenant; layered isolation

Status: **Accepted (revised per product direction 2026-10-11)** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Product direction: multi-tenant and workspace-based, with system admins over all workspaces; users can belong to many workspaces.

## Decision
`workspace_id` is the tenant key on every row; global users with per-workspace memberships; isolation by auth context + repository scoping + forced RLS + composite FKs + per-workspace KEKs + workspace-bound object keys. Tiers: shared (default), dedicated schema (P2), dedicated database (regulated). System plane is separate and cannot read content except via break-glass.

## Alternatives considered
Organization above workspace (dropped for simplicity; departments/collections give intra-workspace scoping). Schema-per-tenant by default (rejected: migration cost at scale).

## Consequences
All tooling (cron jobs, exports, search) must carry workspace context; cross-workspace reporting is system-plane usage-only.
