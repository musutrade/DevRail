## 1. Security and data boundaries

- [x] 1.1 Remove workflow visibility guards and correct RustSec policy path.
- [x] 1.2 Add scoped external-review target checks and organization-keyed identity migration.
- [x] 1.3 Bind Webhook target/event identity to signed payload and organization-scoped update.
- [x] 1.4 Reject approval self-decision and make approval recovery/start claims atomic.

## 2. Run integrity and verification

- [x] 2.1 Add quality-gate execution claim and terminal-state guards.
- [x] 2.2 Remove identified connection-pool double acquisitions without changing business semantics.
- [x] 2.3 Add regression tests, update status/evidence docs, and run all repository gates.
