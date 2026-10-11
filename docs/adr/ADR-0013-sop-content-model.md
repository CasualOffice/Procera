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
- Export: print-ready HTML (browsers save it as a tagged PDF), Markdown, and DOCX (BPMS-042: `docx-rs`, MIT; Heading 1-3 styles with outline levels, Word numbering per list, header-row tables, hyperlinks, bold/italic runs; package timestamps fixed to the version's time, so the bytes are reproducible), with a banner on superseded versions. Images (BPMS-042): `{type:"image", file_id, text (alt, required), caption?}`; the file must be a clean PNG/JPEG/GIF/WebP attached to the same version; rendered through the re-authorized preview link, embedded as a data: URI in the printable HTML, referenced as `attachment:<id>` in Markdown and described in DOCX; a new version carries the previous version's attachments. Not yet built: deterministic server-side tagged PDF, DOCX/Markdown import, pictures inside DOCX, variables and artifact links as blocks.
