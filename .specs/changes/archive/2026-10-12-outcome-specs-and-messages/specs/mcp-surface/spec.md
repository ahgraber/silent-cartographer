# Delta for MCP Surface Specification

## MODIFIED Requirements

### Requirement: A lifecycle operation proceeds only on confirmed consent

> Previously: only a scenario stated that no confirmation request is issued for an invocation that lacks its acknowledgment.

A lifecycle tool SHALL obtain the caller's confirmation before performing its operation, through the mechanism the negotiated protocol era defines, and SHALL leave every store, file, and repository unchanged when that confirmation is declined or cancelled.
A lifecycle tool SHALL NOT ask for confirmation of an invocation that lacks its acknowledgment.
When the connected client offers no elicitation capability, the tool SHALL NOT perform its operation and SHALL NOT issue a request the client cannot answer; it SHALL refuse, state that the client cannot be asked to confirm, and name the command that performs the operation.

Serves: self-service-setup, every-client

#### Scenario: A handshake-era client is asked and confirms

- **GIVEN** a handshake-era client offering an elicitation capability
- **WHEN** an acknowledged lifecycle tool is invoked and the caller confirms
- **THEN** the operation is performed and the tool returns the command's answer

#### Scenario: A modern client is asked and confirms

- **GIVEN** a client on the 2026-07-28 revision offering an elicitation capability
- **WHEN** an acknowledged lifecycle tool is invoked and the caller confirms
- **THEN** the operation is performed and the tool returns the command's answer

#### Scenario: A declined confirmation performs nothing

- **GIVEN** a client offering an elicitation capability
- **WHEN** an acknowledged lifecycle tool is invoked and the caller declines
- **THEN** no store, file, or repository is changed and the tool reports the operation as not performed

#### Scenario: A client that cannot be asked is refused with its remedy

- **GIVEN** a client offering no elicitation capability
- **WHEN** an acknowledged lifecycle tool is invoked
- **THEN** no store, file, or repository is changed, no unanswerable request is issued, and the refusal states that the client cannot be asked to confirm and names the command that performs the operation

#### Scenario: The acknowledgment is checked before anything is asked

- **GIVEN** a client offering an elicitation capability
- **WHEN** a lifecycle tool is invoked without its acknowledgment parameter set
- **THEN** the invocation is refused without a confirmation request being issued
