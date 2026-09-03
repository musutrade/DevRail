## ADDED Requirements

### Requirement: Signed webhook target and identity

Webhook MUST derive repository target and event identity from the HMAC-authenticated body, reject an empty secret or missing identity, and apply an organization predicate before updating a pull request.

#### Scenario: Header target is tampered

- **WHEN** a valid body is paired with a different repository header
- **THEN** the request is rejected and no pull request or notification row changes.

### Requirement: Scoped external review synchronization

External review synchronization MUST prove that the review participant, task, project and repository belong to the same organization and requested target before writing or soft-deleting comments.

#### Scenario: Foreign review ID is supplied

- **WHEN** an actor submits a review ID outside the requested repository
- **THEN** the service fails without writing or deleting external comments.

### Requirement: Approval and gate terminal integrity

An approval requester MUST NOT decide their own approval. Quality gate failure MUST NOT rewrite a terminal run or a succeeded task, and concurrent gate execution MUST have one durable owner.

#### Scenario: Concurrent execution or late failure

- **WHEN** two callers execute gates or a gate finishes after another terminal transition
- **THEN** only one command execution is recorded and the prior terminal conclusion remains unchanged.
