# 10 — Competitive analysis and positioning

Researched 2026-10-11 from vendor pages, release notes and press. Prices marked **(3p)**
come from third-party aggregators and are only a rough guide. Positioning moves fast, so
re-check any claim before quoting it externally.

## 1. Market map

| Segment | Products | What they own | What they lack |
|---|---|---|---|
| **Process governance / BPA** | SAP Signavio, ARIS, Nintex Process Manager (ex-Promapp), Trisotech | Process landscape, modelling, review cycles, read-only portals, mining | Execution of business work. Their SOP is not the running workflow. SaaS only (ARIS excepted). |
| **Enterprise low-code BPM / case** | Appian, Pega, ServiceNow App Engine, Bizagi | Execution, case management, decisioning, agents | SOP/document governance. Cost ($70–200/user/mo (3p), 6-figure deals). Need developers. |
| **No-code workflow / checklists** | Process Street, Pipefy, Kissflow, Tallyfy, Zoho Qntrl, Power Automate | Fast approvals, forms, templates, chat approvals (Teams) | BPMN/DMN, catalog, audit-grade history, self-hosting. |
| **Developer orchestration engines** | Camunda 8, Flowable, Operaton, Temporal | Engine correctness, BPMN/DMN depth | A business UI, governance, no-code. Camunda 8 self-managed production needs a paid licence (since 8.6). |
| **OSS / self-hosted automation** | n8n (fair-code), Windmill (AGPL), ProcessMaker CE (AGPL, now part of Decisions), Budibase (GPL), Appsmith | Integration flows, internal apps | Catalog, SOP governance, attestations, case management. |
| **Process mining** | Celonis, Signavio PI, Power Automate Process Mining | Discovery, conformance, digital twin | Governance and human execution. |
| **Substitutes** | Asana AI Studio, monday.com | Lightweight approvals in work tools | Governance, audit, versioned definitions. |

## 2. Capability matrix

Key: **Y** yes · **P** partial or add-on · **–** no.

| Product | Catalog/SOP gov | Attest. | No-code WF | BPMN | DMN | Case | Mining | Self-host | OSS |
|---|---|---|---|---|---|---|---|---|---|
| SAP Signavio | Y | P | P (approvals only) | Y | Y | – | Y | – | – |
| ARIS | Y | P | – | Y | Y | – | Y | Y | – |
| Nintex PM + Automation | Y | Y | Y (separate product) | P | – | – | P | P | – |
| Process Street | P | Y | Y | – | – | – | – | – | – |
| Pipefy / Kissflow | – | – | Y | – | – | P | – | – | – |
| Appian | – | – | P | Y | Y | Y | Y | P | – |
| Pega | – | – | P | P | Y | Y | Y | Y | – |
| ServiceNow | P | P | Y | – | P | Y | Y | – | – |
| Power Automate | – | – | Y | – | – | – | Y | – | – |
| Camunda 8 | – | – | – | Y | Y | – | – | paid | source-available |
| Flowable OSS | – | – | – | Y | Y | Y (engine) | – | Y | engines only |
| Operaton | – | – | – | Y | Y | – | – | Y | Apache-2.0 |
| n8n / Windmill | – | – | P (dev) | – | – | – | – | Y | fair-code / AGPL |
| **Procera (target)** | **Y** | **Y** | **Y** | **P (subset + import)** | **P (tables → DMN P2)** | **Y (P1)** | **export (P2)** | **Y** | **Apache-2.0** |

## 3. Recent market shifts that create the opening

1. **The OSS BPM vacuum (2024–2026).**
   - Camunda 7 Community Edition reached end of life with 7.24 in Oct 2025.
   - Camunda 8 self-managed production needs a commercial licence.
   - Bonita stopped distributing community binaries.
   - ProcessMaker merged into Decisions (Nov 2025, renamed Jun 2026).
   - n8n is fair-code, not OSI open source.
   - Operaton and Flowable OSS are engines without a business UI.
2. **Governance tools are SaaS-only.** They are SAP-tied or expensive, and charge for read-only consumers (Signavio ~$50–280/user/mo (3p); enterprise £150K–£800K/yr).
3. **Text-to-process AI is now table stakes**, offered by Nintex, Process Street Cora, Kissflow, Pega Blueprint, Power Automate Copilot, Appian Composer and Flowable. It no longer sets a product apart.
4. **Everybody pivoted to "governing agents"**: ARIS's Jan 2026 rebrand, ServiceNow AI Control Tower, Celonis AgentC/MCP, and agent steps in Appian, Flowable and Camunda.

## 4. Positioning

**"The approved SOP is the running process."** Procera is one open, self-hostable workspace. A process's narrative SOP, model, forms and rules are versioned and reviewed together. Publishing them is the governed act that changes how work runs and triggers re-attestation.

Wedges, in priority order:
1. **Unified governance + execution in OSS (Apache-2.0).** No competitor has it.
2. **Sovereign / regulated self-hosting.** Air-gapped deploy, dedicated-DB tenancy, bring-your-own or local LLM, Part 11-ready signatures. Targets EU public sector, banks, pharma, defence, and Indian enterprises with data-localisation needs.
3. **Approvals where people already talk.** Actionable approval and attestation cards inside the chat tools teams already use (Slack, Teams, Google Chat, Mattermost). This is the open analogue of Teams Approvals.
4. **Free unlimited readers/attesters.** Removes the most resented seat cost. Monetize authors, hosted service and enterprise support instead (OQ-06).
5. **Governed AI.** AI drafts are proposals that go through the same review/SoD as humans. Prompt/output provenance is audited and an org-level kill switch is available. Agent steps (P2) are versioned and attested like human steps.

## 5. Table stakes we must not forget

These come from buyer expectations, and v0.1 missed most of them. Each is mapped to a PRD ID.

| Expectation | PRD |
|---|---|
| Approve from mobile, email (signed one-time links) and chat cards; safe bulk approve | FR-N03, FR-P07, FR-E05 |
| Out-of-office delegation with date ranges; claim/unclaim; deputies | FR-E04 |
| Business calendars, working hours, multi-level escalation, pause/resume SLA clocks | FR-E12, FR-I01 |
| Template gallery (onboarding, purchase, CAPA, change control, access requests) and clone-and-adapt | FR-G08 |
| Import BPMN 2.0 XML, Excel/CSV landscapes, Word SOPs; Visio/ARIS/Signavio/Bizagi later | FR-G09 |
| Export to PDF/DOCX for auditors, with watermarks for superseded/controlled copies | FR-G10 |
| Read-and-understood attestations with optional quiz; training matrix | FR-G03, FR-G06 |
| Immutable audit trail with SIEM/CSV/JSON export; Part 11/Annex 11 e-signatures | FR-P05, FR-G11 |
| SSO, SCIM, groups → roles, field-level permissions, **guest/external participants** | FR-P02, FR-P08 |
| Multilingual UI and content with translation workflow; RTL | FR-P04 |
| Sandbox → production promotion of definitions | FR-P09 |
| Cycle time, SLA breach, bottlenecks, attestation coverage, process health score | FR-I01, FR-I05 |
| Event-log export (OCEL 2.0 / XES) as a bridge to mining tools | FR-I03 |

## 6. Pricing reference points (for OQ-06, the business model decision)

| Product | Indicative price |
|---|---|
| Power Automate Premium | $15/user/mo (includes mining) |
| Tallyfy | $5 docs / $30 pro per member/mo |
| Zoho Qntrl | from $20/user/mo |
| Pipefy | Business ~$33, Enterprise ~$65/user/mo |
| Signavio | ~$50/$140/$280 per user/mo (3p) |
| Pega | ~£85–103/user/mo (UK G-Cloud), 500-user minimums reported |
| Process Street | median contract ~$72K/yr (3p) |
| Camunda 8 enterprise | custom; ~$65K/yr quote cited for 500K instances/mo (3p) |

## 7. Threats

- **Microsoft Power Automate bundling.** Approvals in Teams are effectively free to M365 shops. Counter: governance depth, self-hosting and non-Microsoft messaging.
- **An incumbent open-sourcing a governance layer.** This is a low-probability move.
- **Operaton/Flowable communities adding a business UI.** We would partner rather than compete: they are candidate runtimes (ADR-0015).
- **Scope trap.** Trying to match Appian/Pega breadth. Mitigated by the non-goals list in [02-PRD §11](02-PRD.md).

## Sources (selection)

- Signavio approvals/governance: <https://learning.sap.com/courses/managing-business-processes-with-sap-signavio-solutions> · release Nov 2025: <https://www.signavio.com/post/november-2025-product-release-update>
- ARIS standalone and CEO: <https://www.processexcellencenetwork.com/process-mining/news/process-intelligence-firm-aris-appoints-new-ceo-to-lead-agentic-ai-strategy>
- Nintex AI process creation: <https://help.nintex.com/en-US/promapp/AI-ProcessCreation.htm>
- Process Street Cora: <https://www.process.st/product/cora/>
- Pipefy pricing: <https://help.pipefy.com/en/articles/12001177-new-pipefy-ai-models-and-pricing>
- Appian BOAT MQ: <https://appian.com/about/explore/press-releases/2025/appian-is-named-a-leader-in-inaugural-gartner-magic-quadrant-for-business-orchestration-and-automation-technologies>
- Power Automate pricing: <https://www.microsoft.com/en-us/power-platform/products/power-automate/pricing/>
- Camunda 8 licensing: <https://camunda.com/blog/2024/04/licensing-update-camunda-8-self-managed/>
- Flowable 2025.1/2025.2: <https://www.flowable.com/blog/releases/flowable-2025-1-intelligent-orchestration>
- ProcessMaker + Decisions: <https://www.processmaker.com/resources/customer-success/news/merger-of-processmaker-and-decisions-creates-the-future-of-business-automation-and-ai-orchestration/>
- Bonita binaries: <https://community.bonitasoft.com/answer-binaries-future-versions>
- n8n licence: <https://docs.n8n.io/privacy-and-security/sustainable-use-license>
- Tallyfy pricing: <https://tallyfy.com/products/pro/pricing/>
