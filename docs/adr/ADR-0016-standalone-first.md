# ADR-0016 — Standalone first; integrations later via ports

Status: **Accepted (product direction 2026-10-11)** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
The product must be independent. All integrations are optional add-ons.

## Decision
P0 requires only Postgres, S3-compatible storage and SMTP. Local auth + MFA, built-in directory, in-app + email notifications. All integrations behind ports; shipped from P1 (SSO/SCIM/webhooks) and P2 (chat platforms, HRIS, ERP, QTSP).

## Alternatives considered
Integration-first around a single partner platform (rejected by the product owner).

## Consequences
Templates must run with zero integrations; integration hook points documented.
