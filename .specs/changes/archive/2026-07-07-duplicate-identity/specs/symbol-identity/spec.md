# Delta for Symbol Identity

## MODIFIED Requirements

### Requirement: Workspace-namespaced identity

> Previously: identical contract text; the first scenario's WHEN clause read "both are indexed into the store", implying a single store hosting two workspaces — a shape that only ever worked by riding a since-fixed store-accumulation defect.

The system SHALL qualify every canonical identity with the identity of the workspace it was indexed under, such that identical symbol descriptors originating from two different workspaces are never equal.

Serves: twins-stay-distinct

#### Scenario: Same descriptor in two workspaces stays distinct

- **GIVEN** two workspaces that each define a symbol with the same module path and qualified name
- **WHEN** each workspace is indexed into its own store
- **THEN** the two symbols have distinct canonical identities and neither query nor reference resolution conflates them
