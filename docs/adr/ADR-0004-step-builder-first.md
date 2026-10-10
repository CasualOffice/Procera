# ADR-0004 — Step builder is the default authoring UX; BPMN is optional

Status: **Accepted** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Target users are business people across startups to MNCs; BPMN notation is a barrier for most, but analysts and migrators need it.

## Decision
Default editor is a structured step builder with a flow-canvas view. BPMN view (diagram-js, ADR-0018) and BPMN XML import/export of the supported subset are secondary, with compatibility reports.

## Alternatives considered
BPMN-first modeler (rejected: usability). No BPMN at all (rejected: migration and analyst needs).

## Consequences
Must maintain a lossless mapping canvas ↔ IR and a documented IR ↔ BPMN subset mapping.
