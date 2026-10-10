# API contracts and workflow examples

## Start an instance
```http
POST /v1/instances
Authorization: Bearer <token>
Idempotency-Key: 6d0d0f...
Content-Type: application/json

{"processId":"8b3...","input":{"employeeId":"u-42","department":"IT"}}
```
Response `201`: `{"id":"inst-123","definitionVersion":4,"state":"RUNNING","revision":1}`. Duplicate same actor+key+body returns prior result; changed body yields 409.

## Decide a work item
```http
POST /v1/work/work-123/decisions
Idempotency-Key: approve-work-123-1
If-Match: "7"

{"decision":"APPROVE","comment":"Budget confirmed","fields":{"costCenter":"ENG"}}
```
Response `200` with new revision; stale revision `409`, forbidden `403`, missing or hidden `404` per disclosure policy.

## Publish a process
```http
POST /v1/processes/proc-123/publish
If-Match: "draft-11"

{"versionId":"v-4","effectiveAt":"2026-11-01T00:00:00Z","changeReason":"Approval threshold updated"}
```
Requires author/reviewer separation when policy mandates; immutable published version hash returned.

## Event envelope
```json
{"event_id":"uuid","schema_version":1,"event_type":"work.decision.recorded.v1","tenant_id":"tenant-uuid","aggregate_id":"inst-123","aggregate_revision":8,"occurred_at":"2026-10-09T10:00:00Z","trace_id":"trace","actor_ref":"user-42","data":{"work_id":"work-123","outcome":"APPROVED"}}
```

## Process example: purchase approval
Start -> request form -> DMN-like threshold rule (<= 50000 manager; > 50000 finance) -> approval -> conditional approved/rejected -> notification -> end. Business rules version pinned; no arbitrary code evaluation. Audit records exact decision rule version and evaluated inputs, redacted in nonprivileged views.

## Process example: SOP recertification
Scheduled trigger -> owner review -> update draft -> independent compliance approval -> publish at effective date -> request acknowledgement -> report overdue attestations. Governance publication does not automatically mutate running workflow instances.

## Process example: incident case
Create case -> attach evidence -> assign investigator -> discretionary actions (interviews, remediation, legal review) -> mandatory closure checklist -> controlled closure -> retention schedule. Case work may invoke a predefined BPMN workflow but is not forced into a fixed sequence.
