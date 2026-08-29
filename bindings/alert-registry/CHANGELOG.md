# Changelog — `@tx-wat/alert-registry-bindings`

All notable changes to the `@tx-wat/alert-registry-bindings` TypeScript package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `getAlertIdsByOwner({ owner })` for fetching just the owner's alert IDs without the full configs.
- Type declarations and client methods for two-phase webhook rotation:
  - `proposeWebhook({ caller, id, new_webhook_hash })`
  - `confirmWebhook({ caller, id })`
- `renewAlertTtl({ caller, id })` for renewing alert TTL without altering modification timestamps.
- `bumpAlert({ id, ttl })` for permissionless keeper-based TTL bumping.
- `getAlertsModifiedSince({ since })` for incremental alert synchronizations.
- `getContractAlertsPaginated({ querier, target_contract, offset, limit })` and `getAlertsByOwnerPaginated({ querier, owner, offset, limit })`.
- Vitest unit tests covering parameter encoding and method bindings.

### Changed
- `getAdmin()` now surfaces `NotInitialized` as a typed contract error instead of an untyped panic, matching `WatcherRegistry`'s `getAdmin()` convention.
- `setWatcherRegistry({ admin, watcher_registry })` now probes the target contract and rejects a misconfigured address with `InvalidWatcherRegistry` at call time instead of leaving gating silently broken until the next read query.

## [0.1.0] - 2025-05-28

### Added
- Initial release of TypeScript bindings for the `AlertRegistry` Soroban contract.
- Client methods for `initialize`, `transferAdmin`, `setWatcherRegistry`, `getWatcherRegistry`, `isWatcherGatingEnabled`, `setPerOwnerAlertLimit`, `getPerOwnerAlertLimit`.
- Core alert lifecycle methods: `registerAlert`, `updateAlert`, `updateWebhook`, `updateLabel`, `updateTargetContract`, `deactivateAllAlerts`, `removeAlert`, `removeAlertByAdmin`.
- Query methods: `getAlert`, `getAlertActive`, `getAlertCount`, `getActiveAlertCount`, `getAlertsForContract`, `getActiveAlertsForContract`, `getAlertsByOwner`.
- Generated TypeScript interfaces and error mappings for `ContractError`.
- Peer dependency support for `@stellar/stellar-sdk` (>=12.0.0).

[Unreleased]: https://github.com/Tx-wat/stellar-txwatch-contracts/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Tx-wat/stellar-txwatch-contracts/releases/tag/v0.1.0
