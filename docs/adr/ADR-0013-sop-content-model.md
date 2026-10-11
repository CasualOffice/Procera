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

## As built (BPMS-042, 2026-10-11)
- Editor: **TipTap core 3 (MIT, on ProseMirror)** with StarterKit and TableKit, closing U-10. The editor edits the block model directly (`web/src/lib/blocks.ts` maps both ways).
- Blocks at P0: paragraph, heading (1–3), list (`ordered`), callout (`info`/`warning`), table (`header`, `rows`); inline `runs` add bold, italic and links (`http(s)`, `mailto`, in-app `/…`), and must concatenate to the block's plain `text`. The server validates the same model (`crates/library/src/blocks.rs`).
- Export: print-ready HTML (browsers save it as a tagged PDF) and Markdown, with a banner on superseded versions. Not yet built: deterministic server-side tagged PDF, DOCX export and DOCX/Markdown import, images, variables and artifact links as blocks.
