# SP-07: Licence and SBOM review of the candidate dependency set

Status: **Done. Approved set below. Gates live for Rust (cargo-deny) and JS (`scripts/js-licence-gate.mjs`).** · Date: 2026-10-11 · Tracker: BPMS-004 · Policy: [ADR-0014](../adr/ADR-0014-licensing-and-contribution.md), [docs/24 §3.12](../24-ENGINEERING-RULES.md)

Method:
- licences were read from the registries (crates.io via `cargo info` / `cargo deny`; npm via `npm view`) on 2026-10-11;
- the spike workspaces' full dependency trees were run through the product `deny.toml`;
- the JS lockfiles were run through the new JS gate.

## Gates

| Ecosystem | Gate | Runs in |
|---|---|---|
| Rust | `cargo deny check` with `deny.toml`: permissive allow list, advisories, banned sources | CI `licence-gate` job |
| JavaScript | `node scripts/js-licence-gate.mjs`: every `package-lock.json`, allow list plus package deny list (bpmn-js, dmn-js, form-js, SurveyJS Creator, …) | CI (from this change) |
| SBOM | CycloneDX via `cargo cyclonedx`, uploaded per CI run. The JS SBOM is added with the web build (BPMS-010 follow-up). | CI `sbom` job |

**Allow list:**
- MIT, MIT-0, Apache-2.0 (incl. LLVM exception), BSD-2/3, ISC, 0BSD, Zlib, CC0-1.0, Unicode-3.0, Unlicense, BlueOak-1.0.0.
- MPL-2.0 (unmodified, file-level copyleft).
- EPL-2.0 (only unmodified, separately loaded modules).
- Python-2.0 and CC-BY-4.0 (npm data/tooling packages).
- **CDLA-Permissive-2.0**: Mozilla CA root bundle, needed for TLS. Added in this review.
- **BSL-1.0 = Boost Software License** (permissive, OSI), e.g. `xxhash-rust` via the `redis` client. Not to be confused with the blocked **Business Source License (BUSL-1.1)**.

**Blocked:** AGPL, GPL/LGPL (linked), SSPL, BUSL-1.1 (Business Source), SUL, Commons-Clause, Elastic, the bpmn.io watermark licence, and commercial EULAs.

## Approved Rust dependencies (checked)

| Area | Crate | Licence | Status |
|---|---|---|---|
| HTTP | axum 0.8, tokio, tower, hyper | MIT | ✅ in use |
| CLI / config | clap 4 | MIT/Apache-2.0 | ✅ in use |
| Serialization | serde, serde_json | MIT/Apache-2.0 | ✅ in use |
| Time / ids | time, uuid (v7) | MIT/Apache-2.0 | ✅ in use |
| Telemetry | tracing, tracing-subscriber | MIT | ✅ in use |
| Schema validation | jsonschema 0.58 | MIT | ✅ in use (conformance) |
| Database | sqlx 0.8 (postgres, rustls) | MIT/Apache-2.0 | ✅ spike-verified (SP-09/10) |
| Authorization | cedar-policy 4.13 | Apache-2.0 | ✅ spike-verified (SP-09) |
| Expressions | cel 0.15 | MIT | ✅ recommended (SP-08) |
| Hashing | sha2 | MIT/Apache-2.0 | ✅ spike-verified (SP-10) |
| TLS roots | webpki-root-certs | CDLA-Permissive-2.0 | ✅ allowed (data licence) |
| Decimals | rust_decimal | MIT | ✅ planned (SP-08 money extension) |
| FEEL | dsntk-* 0.3 | MIT/Apache-2.0 (+ `dfp-number-sys` MIT/Apache, C code) | ⚠️ **offline DMN importer only** (SP-08). It pulls `reqwest` (HTTP client) for external functions and does not build to WASM. |

## Approved JavaScript dependencies (checked)

| Area | Package | Licence | Decision |
|---|---|---|---|
| UI runtime | react / react-dom 19 | MIT | ✅ |
| Accessible primitives | react-aria, react-aria-components, react-stately | Apache-2.0 | ✅ (ADR-0018) |
| Data grid | @tanstack/react-table 9, @tanstack/react-virtual | MIT | ✅ |
| Server state | @tanstack/react-query 5 | MIT | ✅ |
| Step canvas | @xyflow/react 12 | MIT | ✅ (Pro is paid examples, not a licence) |
| BPMN view (P2) | diagram-js, bpmn-moddle, dmn-moddle | MIT | ✅ with our own renderer, no bpmn-js code |
| BPMN read-only viewer | bpmn-visualization | Apache-2.0 | ✅ |
| Auto-layout | elkjs | EPL-2.0 OR GPL-3.0-or-later | ✅ under **EPL-2.0**, unmodified, lazy-loaded chunk |
| Auto-layout (simple) | dagre | MIT | ✅ |
| PDF viewing / field placement | pdfjs-dist | Apache-2.0 | ✅ |
| Block editor (SOP/policy) | @tiptap/core, @tiptap/react (ProseMirror) | MIT | ✅ preferred (U-10). Lexical (MIT) is the alternative. |
| Forms | @rjsf/core (Apache-2.0), @jsonforms/core (MIT), formiojs (MIT) | — | ✅ renderer options for the own builder |
| i18n | @formatjs/intl (MIT), messageformat (Apache-2.0), i18next (MIT) | — | ✅ ICU MessageFormat |
| Validation | zod 4 | MIT | ✅ |
| Dates | date-fns 4 | MIT | ✅ |
| Build | vite 8, @vitejs/plugin-react | MIT | ✅ |
| Types | typescript 7 | Apache-2.0 | ✅ |
| Tests | @playwright/test (Apache-2.0), @axe-core/playwright / axe-core (MPL-2.0, dev only) | — | ✅ |

**Rejected:**
- **bpmn-js, dmn-js, @bpmn-io/form-js:** the watermark licence prevents white-labelling.
- **survey-creator-core:** commercial EULA.
- All are on the JS gate's deny list.

## Fonts and icons

These must be decided with the design system (BPMS-010). The requirement is an **OFL-1.1 or MIT/Apache licence**:
- **Inter** (OFL-1.1) or IBM Plex (OFL-1.1) for the UI font;
- **Lucide** (ISC) or Tabler Icons (MIT) for icons.

Self-host the fonts, with no runtime Google Fonts dependency (privacy and air-gapped installs). Add OFL to the allow list for font packages when they're introduced.

## Re-review cadence

Quarterly, and whenever a new dependency class is added (docs/22 §4 external dependencies). Each re-review re-runs the registry checks: a licence change upstream breaks the gate in CI, which is the point.
