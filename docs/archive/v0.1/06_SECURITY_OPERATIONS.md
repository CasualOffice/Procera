# Security, reliability and operations

## Threat model
Assets: employee data, workflow variables, confidential approvals, compliance evidence, integration credentials, audit trail. Adversaries: external attackers, compromised tenants, malicious insiders, compromised connector, leaked webhook URL. Trust boundaries: browser/API, API/DB, runtime/connector, SaaS integrations, tenant-to-tenant, admin/support access.

## Mandatory controls
- Auth: OIDC authorization code + PKCE for browser; short-lived tokens; SAML via trusted IdP broker; SCIM deprovisioning and group sync.
- Authorization: policy enforcement at every read/write and background transition; object and field-level access; tenant context derived from verified claims; negative tests for IDOR and cross-tenant joins.
- Secrets: KMS/Vault envelope encryption, rotation, no secrets in model JSON or logs, outbound connector secret scopes.
- Webhooks: HMAC signatures, timestamp window, nonce/replay protection, idempotency, optional mTLS and egress controls.
- Evidence: checksums, malware scanning, retention/hold, short-lived scoped download URLs, redaction on export.
- Audit: actor, subject, action, reason, timestamp, before/after hashes, request/trace IDs; append-only with tamper-evident export strategy. Do not claim legal immutability without WORM controls.
- Supply chain: SBOM, pinned dependencies, vulnerability scanning, signed release artifacts, third-party license inventory.
- AI: prompt-injection defenses, least-privilege tools, human approval before publishing or acting, tenant data separation.

## Service targets (proposed, subject to sizing)
Pilot: 99.9% monthly API availability; production target 99.95% after HA readiness. RPO <= 15 min, RTO <= 2 hr initially; test restores quarterly. P95 read <300 ms, P95 state transition <500 ms excluding connector latency under defined load. 99.99% scheduled timers initiated within 60 sec at nominal load (validate by test).

## Observability
Metrics: transition rate/error, runnable token backlog, timer lateness, outbox lag, integration retries/DLQ, tenant throttling, auth denies, DB lock contention, version publication failure, SOP review overdue. Traces correlate instance_id and trace_id; do not log PII or secret form values. Alerts on sustained backlog, timer drift, replica lag and dead-letter spikes.

## Recovery and resilience tests
Kill API during commit; kill worker after external call but before ack; duplicate Kafka message; DB failover; lost broker connectivity; outbox replay; clock skew; expired leases; missing assignee; revoked employee; rotated integration credential; corrupted attachment; restoration from snapshot + WAL; 30-day idle instance recovery.

## Security test matrix
SEC-01 cross-tenant process read denied; SEC-02 cross-tenant task approve denied; SEC-03 field-level redaction; SEC-04 revoked identity cannot approve; SEC-05 webhook replay rejected; SEC-06 connector SSRF prevented; SEC-07 audit export scoped; SEC-08 privilege escalation via delegation blocked; SEC-09 unsafe BPMN/script content rejected; SEC-10 signed URL expiry and tenant binding verified.

## Compliance readiness
Support evidence and mappings for ISO 27001, SOC 2 and internal controls, but do not claim certification or compliance solely because the product includes controls. Define customer data processing roles, retention and deletion obligations by jurisdiction before release.
