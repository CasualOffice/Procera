# 19 — Process template library (pre-designed, industry-standard processes)

Status: **Draft for G2 sign-off** · Owner: Product + domain SMEs · Implements FR-G08 (template gallery) and part of FR-G09 (import).
Researched 2026-10-11. Unverified items are marked ⚠.

The template library is how a new workspace gets to "first useful process published in ≤ 30 minutes" (PRD success measure). A template is a complete, governed, runnable starting point. Once installed it is the workspace's own draft, and the workspace changes it, reviews it and owns it.

## 1. Purpose and principles

1. **Standalone first.** Every template runs with **zero integrations**: in-app work, in-app notifications and email only. Places where an integration would help (HRIS, ERP, ITSM, chat platforms, e-signature QTSP) are marked as **hook points**: a `service.call` or `wait.signal` stub that is disabled by default and falls back to a human task. Enabling a connector later replaces the human fallback without redesigning the process.
2. **Standards-aligned, never standards-copied.** Templates *align to* frameworks (APQC PCF, ISO 9001/27001/13485 clauses, GxP, SOX/COSO, ITSM practice, GDPR/DPDP). They never reproduce copyrighted text. Alignment is expressed as references (`iso9001:7.5.3`, `apqc:pcf:<id>`) plus our own original wording. See §8 for framework-by-framework terms.
3. **Complete package.** Every template ships all of the following:
   - IR process ([11-WORKFLOW-IR-SPEC](11-WORKFLOW-IR-SPEC.md))
   - forms
   - rule tables
   - SOP skeleton
   - RACI
   - SLA defaults
   - KPIs
   - sample/test-run fixtures
   - locale bundles

   A process without its SOP and KPIs is not a template.
4. **Lint-clean against IR v1.** Templates use only v1-supported constructs: no inclusive/event-based gateways, no scripts, structured parallelism, bounded loops. CI compiles every template on every IR or compiler change.
5. **Role placeholders, not people.** Assignments reference `{{role:<key>}}` placeholders that the install wizard maps to workspace roles or groups. Templates contain no user ids, no emails and no tenant-specific data.
6. **Opinionated defaults, everything configurable.** Thresholds, SLAs, calendars, approval levels and optional steps are install-time parameters with sensible defaults, not hard-coded values.
7. **Controlled-process aware.** Templates tagged `criticality: controlled` (GxP, SOX, finance above threshold) ship with SoD on, no auto-outcome escalations, and signature mode `simple` or `part11`.

## 2. Template package format

A template is a versioned, signed bundle `tpl:<key>@<semver>` stored in the platform-level Template Gallery (outside every workspace).

```jsonc
{
  "manifestVersion": 1,
  "key": "hr.employee-onboarding",
  "version": "1.2.0",                       // semver: major = breaking IR/variable change
  "name": { "en": "Employee onboarding", "hi": "कर्मचारी ऑनबोर्डिंग" },
  "summary": { "en": "Offer accepted → Day-1 ready, with IT, facilities and policy acknowledgement." },
  "domain": "hr",
  "industries": ["cross-industry"],
  "wave": 1,
  "criticality": "standard",
  "alignment": [                            // references only, never copied text
    { "framework": "apqc-pcf", "ref": "7.0 Develop and Manage Human Capital (category)" },
    { "framework": "iso27001", "ref": "A.6.1, A.6.3, A.5.18" }
  ],
  "roles": [                                // placeholders the wizard must map
    { "key": "hr_coordinator", "label": "HR coordinator", "required": true, "suggest": "group:hr" },
    { "key": "hiring_manager", "label": "Hiring manager", "required": true, "suggest": "relation:manager_of(new_hire)" },
    { "key": "it_provisioner", "label": "IT provisioning", "required": true },
    { "key": "facilities",     "label": "Facilities", "required": false }
  ],
  "parameters": [                           // install-time knobs with defaults
    { "key": "day1_lead_days", "type": "integer", "default": 5, "label": "Business days before start to finish prep" },
    { "key": "include_facilities", "type": "boolean", "default": true },
    { "key": "calendar", "type": "calendar_ref", "default": "workspace-default" }
  ],
  "artifacts": {
    "process":  "process.ir.json",          // IR v1, placeholders unresolved
    "forms":    ["forms/new-hire.json", "forms/it-checklist.json"],
    "rules":    ["rules/equipment-by-role.json"],
    "sop":      "sop/skeleton.json",        // SOP block model (ADR-0013)
    "raci":     "raci.json",
    "sla":      "sla.json",
    "kpis":     "kpis.json",
    "fixtures": ["fixtures/happy-path.json", "fixtures/late-start.json"],
    "i18n":     ["i18n/en.json", "i18n/hi.json"]
  },
  "hookPoints": [
    { "node": "create-accounts", "connector": "hris|idp", "fallback": "human.task" }
  ],
  "requires": { "irSchema": 1, "minProduct": "0.6.0", "features": ["human.approval", "wait.timer"] },
  "license": "Apache-2.0",
  "attribution": [],                        // filled when a framework requires it (e.g. APQC)
  "changelog": "CHANGELOG.md",
  "signature": "sigstore-bundle"            // gallery integrity
}
```

Rules:
- `kpis.json` defines each KPI against domain events: name, formula (expression language), unit, target and direction. Because it is computed only from emitted events, analytics work out of the box.
- `fixtures` are test-run scenarios (FR-E11) with fake identities, inputs and expected end state. They double as the template's CI test.
- The SOP skeleton contains original prose sections plus `<<fill: …>>` prompts the owner must complete. Publication is blocked while unfilled required prompts remain.

## 3. Gallery and install lifecycle

```
System admin (platform)                       Workspace admin / process owner
───────────────────────                        ──────────────────────────────
author/import template → SME review
→ CI (compile, fixtures, a11y, i18n)
→ publish to Gallery tpl:key@1.2.0   ──────►   browse / preview (read-only diagram, SOP, KPIs)
                                               → Install wizard:
                                                  1. map role placeholders → roles/groups/relations
                                                  2. set parameters (thresholds, SLAs, calendar)
                                                  3. choose optional steps / locales
                                                  4. test-run with fixtures
                                               → creates normal DRAFT in workspace
                                                  (lineage: template_ref = tpl:key@1.2.0)
                                               → fill SOP prompts → review → publish (normal governance)
publish tpl:key@1.3.0  ─────────────────────►  "Update available" offer:
                                                  3-way semantic diff (template@1.2.0 → @1.3.0
                                                  vs workspace's current version)
                                               → accept per change / ignore → becomes a new draft
```

- **Gallery scope.**
  - Platform gallery: curated by system admins, visible to all workspaces.
  - Workspace-private templates (P1): a workspace saves its own process as a template for reuse across its workspaces or departments.
  - Marketplace (P2/Phase 4): third-party submissions behind review.
- **System-admin controls.** Publish, deprecate or hide templates; restrict a template to selected workspaces or plans; view install counts (no tenant content).
- **Install is a copy.** The workspace owns the result. Gallery changes never mutate installed processes or running instances. Lineage (`template_ref@version`, parameter values, role mapping) is kept for upgrade offers and support.
- **Upgrade offers.** A 3-way semantic diff (IR + forms + rules + SOP). Changes the workspace already overrode are shown as conflicts. Accepting creates a new draft that goes through the workspace's normal review/publish. Upgrades are **never** auto-applied.
- **Uninstall.** Retiring the process is ordinary lifecycle (`Deprecated → Retired`). Lineage stays for audit. Template deprecation in the gallery only stops new installs and shows a banner on existing copies.
- **Permissions.**
  - `platform.template_admin` publishes to the gallery.
  - `workspace.admin` and `process.owner` can install.
  - Install requires that the mapped roles exist.

## 4. Catalogue

Columns: **Key**, **Flow summary**, **Key roles** (placeholders), **Default SLA** (business days unless stated), **Headline KPI**, **Alignment** (reference only), **W** = wave.
- **Wave 1** ships at pilot (12 templates).
- **Wave 2** ships at GA.
- **Wave 3** is industry packs, demand-gated.

Every row is a single standard process unless marked **(case)**.

### 4.1 HR (cross-industry)

| Key | Flow summary | Key roles | SLA | KPI | Alignment | W |
|---|---|---|---|---|---|---|
| `hr.employee-onboarding` | Offer accepted → new-hire form → parallel IT accounts / equipment / facilities / policy pack → manager Day-1 check → 30-day check-in | hr_coordinator, hiring_manager, it_provisioner, facilities | Ready ≥ 1 day before start | % new hires Day-1 ready | APQC 7.0 category; ISO 27001 A.6.1–6.3 | 1 |
| `hr.employee-offboarding` | Resignation/termination → manager confirm last day → parallel access revocation / asset return / final settlement / knowledge handover → exit interview | hr_coordinator, manager, it_provisioner, finance | Access revoked ≤ last day | % access revoked on time | ISO 27001 A.5.18, A.6.5 | 1 |
| `hr.leave-request` | Request → balance rule check → manager approval → HR record | employee, manager, hr_coordinator | 2 | Median approval time | — | 1 |
| `hr.expense-claim` | Claim + receipts → policy rule (limits by category) → manager → finance review above threshold → reimbursement record | employee, manager, finance_reviewer | 5 | % claims paid within SLA | SOX/COSO control activity (expense approval) | 1 |
| `hr.travel-request` | Trip request → cost estimate → manager → finance if international/above threshold → booking task | employee, manager, finance_reviewer, travel_desk | 3 | Pre-approval compliance % | — | 2 |
| `hr.policy-acknowledgement` | Policy published → audience resolved → acknowledge (optional quiz) → reminders → overdue report | policy_owner, employee | 10 | Acknowledgement coverage % | ISO 9001 7.3/7.5; ISO 27001 A.6.3 | 1 |
| `hr.performance-review-cycle` | Cycle start → self review → manager review → calibration → employee sign-off | hr_coordinator, employee, manager, calibration_panel | Cycle window | % completed on time | — | 2 |

### 4.2 Finance and procurement

| Key | Flow summary | Key roles | SLA | KPI | Alignment | W |
|---|---|---|---|---|---|---|
| `fin.purchase-requisition` | Requisition → budget check task → threshold rule picks approver chain → approve/reject/changes → PO creation task (hook: ERP) | requester, manager, finance_approver, cfo, buyer | 3 | Requisition cycle time | SOX/COSO authorization control; APQC 4.0 | 1 |
| `fin.po-approval` | PO draft → amount/category rule → approvals → vendor dispatch task | buyer, finance_approver | 2 | PO cycle time | SOX/COSO | 2 |
| `fin.vendor-onboarding` | Vendor request → due-diligence form (tax, bank, sanctions check task) → compliance review → finance master-data task → activation | requester, procurement, compliance_reviewer, finance_master_data | 10 | Onboarding lead time | ISO 27001 A.5.19–5.21; SOC 2 CC9.2 | 1 |
| `fin.invoice-exception` | Mismatch flagged → buyer investigates → resolve/credit note/reject → AP release | ap_clerk, buyer, finance_approver | 5 | Exceptions resolved in SLA % | SOX 3-way match control | 2 |
| `fin.budget-change` | Change request → impact form → owner + finance approval (two-level above threshold) → record | budget_owner, finance_approver, cfo | 5 | Approval time | SOX/COSO | 2 |
| `fin.month-end-close` | Scheduled trigger → parallel checklist tasks (reconciliations, accruals, review) → controller sign-off | accountant, controller | Close calendar (D+5) | Days to close | SOX/COSO; APQC 9.0 | 2 |
| `proc.rfq-sourcing` | Sourcing request → RFQ prep → quote collection (hook: supplier portal) → evaluation matrix → award approval | requester, buyer, evaluation_panel, approver | 15 | Sourcing cycle time | APQC 4.2 | 2 |
| `proc.contract-renewal` | Timer N days before expiry → owner decision (renew/renegotiate/exit) → legal review if changed → approval | contract_owner, legal_reviewer, approver | 30 before expiry | Renewals decided before expiry % | — | 2 |

### 4.3 IT service management (practice-aligned)

Generic ITSM practice names are used; no ITIL® text or certification claim (§8).

| Key | Flow summary | Key roles | SLA | KPI | Alignment | W |
|---|---|---|---|---|---|---|
| `it.access-request` | Request (system, role) → manager approval → system-owner approval for privileged roles → provisioning task (hook: IdP) → requester confirm | requester, manager, system_owner, it_provisioner | 2 | Time to access; % with dual approval for privileged | ISO 27001 A.5.15–5.18; SOC 2 CC6.2 | 1 |
| `it.access-recertification` | Scheduled campaign → per-owner review of entitlements → keep/revoke → revocation tasks → evidence export | system_owner, manager, it_provisioner, compliance | Campaign 15 | Recertification completion % | ISO 27001 A.5.18; SOC 2 CC6.3 | 2 |
| `it.change-request` | Change type rule (standard/normal/emergency) → risk form → CAB / ECAB approval → implement → review | requester, change_manager, cab_member, implementer | Normal: 5 | Change success rate | ISO 27001 A.8.32; SOC 2 CC8.1 | 1 |
| `it.incident` **(case)** | Intake → triage priority rule → assign → discretionary diagnosis/escalation → resolve → requester confirm → close | reporter, service_desk, resolver_group, incident_manager | P1: 4h; P3: 3d | MTTR by priority | ISO 27001 A.5.24–5.26 | 2 |
| `it.problem` **(case)** | Open from incidents → root-cause analysis → known error → change link → close | problem_manager, resolver_group | 20 | Problems with RCA % | — | 2 |
| `it.service-request` | Catalog item form → approval rule → fulfilment task → confirm | requester, approver, fulfiller | 3 | Fulfilment time | — | 2 |
| `it.asset-lifecycle` | Request → procure (links fin.purchase-requisition) → assign → periodic verification → retire/dispose with data wipe evidence | requester, asset_manager, it_provisioner | — | Assets with verified owner % | ISO 27001 A.5.9–5.11, A.7.14 | 2 |

### 4.4 Governance, risk and compliance

| Key | Flow summary | Key roles | SLA | KPI | Alignment | W |
|---|---|---|---|---|---|---|
| `grc.sop-recertification` | Review-due timer → owner reviews → no-change attest **or** edit draft → independent reviewer → publish at effective date → re-acknowledgement | process_owner, reviewer, compliance_officer | 10 | SOPs reviewed on time % | ISO 9001 7.5.2–7.5.3 | 1 |
| `grc.policy-exception` | Exception request → risk rating rule → owner + compliance approval → expiry timer → renewal/close | requester, policy_owner, compliance_officer | 5 | Expired exceptions open (target 0) | ISO 27001 5.1; SOC 2 CC5.3 | 1 |
| `grc.risk-assessment` | Scope → identify → likelihood/impact rule table → treatment plan approval → register update → review timer | risk_owner, risk_manager, approver | 15 | Risks with approved treatment % | ISO 27001 6.1.2–6.1.3 (clauses referenced) | 2 |
| `grc.internal-audit` **(case)** | Plan → fieldwork activities → findings → management response → follow-up → close | lead_auditor, auditee_owner, audit_committee | Per plan | Findings closed on time % | ISO 9001 9.2; ISO 27001 9.2 | 2 |
| `grc.control-testing` | Scheduled per control → test procedure task → evidence upload → reviewer sign-off → deficiency → remediation | control_owner, tester, reviewer | 10 | Controls tested in period % | SOX/COSO; SOC 2 | 2 |
| `grc.dsar` | Request intake → identity verification → scope search tasks → legal/exemption review → response pack → close (1-month clock) | privacy_officer, data_owner, legal_reviewer | 30 calendar days (GDPR); configurable | DSARs answered in time % | GDPR Art. 12–22; DPDP | 1 |
| `grc.data-breach` **(case)** | Detect → assess (72h timer starts) → containment → notify authority decision → notify individuals → post-incident review | incident_manager, privacy_officer, legal_reviewer, ciso | Authority notice ≤ 72h | Breaches assessed within 24h % | GDPR Art. 33–34; DPDP | 2 |

### 4.5 Quality

| Key | Flow summary | Key roles | SLA | KPI | Alignment | W |
|---|---|---|---|---|---|---|
| `qms.capa` **(case)** | §5.4 | — | — | — | ISO 9001 10.2; ISO 13485 8.5.2–8.5.3 | 1 |
| `qms.deviation` | Report → QA triage → impact assessment → disposition approval → CAPA link if needed | reporter, qa_reviewer, area_owner | 15 | Deviations closed in 30 days % | ISO 13485 8.3; GxP | 2 |
| `qms.document-change-control` | Change request → impact (linked SOPs/forms/training) → draft → review → approval → training/acknowledgement → effective | requester, document_owner, qa_approver | 10 | Change lead time | ISO 9001 7.5.3; ISO 13485 4.2.4 | 2 |
| `qms.supplier-quality` | Supplier qualification → audit task → scorecard rule → approve/conditional/reject → re-evaluation timer | procurement, qa_reviewer, approver | 20 | Approved suppliers current % | ISO 9001 8.4; ISO 13485 7.4 | 2 |
| `qms.customer-complaint` | Intake → classify → investigate → reportability decision → response → trend review | complaint_handler, qa_reviewer | 20 | Complaints closed in SLA % | ISO 9001 9.1.2; ISO 13485 8.2.2 | 2 |

### 4.6 Legal, sales and customer

| Key | Flow summary | Key roles | SLA | KPI | Alignment | W |
|---|---|---|---|---|---|---|
| `legal.contract-review` | Request + draft upload → risk rule (value, clause deviations) → legal review → business approval → signature task (hook: e-sign) → repository | requester, legal_reviewer, business_approver | 7 | Contract turnaround | — | 2 |
| `legal.nda` | Standard-template NDA form → auto-approve if unchanged template, else legal review → signature task | requester, legal_reviewer | 2 | NDA turnaround | — | 2 |
| `sales.discount-approval` | Quote + discount % → rule picks approver level → approve/counter → quote release | sales_rep, sales_manager, finance_approver | 1 | Approval time; avg discount | SOX revenue control (if applicable) | 2 |
| `cx.customer-complaint` **(case)** | Intake → categorise → investigate → resolution offer → closure survey | agent, team_lead | 5 | First-contact resolution % | — | 2 |
| `cx.returns-rma` | Return request → eligibility rule → authorise → receive/inspect → refund/replace | agent, warehouse, finance | 10 | RMA cycle time | — | 2 |

### 4.7 Facilities and operations

| Key | Flow summary | Key roles | SLA | KPI | Alignment | W |
|---|---|---|---|---|---|---|
| `ops.work-order` | Request → priority rule → assign technician → complete with photo evidence → requester confirm | requester, facilities_coordinator, technician | P1: 4h; P3: 5d | Work orders in SLA % | — | 2 |
| `ops.visitor-access` | Pre-registration → host approval → NDA/safety acknowledgement → badge task → check-out | host, reception, security | Same day | Pre-registered visitors % | ISO 27001 A.7.2 | 2 |
| `ops.safety-incident` **(case)** | Report → immediate actions → investigation → corrective actions → regulator-reportability decision → close | reporter, ehs_officer, area_manager | Investigation 10 | Incidents investigated in SLA % | ISO 45001 10.2 (referenced) | 2 |

### 4.8 Industry packs (Wave 3, demand-gated)

| Pack | Templates | Notes |
|---|---|---|
| **Pharma / life sciences (GxP)** | `gxp.change-control`, `gxp.deviation`, `gxp.capa` | `criticality: controlled`; **Part 11 signatures** (ESIG-01..05) at approval and closure; reason-for-change mandatory; audit-trail review task (Annex 11 §9); validation-pack fixtures. Aligns to 21 CFR 211/820 and ICH Q10 by reference. |
| **Healthcare** | `hc.patient-complaint` (case), `hc.credentialing` | Credentialing: application → primary-source verification tasks → committee approval → re-credentialing timer (24/36 months configurable). PHI variables are `restricted`; HIPAA alignment is by reference. |
| **Banking / FS** | `fs.kyc-review`, `fs.loan-exception`, `fs.sar-investigation` (case) | Periodic KYC refresh by risk-rating rule. SAR case is need-to-know with tipping-off protection: participants only, no requester visibility. |
| **Manufacturing** | `mfg.ecr-eco`, `mfg.ncr` | ECR impact (BOM, routing, inventory) → change board → ECO implementation tasks; NCR disposition (use-as-is/rework/scrap/return) with MRB approval. |
| **Public sector** | `gov.rti-foi-request`, `gov.grant-approval` | RTI (India: 30-day statutory clock) / FOI (configurable clock) with transfer and appeal branches; grant scoring rule table plus committee approval. |
| **Education** | `edu.admissions-exception` | Exception request → committee review → decision letter task. |

## 5. Worked templates

### 5.1 `hr.employee-onboarding` (Wave 1)

**IR outline:**
- `start(trigger: manual|api; inputForm: new-hire)`
- `human.task: hr-verify-docs` (hr_coordinator)
- `parallel.split` into three branches:
  - `human.task: it-accounts` (it_provisioner; hook: idp)
  - `decision: needs-equipment` (rule `equipment-by-role` → `human.task: equipment`)
  - `decision: include_facilities` → `human.task: desk-and-badge`
- `parallel.join`
- `notify: welcome-email` (new hire, external email)
- `wait.timer: until start_date` (business, calendar)
- `human.task: day1-check` (hiring_manager)
- `human.task: policy-ack` (new hire, links `hr.policy-acknowledgement` via `subprocess.call`)
- `wait.timer: P30D`
- `human.task: 30-day-checkin` (hiring_manager)
- `end`

**Forms.** `new-hire`:
- legal name
- preferred name
- personal email
- start date
- job title
- department
- hiring manager (principal)
- work location
- employment type (enum)
- salary (`restricted`, HR only)

`it-checklist`: accounts created (multi-select), laptop asset tag, notes.

**Rule table `equipment-by-role` (FIRST):**

| department | employment_type | → equipment |
|---|---|---|
| Engineering | any | laptop-high, monitor×2 |
| any | contractor | laptop-standard (loaner) |
| any | any | laptop-standard |

**RACI:**

| Activity | HR coord. | Hiring mgr | IT | Facilities | New hire |
|---|---|---|---|---|---|
| Verify documents | R/A | I | – | – | C |
| Accounts & equipment | I | C | R/A | – | – |
| Desk & badge | I | C | – | R/A | – |
| Day-1 check | C | R/A | I | I | C |
| Policy acknowledgement | A | I | – | – | R |

**SLAs.**
- Preparation branches complete ≥ `day1_lead_days` before start (breach → notify HR + manager).
- Each task due 2 business days after activation, with a reminder after 1 day.

**KPIs.**
- % Day-1 ready (all branches done before start).
- Median prep lead time.
- % policy acknowledged by day 5.
- 30-day check-in completion.

**SOP skeleton.** Purpose · Scope (employees, contractors) · Roles · Prerequisites (signed offer) · Procedure steps (mirrors IR nodes) · Exceptions (start-date change → owner reschedules timer; no-show → cancel instance with reason) · Records kept · `<<fill: local document checklist>>` · Review frequency (12 months).

### 5.2 `fin.purchase-requisition` (Wave 1)

**IR outline:**
- `start(inputForm: requisition)`
- `set: total = sum(items.amount)`
- `decision` via rule `approval-chain`
- `human.approval: manager` (`excludeInitiator`)
- approval at the next level when the rule requires it (`finance_approver`, then `cfo`; `excludeActorsOf` prior levels)
- on `changes_requested`, loop back to `human.task: revise` (bounded, `maxIterations: 5`)
- on `rejected`, `notify: rejection-with-reason`, then `end(completed)`
- on `approved`, `human.task: create-po` (buyer; hook: ERP `service.call` with `idempotencyKey = instance.id`)
- `notify: requester`
- `end`

**Form `requisition`:**
- line items: description, qty, unit price, currency, cost centre, GL code
- vendor (optional)
- needed-by date
- justification
- attachments (quotes)

**Rule table `approval-chain` (FIRST).** Parameters `t1` = 50,000 and `t2` = 500,000, in workspace currency, set at install.

| total | category | → levels |
|---|---|---|
| ≤ t1 | any | manager |
| ≤ t2 | any | manager, finance_approver |
| > t2 | any | manager, finance_approver, cfo |
| any | capex | + finance_approver (if not already) |

**RACI.**
- Requester: R (raise).
- Manager: A (≤ t1).
- Finance approver: A (> t1).
- CFO: A (> t2).
- Buyer: R (PO).
- Finance: I (all).

**SLAs.** 1 business day per approval level, with escalation to the approver's manager at 2 days. Whole process: 3 days.

**KPIs.**
- Requisition-to-PO cycle time.
- Approvals in SLA %.
- Rework rate (changes requested).
- Spend approved by level.

**SOP skeleton.** Purpose · Spend authority matrix (generated from the rule table) · Prohibited splitting of requisitions · Emergency purchases · Records · SoD statement.

### 5.3 `it.change-request` (Wave 1)

**IR outline:**
- `start(inputForm: change)`
- `decision: change_type`:
  - **standard** (pre-approved model): `human.task: implement`
  - **normal**:
    - `human.task: risk-assessment` (change_manager)
    - `decision: risk` (rule)
    - for low risk, `human.approval: change_manager`
    - for medium/high risk, `human.approval: cab` (`quorum: n_of_m{n: param cab_quorum, default 2}`, `excludeInitiator`)
  - **emergency**:
    - `human.approval: ecab` (quorum 1, `due: PT4H`)
    - `human.task: implement`
    - mandatory retrospective review
- `human.task: implement` (implementer; `boundary timeout` → `notify change_manager`)
- `decision: success?`:
  - fail → `human.task: backout`
  - success → `human.task: post-implementation-review`
- `end`

**Form `change`:**
- title
- type (enum)
- affected services/CIs (text list; hook: CMDB)
- description
- implementation plan
- backout plan
- planned window (start/end datetime)
- risk questionnaire (impact, likelihood, tested?, downtime?)

**Rule table `risk` (UNIQUE).**
- impact (low/med/high) × likelihood (low/med/high) → low/medium/high.
- The table is a 9-row matrix; high impact with untested → high.

**RACI.**
- Requester: R.
- Change manager: A (process).
- CAB: A (normal medium/high).
- ECAB: A (emergency).
- Implementer: R.
- Service owner: C.

**SLAs.**
- Normal approval: 5 days.
- Emergency approval: 4 hours.
- Post-implementation review: 5 days after implementation.

**KPIs.**
- Change success rate.
- Emergency change ratio.
- Changes with backout executed.
- Approval lead time.

**SOP skeleton.** Change types and criteria · CAB composition `<<fill>>` · Freeze periods `<<fill>>` · Standard change catalogue `<<fill>>` · Emergency rules · Records.

### 5.4 `qms.capa` (Wave 1, case-based)

A CAPA is a **case** (FR-C01), not a fixed flow. Investigations vary, but closure controls are fixed.

**Case template:**
- **Stages:** Initiation → Investigation → Action plan → Implementation → Effectiveness check → Closure.
- **Case file:**
  - source (complaint, deviation, audit finding, NCR)
  - problem statement
  - containment actions
  - root-cause method (5-Why / fishbone / other)
  - root cause
  - corrective actions
  - preventive actions
  - effectiveness criteria
  - evidence attachments
- **Mandatory activities:**
  - `qa-triage` (qa_reviewer, sets risk class by rule)
  - `rca-record`
  - `action-plan-approval` (`human.approval`, qa_approver, `excludeActorsOf` investigator)
  - `effectiveness-check` (timer: `P90D` default after implementation)
  - `closure-approval`
- **Discretionary activities** (investigator adds as needed):
  - interview
  - lab test
  - supplier request (links `qms.supplier-quality`)
  - document change (opens `qms.document-change-control` via `subprocess.call`)
  - training task
- **Milestones:** containment ≤ 48h for high risk; RCA ≤ 30 days; closure ≤ 90 days (risk-class rule sets the targets).
- **Closure controls:** all actions done, effectiveness criteria met (or a documented re-open), approver ≠ investigator.
- **Signatures:** `simple` by default; the GxP pack switches to `part11` with meaning "Approved: CAPA closure".

**RACI.**
- Initiator: R (raise).
- QA reviewer: R (triage).
- Investigator: R (RCA, actions).
- Action owners: R.
- QA approver: A (plan, closure).
- Management: I (trend review).

**KPIs.**
- CAPAs closed in target %.
- Overdue CAPAs.
- Recurrence rate (same root-cause category within 12 months).
- Effectiveness-check pass rate.

**SOP skeleton.** CAPA trigger criteria · Risk classification `<<fill>>` · RCA methods · Effectiveness verification · Escalation to management review · Records & retention.

### 5.5 Wave 1 set (12)

1. `hr.employee-onboarding`
2. `hr.employee-offboarding`
3. `hr.leave-request`
4. `hr.expense-claim`
5. `hr.policy-acknowledgement`
6. `fin.purchase-requisition`
7. `fin.vendor-onboarding`
8. `it.access-request`
9. `it.change-request`
10. `grc.sop-recertification`
11. `grc.policy-exception`
12. `grc.dsar`
13. `qms.capa` (case)

`qms.capa` depends on case management (P1). If case management slips past pilot, it moves to Wave 2 and the pilot set is the first 12. This covers the five pilot processes from 07-ROADMAP: purchase, onboarding, access, SOP recertification, incident/exception (via `grc.policy-exception`).

### 5.6 As built: built-in request types (Procera 0.2)

Until the template gallery installs processes, four simplified Wave 1 templates ship as built-in request types (`crates/requests/builtin/*.json`, compiled at boot and pinned by checksum; A-45). Routing uses CEL decisions, not rule tables. Every approval excludes the initiator; later levels exclude earlier deciders; the fallback is role `process.owner`; changes requested loop to a `revise` task for the requester (`maxIterations: 5`).

| Key (display id) | Start form | Routing |
|---|---|---|
| `hr.leave-request` (LR) | type (annual, sick, unpaid, other), first day, last day, reason | Manager (P2D business) |
| `hr.expense-claim` (EC) | amount (EUR), category, description, date | Manager; then finance (role `finance.approver` or group `Finance`) when the amount is over 500 |
| `fin.purchase-requisition` (PR) | amount (INR), vendor, cost centre, justification | Manager; finance above 50,000; then the `CFO` position above 500,000 (§5.2 simplified: no line items, no PO task) |
| `it.access-request` (AR) | system, access level, sensitivity, beneficiary (a member), justification | Beneficiary's manager, or role `system.owner` then group `Security` for high sensitivity; then group `IT` provisions (a human task with a ticket reference) and the beneficiary is notified |

Onboarding (§5.1) is deferred: it needs parallel tasks and a start-date timer in the request UI.

## 6. Quality bar and governance of templates

| Gate | Requirement |
|---|---|
| Domain review | Named SME reviewer per domain (HR, finance, ITSM, GRC, QMS, industry). Sign-off recorded in the template changelog. |
| Compile & lint | Compiles under the current and previous IR compiler. Zero errors. Warnings justified in the manifest. |
| Fixtures | ≥ 2 fixtures (happy path plus one exception path). Controlled templates also cover SoD-violation and timeout paths. CI runs them as test-runs. |
| Accessibility | Forms pass automated axe checks plus a manual keyboard pass (A11Y-01). Labels and help text present; no colour-only status. |
| Localisation | `en` complete. Strings externalised (ICU). Wave 1 also ships `hi`. Dates, currency and thresholds locale-aware. |
| Originality | No copied framework text. Alignment references only. Attribution block present where a framework requires it (§8). |
| Privacy | Personal data variables classified. `restricted` used for salary, health and similar fields. No restricted data in notify templates. |
| Versioning | Semver. **Major:** removed/renamed variables, changed role placeholders, IR structure changes that break upgrade diffs. **Minor:** new optional steps or parameters. **Patch:** text and fixes. |
| Deprecation | 2 minor releases' notice in the gallery. Installed copies untouched. Replacement template linked. |
| Ownership | Each template has a maintainer and a review-due date (12 months), managed by `grc.sop-recertification` run on the platform itself (dogfooding). |

## 7. Backlog items

| ID | Title | Wave | Size | Depends on |
|---|---|---|---|---|
| TPL-001 | Template manifest schema v1 + validator | 1 | M | IR v1 compiler |
| TPL-002 | Gallery storage, signing and platform-admin publish UI | 1 | M | TPL-001, platform admin console |
| TPL-003 | Install wizard (role mapping, parameters, optional steps, test-run) | 1 | L | TPL-001, authz roles/groups, test-run (FR-E11) |
| TPL-004 | Lineage tracking + installed-from badge | 1 | S | TPL-003 |
| TPL-005 | Template CI: compile, fixtures, a11y, i18n lint | 1 | M | TPL-001, conformance harness |
| TPL-006 | SOP skeleton block model with `<<fill>>` prompts + publish blocker | 1 | M | SOP content model (ADR-0013) |
| TPL-007 | KPI definitions from domain events + default dashboards | 1 | M | Insights event model |
| TPL-008 | Author Wave 1 HR templates (5) | 1 | L | TPL-001, TPL-006 |
| TPL-009 | Author Wave 1 finance/IT templates (4) | 1 | L | TPL-001, rule tables |
| TPL-010 | Author Wave 1 GRC templates (3) | 1 | M | TPL-001 |
| TPL-011 | Author `qms.capa` case template | 1 (or 2) | M | Case management (FR-C01) |
| TPL-012 | Hindi locale bundles for Wave 1 | 1 | S | TPL-008..011 |
| TPL-013 | Upgrade offer: 3-way semantic diff + selective accept | 2 | L | TPL-004, semantic diff (FR-G05) |
| TPL-014 | Workspace-private templates ("save as template") | 2 | M | TPL-001 |
| TPL-015 | Author Wave 2 templates (~27) | 2 | L | TPL-005 |
| TPL-016 | Hook-point framework (connector replaces human fallback) | 2 | M | Connector framework |
| TPL-017 | APQC PCF taxonomy import for the landscape (with attribution) | 2 | S | Legal confirmation of PCF licence ⚠ |
| TPL-018 | GxP pack with Part 11 signatures + validation fixtures | 3 | L | ESIG-01..05 |
| TPL-019 | Healthcare, FS, manufacturing, public sector, education packs | 3 | L | TPL-015, demand gate |
| TPL-020 | Template marketplace (third-party submission + review) | 3 | L | TPL-002, Phase 4 gate |

## 8. Framework usage terms (why "aligned, not copied")

| Framework | Owner / terms found | How we use it |
|---|---|---|
| **APQC PCF** | Copyrighted by APQC. Recent versions are offered under a perpetual, worldwide, royalty-free licence to use, copy, modify and create derivative works, **with mandatory attribution in all copies and derivatives**. Industry PCFs co-developed with IBM carry specific licence terms; v7.2.2 updated the licence/attribution language. ⚠ Exact current text (cross-industry v7.4, Aug 2024) was not retrievable (403); legal to read the licence in the download before TPL-017. | Category references in `alignment`. Optional taxonomy import with the required attribution in the UI and NOTICE. |
| **ITIL® 4** | Trademark of AXELOS Ltd / PeopleCert. PeopleCert licenses use of ITIL trademarks and text, and runs tool-vendor accreditation. | Generic practice names only (incident, change, problem, service request). No ITIL text, no "ITIL-compliant" claims, ® not used as a product claim. Accreditation is a later commercial option. |
| **COBIT 2019** | ISACA requires an annual licence when COBIT content is used in commercial works or products, including software. Derivative works and display through products are unauthorised without permission. | **No COBIT content or ids.** Mention as a customer-side mapping only. |
| **SCOR DS** | ASCM: Creative Commons BY-NC-ND. No commercial use, no derivatives. | **Not used** in templates. |
| **ISO 9001 / 27001 / 13485 / 45001** | ISO copyright. Standard text cannot be reproduced. Clause numbers and titles as references are normal practice. | Clause references only. Wording is our own. |
| **COSO / SOX** | SOX is US law (public). The COSO framework is copyrighted by COSO. ⚠ Specific reuse terms not verified. | Reference "SOX control activity" generically. No COSO text. |
| **21 CFR Part 11 / 211 / 820, HIPAA, GDPR, DPDP** | Statutes and regulations, freely citable. | Cited directly. Requirement IDs in [14-DATA-GOVERNANCE](14-DATA-GOVERNANCE-AND-COMPLIANCE.md). |
| **ICH Q10, EU GMP Annex 11** | Published guidance, freely available. Reproduction of large portions is not needed. | Referenced. |

## 9. Sources

- [APQC PCF Cross-Industry v7.4 (PDF listing)](https://www.apqc.org/resource-library/resource-listing/apqc-process-classification-framework-pcf-cross-industry-pdf-12)
- [APQC PCF overview, cross-industry and industry versions](https://www.apqc.org/node/245395)
- [APQC PCF Banking v7.2.2 (licence/attribution update note)](https://www.apqc.org/resource-library/resource-listing/apqc-process-classification-framework-pcf-banking-pcf-pdf-0)
- [PeopleCert third-party product licensing](https://peoplecert.org/Organizations/Services/third-party-product-licensing-service)
- [ITIL (ownership history)](https://en.wikipedia.org/wiki/ITIL)
- [ISACA intellectual property and licensing (COBIT)](https://www.isaca.org/about-us/intellectual-property-and-licensing)
- [ISACA COBIT usage guidelines](https://www.isaca.org/about-us/cobit-usage-guidelines)
- [ASCM SCOR DS open access guidance](https://www.ascm.org/corporate-solutions/standards-tools/scor-ds/open-access-guidance/)
- [21 CFR Part 11 (eCFR)](https://www.ecfr.gov/current/title-21/chapter-I/subchapter-A/part-11)
