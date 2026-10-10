# ADR-0014 — Apache-2.0, DCO, permissive-only dependencies

Status: **Proposed (OQ-06)** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Wedge is a genuinely open product; licence traps found in object storage, cache, search, IdP and modeler ecosystems.

## Decision
Source under Apache-2.0; contributions via DCO sign-off; trademark retained for the brand; CI blocks AGPL/GPL(linked)/SSPL/BSL/BUSL/SUL/Commons-Clause in shipped artefacts. Business model (hosted service, support, enterprise add-ons) decided under OQ-06 without changing core licence.

## Alternatives considered
Open core with proprietary enterprise modules (possible later, must not cripple core). AGPL (rejected: hinders embedding/adoption).

## Consequences
Licence gate in CI; NOTICE file maintained.
