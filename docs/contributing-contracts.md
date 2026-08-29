---
title: Contributing (Contracts)
---

# Contributing to Contracts (Soroban-specific)

This guide gives contributor-focused advice for writing and reviewing smart-contract code for this repository (Soroban / Rust).

## Principles
- Keep contracts minimal and auditable: small surface area reduces security risk.
- Be explicit about authorization: require and document who can call admin-only entrypoints.
- Optimize for storage cost and cleanup: persistent storage is paid for and permanent until removed.

## Authorization patterns
- Use host `env.require_auth()` early in entrypoints to assert signer privileges.
- Prefer capability-based checks (explicit addresses or allowlists) over implicit assumptions.
- Example pattern:

```rust
// At start of public entrypoint
env.require_auth(&admin_address);
// proceed with state mutation
```

- For multi-signature or timelock patterns, implement clear on-chain checks and fail closed.

## Storage tiers & keys
- Separate key namespaces by purpose (e.g. `meta:...`, `data:...`, `index:...`) to avoid collisions.
- Keep keys compact (fixed-size bytes where possible) — long strings increase storage cost.
- Avoid storing large vectors or blobs in a single key; use paginated indices if needed.
- When storing mappings, also store reverse/index keys to support efficient queries and deletions.

## TTL / expiration management
- Soroban does not automatically expire contract storage. Implement TTL by storing an expiration ledger sequence alongside the value.
- Enforce TTL on reads: treat expired items as non-existent and optionally clean them up lazily.
- Provide a public `cleanup_expired()` function that can be called to reclaim storage and keep costs bounded.

Example TTL approach:

```rust
// store: (value, expires_at_ledger)
// read path: if now_ledger > expires_at => treat missing
// cleanup path: caller iterates keys and removes expired entries
```

## Gas & performance
- Minimize storage writes — each write increases cost. Batch writes only when necessary.
- Prefer integer or compact encodings for on-chain counters and timestamps.
- Avoid unbounded loops in contract entrypoints; make expensive maintenance work callable separately and resumable.

## Testing and audits
- Add unit tests for all auth checks, edge cases, and TTL behavior. Use `integration-tests` harness for cross-contract interactions.
- Add property-style tests for invariants (e.g., total supply never exceeds cap, expired keys inaccessible).
- Document assumptions in method docs and `docs/` files.

## Review checklist (quick)
- Is there an explicit `require_auth` where state is mutated?
- Are storage keys namespaced and compact?
- Is there a plan for TTL/cleanup if data can grow over time?
- Are loops bounded or paginated?
- Are public invariants tested?
- Are monorepo `CHANGELOG.md` and per-package changelogs (`bindings/*/CHANGELOG.md`) updated?

## Helpful references
- Soroban SDK docs and examples (check upstream docs).
- This repo's `integration-tests` for cross-contract examples.
- [docs/compatibility.md](compatibility.md) for bindings and sister repo release instructions.

