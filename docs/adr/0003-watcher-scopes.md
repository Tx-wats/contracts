# ADR 0003: Watcher Scopes and Their Enforcement Point

## Status

Accepted

## Context

`WatcherRegistry` stores an optional scope per watcher
(`DataKey::WatcherScope`, set with `set_watcher_scope`) and exposes
`is_watcher_authorized_for(watcher, target)`. An empty or absent scope means
the watcher is unrestricted. `AlertRegistry`, however, only calls
`is_watcher_authorized(querier)` in `assert_watcher_if_configured`, so scopes
are not enforced anywhere yet.

Open questions:

1. Check the target once per call, or filter every returned alert?
2. What does a cross-contract call per result cost?
3. How do unscoped watchers interact with list calls that span many targets?

## Decision

### Enforcement point: per-call target check in AlertRegistry

Enforcement lives in `AlertRegistry`, the contract that serves the data.
`WatcherRegistry` remains the source of truth for scopes and only answers
`is_watcher_authorized_for`.

- Getters keyed by a single target contract (for example
  `get_active_contract_alerts_paginated`, `get_active_contract_alerts_paged`,
  `get_active_alerts_for_contract`) make **one** cross-contract call,
  `is_watcher_authorized_for(querier, target_contract)`, in place of
  `is_watcher_authorized(querier)`, and fail with `NotAWatcher` if it returns
  `false`.
- Getters keyed by a single alert ID (`get_alert`, `get_alert_active`, …) load
  the record and run the same check against its `target_contract`.

### No per-result filtering

We reject filtering each result through its own cross-contract call. Each call
costs a contract invocation plus storage reads in `WatcherRegistry`; on a page
of N alerts that is N extra invocations, which quickly hits Soroban
per-transaction CPU and read limits and makes page cost depend on data rather
than on the page size.

### Multi-target and owner-keyed list calls

Calls whose results can span several targets (owner-indexed or global lists)
are allowed only for **unscoped** watchers. A scoped watcher calling them gets
`NotAWatcher`; it must use the per-target getters for each target in its
scope. This keeps the check to one cross-contract call per request.

### Unscoped watchers

An unscoped watcher (absent or empty scope) passes every check, matching the
current `is_watcher_authorized_for` semantics. Existing deployments therefore
see no behaviour change until an admin sets a scope.

## Consequences

- Each gated read costs at most one cross-contract call, as today.
- Scoped watchers need one call per target they watch.
- Removing every target from a scope makes the watcher unrestricted again;
  admins who want to revoke access must unregister the watcher instead.
- Enforcement is a follow-up change in `AlertRegistry` (Issue 61); this ADR
  is the reference for that work.
