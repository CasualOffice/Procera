# ADR-0018 — UI stack and canvas libraries

Status: **Accepted** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Need accessible, distinctive UI; licence-clean diagramming.

## Decision
React + TypeScript; React Aria primitives; TanStack Table/Virtual for the DataGrid; React Flow (MIT) for the step canvas; diagram-js + bpmn-moddle (MIT) with our own renderer for the BPMN view; bpmn-visualization (Apache-2.0) read-only; no bpmn-js/dmn-js/form-js (watermark clause).

## Alternatives considered
bpmn-js (rejected: watermark), JointJS+ (commercial).

## Consequences
Clean-room rule: no code copied from bpmn-js. Visual language per 05-UX §7.
