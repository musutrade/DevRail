# RUSTSEC-2023-0071 Risk Acceptance

Status: time-bounded acceptance, not a vulnerability resolution

Owner: DevRail maintainers

Review date: 2026-12-31

## Scope

The advisory is reachable in the backend dependency graph through:

`web-push -> jwt-simple -> superboring -> rsa`

The current RustSec record reports that no patched `rsa` release is available.
The dependency is retained because DevRail's optional Web Push worker uses
`web-push` for VAPID signatures.

## Compensating controls

- Web Push delivery starts only when the public key, private key, and subject
  are all configured; otherwise the worker exits without sending messages.
- VAPID private material is supplied through deployment secrets and is never
  accepted from an API request.
- The notification worker is not exposed as an inbound HTTP endpoint.
- The audit and deny checks keep this advisory explicit and emit an ignored
  advisory note on every run.

## Required follow-up

Before 2026-12-31, maintainers must re-check the RustSec advisory and the
`web-push` dependency graph. Replace or patch the dependency if a supported
constant-time implementation is available; otherwise renew this acceptance
with a new owner, evidence, and expiry. Removing the ignore without a
replacement is expected to fail the supply-chain gate.
