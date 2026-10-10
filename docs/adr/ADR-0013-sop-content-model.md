# ADR-0013 — Structured block model for SOP/policy content

Status: **Accepted** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
SOPs/policies must diff semantically, render accessibly, export to PDF/DOCX, and be authored easily.

## Decision
Portable JSON block model (headings, paragraphs, lists, tables, callouts, steps, images, links to artifacts, variables) as source of truth; deterministic PDF (tagged) and DOCX export; DOCX/Markdown import. Optional CasualOffice OpenDoc engine integration later for high-fidelity DOCX.

## Alternatives considered
Store DOCX as source (rejected: poor diffs). Raw HTML (rejected: sanitisation/diff).

## Consequences
Need an editor (block editor) and a renderer shared by web/PDF.
