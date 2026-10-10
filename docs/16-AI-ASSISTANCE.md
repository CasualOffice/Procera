# 16 — AI assistance

Status: Draft · Owner: Product + Security lead · Closes gap C9 · Phase: P1 (assistants), P2 (governed agent steps).

Text-to-process and SOP drafting are table stakes in 2026 (10-COMPETITIVE §3). Our difference is **governed AI**:
- every AI output is a proposal that goes through the same review, SoD and audit as human work;
- every use is labelled and traceable;
- AI is optional, off by default, and can run fully on local models for air-gapped installs.

## 1. Scope

| ID | Capability | Phase | Output | Lands as |
|---|---|---|---|---|
| AI-001 | **Process drafting** from a description, pasted text or an uploaded SOP/document | P1 | IR v1 draft + forms + rule tables | New *draft* (never published), compiler diagnostics attached |
| AI-002 | **SOP drafting** from a process model, or restructuring an existing document into the SOP block model | P1 | SOP blocks | Draft SOP version |
| AI-003 | **Form generation** from a description or an example document | P1 | JSON Schema + UI schema | Draft form version |
| AI-004 | **Rule table suggestion** from a policy text ("over 50 000 needs finance") | P1 | Rule table | Draft rule version |
| AI-005 | **Summarise a request or case** (history, current blocker, next step) | P1 | Text | Ephemeral panel; optional save as comment (attributed) |
| AI-006 | **Explain an audit timeline** in plain language for investigators | P1 | Text with event links | Ephemeral; can be pinned to an investigation as a labelled note |
| AI-007 | **Translate an SOP / form labels** into another workspace locale | P1 | Locale bundle / translated SOP draft | Draft translation needing reviewer approval |
| AI-008 | **Improve suggestions** for a draft (missing rejection paths, unclear steps, SLA gaps) | P1 | Annotated suggestions | Suggestion list on the draft |
| AI-009 | **Natural-language search / filters** ("overdue purchase requests in Finance last week") | P1 | Data-grid filter AST | Applied filter shown as chips, editable |
| AI-010 | **Governed agent step** (`ai.task` node: classify, extract, draft a reply) inside a process | P2 | Structured output validated against schema | Output goes to a human review step unless the node is explicitly non-decisional |

Out of scope:
- AI approving, rejecting, signing or publishing anything;
- AI choosing assignees based on people's behaviour or performance;
- AI-driven employee scoring or monitoring;
- autonomous changes to running instances.

## 2. Invariants (enforced in code, not configurable)

1. **Drafts only.** AI-generated artefacts enter the canonical lifecycle at `Draft`. Publication still needs the normal review and SoD (12-AUTHZ §10). The *accepting user* is the author of record. The AI is never a reviewer, and AI output never satisfies an independent-review requirement.
2. **No decisions.** AI cannot complete `human.approval`, cannot produce signatures (ESIG-*) and cannot act as a principal. The AI service has no write access to decisions, roles or settings.
3. **No people-scoring or behaviour-based routing.**
   - Assignment logic stays rule- and role-based (11-IR §3.1).
   - Task allocation based on individual behaviour or traits falls under EU AI Act Annex III §4(b), which is **high-risk**, with obligations applying from **2 Dec 2027** (AI Omnibus).
   - The product does not offer such a mode, and the documented Art. 6(3) assessment records this (AI-01 in 14).
4. **Labelled and traceable (Art. 50).** Every AI-produced artefact or text carries an `ai_generated` provenance record. The UI shows an **"AI-drafted"** badge until a human edits *and* accepts each part.
5. **Same permissions as the user.** The assistant only sees what the invoking user can see, using the same policy evaluation and redaction path. It cannot read across workspaces.
6. **Off by default.** The System Admin must configure a provider and the Workspace Admin must enable features before anything is sent to a model.

## 3. Configuration and control

**System Admin (platform plane):**
- provider configuration: endpoints, API keys stored in the secret store, model allowlist, region;
- platform kill switch;
- per-workspace AI quota;
- whether hosted (non-local) providers are allowed at all.

**Workspace Admin:**
- per-feature toggles (AI-001…AI-010);
- which roles may use each feature (`ai:use_assistant`, `ai:manage_settings`);
- an "exclude restricted data entirely" switch (default **on**);
- a monthly budget and alert threshold;
- a provider choice from the platform allowlist.

Kill switches take effect immediately: in-flight requests are cancelled and the UI hides the entry points.

## 4. Provider abstraction

```text
trait ModelProvider {
  fn capabilities(&self) -> Capabilities;              // context size, tool use, structured output, vision
  async fn complete(&self, req: ModelRequest) -> Result<ModelResponse, ProviderError>;
}
```

| Adapter | Use |
|---|---|
| **Anthropic** (Claude: latest models, e.g. `claude-opus-5-5` / `claude-sonnet-5-5` for drafting, `claude-haiku-4-5-20251001` for summaries/search) | Hosted default when allowed |
| **OpenAI-compatible endpoint** (covers Azure OpenAI, vLLM, LM Studio, many gateways) | Customer-chosen providers |
| **Local** via Ollama or vLLM (open-weight models) | Air-gapped / sovereign installs; no data leaves the deployment |

- **Model routing per feature** is configurable. Defaults favour a cheaper model for summaries and search, and a stronger model for process drafting.
- **Structured output** uses JSON Schema. Providers without native structured output go through a validate-and-repair loop with at most 2 retries.
- **Prompts are versioned assets** in the repo (`ai/prompts/<feature>/vN.md`). The prompt version is part of the provenance record.

## 5. Data handling

- **No training on tenant data.** Only providers with contractual no-training/zero-retention terms are allowlisted for hosted use. This is documented in the subprocessor list (PRV-05).
- **Minimisation and redaction before prompt:**
  - `restricted` values are removed by default (replaced with typed placeholders such as `<restricted:decimal>`);
  - `confidential` values are included only if the workspace allows it;
  - file contents are included only when the user explicitly attaches them to the request.
- **Residency:** a provider region is pinned per workspace and matched to the workspace's data residency (PRV-05). Local models satisfy any residency rule.
- **Retention:** prompts and responses are not stored by default. Only provenance (hashes, ids, token counts) is kept. An optional per-workspace "store transcripts for 30 days" setting exists for quality review, and its contents are classified `confidential`.
- **DPIA template** for AI features ships with the product (PRV-06).

## 6. Prompt-injection and safety defences

Source documents, request comments and SOP text can contain hostile instructions. Defences:

1. **Untrusted content tagging.** All workspace content is wrapped in delimited, labelled blocks. System prompts state that such content is data, never instructions.
2. **No tools with side effects.** Assistants in P1 have read-only tools scoped to the user's permissions (fetch a process, list a form's fields, look up directory groups by name). There is no tool that writes, sends, publishes or assigns. Writes happen only when the user clicks *Accept*, through normal APIs.
3. **Schema and compiler validation.** Every structured output is validated against its JSON Schema, then compiled:
   - processes go through the IR compiler (11-IR §11), with diagnostics shown to the user;
   - forms are checked by the JSON Schema meta-validator;
   - rules go through the rule-table type checker.
   Invalid output is never persisted as a draft without its diagnostics.
4. **Reference checks.** AI-proposed connectors, groups or roles must already exist and be permitted. Unknown references are flagged, not auto-created.
5. **Output rendering.** AI text is rendered as plain text or sanitized Markdown, with no HTML, scripts or remote images. Links are shown with their full target.
6. **Egress.** Hosted providers are reached only through the egress proxy (SSRF-01). Local providers need explicit allowlisting.
7. **Abuse limits:** per-user rate limits, maximum input size, and maximum output tokens per feature.

## 7. Provenance and audit

Every AI invocation that produces a persisted artefact or a saved text writes an audit event `ai.suggestion.accepted` / `ai.suggestion.rejected` containing:

```text
feature, provider, model, model_version, prompt_version, prompt_hash (SHA-256 of rendered prompt),
input_refs (object ids + versions, not contents), output_hash, token_in, token_out, cost_estimate,
invoking_user, accepting_user, accepted_parts, workspace_id, trace_id, timestamp
```

- Draft versions keep `ai_provenance_id` per generated element, so the semantic diff shows which parts were AI-drafted and which were later human-edited.
- Exports (PDF/DOCX) of AI-assisted SOPs include a provenance note in the document metadata. Whether it is also visible on the page is a workspace setting.

## 8. UX patterns

- **Suggestion diff.** AI output is shown as a diff against the current draft, or as a preview for new artefacts. The user accepts or rejects per change: per step, per form field, per rule row.
- **"AI-drafted" badge** on each element until a human edits or explicitly confirms it. Reviewers see a count of unconfirmed AI elements in the review screen and cannot approve while any remain unconfirmed (configurable for standard processes, always enforced for controlled ones).
- **Explain and cite.** Summaries and timeline explanations link every claim to the underlying event or comment. Claims without a source are not shown.
- **Graceful absence.** When AI is disabled, every flow works the same way manually. No empty AI panels and no nagging.
- **Cost transparency.** Admins see usage per feature and per user against the budget.
- **Accessibility.** Streaming output is announced politely (`aria-live="polite"`), and accept/reject is fully keyboard-operable.

## 9. Evaluation harness

Location: `ai/evals/`. It runs nightly against each configured model and on every prompt change.

| Suite | Metric | Target (P1 GA) |
|---|---|---|
| Process drafting golden set (≥ 150 descriptions → reference IR, incl. the 19-TEMPLATES catalogue paraphrased) | Compile-pass rate | ≥ 90 % first attempt, ≥ 98 % after repair |
| | Structural similarity to reference (node/edge/outcome match) | ≥ 0.8 |
| | Unhandled-outcome rate | ≤ 5 % |
| SOP drafting | Rubric score (completeness of required blocks, no invented roles) by LLM-judge + human spot check | ≥ 4/5 |
| Form generation | Schema-valid rate; field type accuracy | 100 %; ≥ 90 % |
| Summaries / audit explanation | Citation precision (every claim backed by a linked source) | ≥ 98 % |
| NL filters | Exact filter-AST match | ≥ 85 % |
| Injection suite (hostile documents/comments) | Instruction-following rate on injected content | 0 % (any success fails the build for that model) |
| Redaction | Restricted value leakage into prompt | 0 |
| Pilot telemetry | Reviewer acceptance rate of AI-drafted elements; edit distance before acceptance | Tracked; informs prompt work |

A model is only added to the allowlist after passing the suites. Results are versioned per model and prompt version.

## 10. Cost and quota

- Per-workspace monthly token or cost budgets, set by the System Admin with optional Workspace Admin sub-limits per feature.
- Soft limit at 80 % notifies admins. At the hard limit, the feature is disabled until the period ends or the budget is raised.
- Caching: identical prompts within a short TTL reuse results for search and filter suggestions only, never for drafting.
- Usage metrics are exposed on the admin dashboard (FR-A01) and in Prometheus.

## 11. Compliance mapping

| Obligation | How addressed |
|---|---|
| EU AI Act Art. 4 (AI literacy) | In-product guidance and admin docs on limitations; training note in the onboarding template |
| Art. 50 transparency | Badges, provenance, export metadata |
| Annex III §4(b) high-risk | Not offered; Art. 6(3) assessment documented; invariant 3 |
| GDPR Art. 22 (automated decisions) | No automated decisions with legal or similar effect; humans decide |
| GxP (Annex 22 draft) | AI disabled by default in Part 11-mode workspaces; enabling requires a Compliance Officer and a validation note |
| DPDP / GDPR minimisation | Redaction, no-training providers, residency pinning |

## 12. Backlog

| ID | Item | Phase | Size | Depends on |
|---|---|---|---|---|
| AI-011 | Provider abstraction + Anthropic, OpenAI-compatible and Ollama adapters | P1 | M | Secret store, egress proxy |
| AI-012 | System Admin provider config + workspace feature toggles + kill switch | P1 | S | AI-011, settings model |
| AI-013 | Redaction-before-prompt (shared with export redaction path) | P1 | S | Field-level policy |
| AI-014 | Provenance records + audit events + "AI-drafted" badge model | P1 | M | Audit trail |
| AI-015 | Process drafting (AI-001) with compiler loop and suggestion diff UI | P1 | L | IR compiler, designer |
| AI-016 | SOP drafting / restructure (AI-002) | P1 | M | SOP block model |
| AI-017 | Form + rule-table generation (AI-003, AI-004) | P1 | M | Form builder, rule tables |
| AI-018 | Request/case summary + audit explanation with citations (AI-005, AI-006) | P1 | M | Audit explorer |
| AI-019 | SOP/form translation workflow (AI-007) | P1 | M | i18n bundles |
| AI-020 | Draft improvement suggestions (AI-008) | P1 | S | AI-015 |
| AI-021 | NL → data-grid filter (AI-009) | P1 | S | Shared data grid filter AST |
| AI-022 | Evaluation harness, golden sets, injection suite, CI integration | P1 | M | AI-011 |
| AI-023 | Usage metering, budgets, admin dashboard tiles | P1 | S | AI-011 |
| AI-024 | AI DPIA template, Art. 6(3) assessment, subprocessor list entries | P1 | S | — |
| AI-025 | Governed `ai.task` node (AI-010): schema-bound output, mandatory review step, versioned prompts in release bundle | P2 | L | IR v2, AI-022 |
