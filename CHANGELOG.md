# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed — bindings & docs

- **Published bindings baked a fake contract ID.** `publish-bindings.yml` passed
  a literal `CXXX…` placeholder to `stellar contract bindings typescript`, and
  `bindings/watcher-registry/src/index.ts` shipped the same placeholder for both
  networks, so the package README's own usage example was non-functional as
  published. CI now resolves the real address from `DEPLOYMENTS.md` via
  `scripts/deployment_address.py`, **fails the publish job loudly** if it is
  still a placeholder, overlays it into `src/index.ts`, and runs a post-publish
  smoke test that instantiates the client against the deployed testnet address.
  `networks.testnet.contractId` now holds the real testnet address;
  `networks.mainnet.contractId` is `""` until mainnet is deployed (issue #90).
  (issues #72, #73)
- **`docs/watcher-registry.md` documented typed errors as raw panics.** Each
  `**Panics:**` note is rewritten to describe the `Result<(), ContractError>` /
  `try_*` pattern, and a new **Errors** table lists all five `ContractError`
  variants with their discriminants. (issue #65)
- **`docs/events.md` did not explain `replace_watcher`'s dual event.** A single
  `replace_watcher` emits both `watcher.remove` and `watcher.replace` for the
  same `old_watcher`; this is now documented with a worked indexer
  de-duplication example. (issue #70)

### Fixed — build and test suite restored

The workspace did not compile and the test suite had never run. Both are now green.

- **`alert-registry` did not build at all.** `panic_with_error!` was used without
  being imported, and `register_alert` returned a bare `u64` from a function
  declared `-> Result<u64, ContractError>`. No WASM artifact could be produced
  from this repository before this change.
- **`Cargo.lock` was git-ignored** while the toolchain was pinned to Rust 1.85,
  so every clean checkout re-resolved dependencies and eventually picked crates
  requiring Rust 1.88 — failing before compilation began. The lockfile is now
  committed, the workspace declares `rust-version`, and the MSRV-aware resolver
  (`resolver = "3"`) keeps resolution inside the pinned toolchain.
- **CI built the whole workspace for `wasm32`**, which pulled `test-utils` in and
  force-enabled soroban-sdk's `testutils` (requires `std`) on a `no_std` target.
  WASM builds now target the two contract crates explicitly.
- All CI invocations use `--locked` so dependency drift fails loudly instead of
  silently changing what is built.
- The test suite compiles and passes for the first time: **169 tests**, plus
  `clippy -D warnings` and `cargo fmt --check` clean.

### Security

- **RUSTSEC-2026-0009 (`time`)** — the advisory scan could not run at all
  (cargo-audit needs rustc >= 1.88) and the patched `time` release needs the
  same, so the 1.85 pin made the advisory unfixable. The toolchain is bumped to
  1.90.0 with a declared MSRV of 1.88, `time` is updated to 0.3.55, and the
  audit runs as its own CI job on stable so it is never coupled to the
  contracts' MSRV again. The crate reaches the workspace only through
  `soroban-ledger-snapshot` (host/test tooling) and is not part of the deployed
  `wasm32` artifact.

### Fixed — correctness

- **`update_alert` silently discarded rule validation.** It called
  `validate_rules` and dropped the returned `Result`, so an alert could be
  updated with rule descriptors that `register_alert` rejects. Now propagated.
- **`update_webhook` accepted webhook hashes of any length**, while
  `register_alert` required exactly 64 characters. Both now enforce the same rule.
- **`replace_watcher` could drop the replacement watcher.** Corrected so the new
  address is always added when it was not already registered.
- `configs_paginated` could overflow on `offset + limit`; now saturating.
- `AlertRegistry::transfer_admin` emitted no event, leaving a change of control
  invisible on-chain. It now emits `("admin", "transfer")`.

### Added

- **`get_alerts_by_owner_paginated` and `get_contract_alerts_paginated`** — paginated
  retrieval of alert configs by owner address and contract-indexed lookups using a shared
  `configs_paginated` helper (#39).
- **`test_remove_watcher_not_registered`** — test verifying that calling `remove_watcher`
  on a never-registered address completes without error and leaves the watcher list unchanged (#59).
- **Two-phase webhook rotation** — `propose_webhook` stages a new hash in the new
  `AlertConfig::pending_webhook_hash` field without disturbing the live webhook;
  `confirm_webhook` promotes it and clears the pending slot, returning the new
  `ContractError::NoPendingWebhook` when no rotation is in progress. This
  behaviour was already specified in `docs/alert-registry.md` and covered by
  tests, but had never been implemented. Emits `alert.wh_prop` / `alert.wh_conf`.
- **`renew_alert_ttl`** — owner-authenticated TTL extension that leaves
  `updated_at` untouched, so renewing storage does not make an alert reappear in
  `get_alerts_modified_since` incremental syncs.

### Removed

- `docs/pr-get-alerts-by-owner-paginated.md` and `docs/pr-remove-watcher-not-registered.md` —
  removed stray per-PR change summaries in favor of durable reference documentation (#106).

- `contracts/alert-registry/src/{contract,storage,types}.rs` — 857 lines of a
  second, divergent `AlertRegistry` implementation that was never declared as a
  module and therefore compiled into nothing.
- Root-level `task1.md`, `task2.md` and `issue.md` scratch notes (the first two
  described work already shipped; the third contained the word "test").
- **Feature A — `watcher.remove` event**: `WatcherRegistry::remove_watcher` now emits
  `(Symbol("watcher"), Symbol("remove"))` with `data = watcher: Address` **only when the
  watcher was actually present** (no-op removals are silent). Dependent systems such as
  `AlertRegistry` watcher-gating must subscribe to this event to revoke trust immediately.
  `clear_all_watchers` emits one event per removed watcher for the same reason.
- **Feature B — configurable TTL via `bump_alert`**: `AlertRegistry` now exposes
  `bump_alert(config_id, ttl)` which extends the TTL of an alert and its associated
  indexes up to `MAX_TTL` (535 680 ledgers ≈ 31 days). Values above the cap are silently
  clamped. No auth is required — any address (e.g. an off-chain keeper) may call it.
  Emits `(Symbol("alert"), Symbol("bump"))` with `data = (id: u64, effective_ttl: u32)`.
- `DEFAULT_TTL` constant (17 280 ledgers ≈ 24 hours) replaces the previous hardcoded
  100-ledger value across all `extend_ttl` calls in `alert-registry`.
- `MAX_TTL` constant (535 680 ledgers ≈ 31 days) as the protocol-enforced ceiling for
  caller-specified TTL values.
- `WatcherRegistry::is_authorized` alias for `is_watcher_authorized` (backwards compat).
- `WatcherRegistry::clear_all_watchers` bulk-deauthorizes all watchers in one admin call,
  emitting a `watcher.remove` event for each removed address.
- `WatcherRegistry::decrement_watcher_count` is now correctly called on `remove_watcher`
  (previously the count only ever incremented — this was a bug fix).
- `alert.bump` event documented in `docs/events.md`.
- `watcher.remove` event marked ✅ implemented in `docs/events.md`.
- `docs/ttl.md` updated to document `DEFAULT_TTL`, `MAX_TTL`, and `bump_alert`.

### Fixed
- `WatcherRegistry::remove_watcher` no longer emits an event when the watcher address
  was not registered (previously always emitted regardless).
- `WatcherRegistry::get_watcher_count` now decrements correctly on removal.
- `AlertRegistry::remove_alert` body was missing in `lib.rs` (structural corruption);
  restored with correct `remove_alert_record` call.
- `AlertRegistry::remove_alert_by_admin` was missing from `lib.rs`; restored.
- `AlertRegistry::register_alert` had duplicated validation calls; deduplicated.
- `AlertRegistry::update_alert` now keeps `DataKey::AlertActive` in sync when `active`
  changes.
- `contract.rs` `#[contract]` / `#[contractimpl]` attributes removed to prevent
  duplicate Soroban client generation conflicting with `lib.rs`.
- All `extend_ttl(_, 100, 100)` calls replaced with `extend_ttl(_, DEFAULT_TTL, DEFAULT_TTL)`.

- `get_watcher_count` function to WatcherRegistry for efficient watcher count queries (#21)
- TypeScript bindings for AlertRegistry published to npm as `@tx-wat/alert-registry-bindings` (#120)
- GitHub Actions workflow for automated npm publishing of TypeScript bindings
- `make bindings` target for local TypeScript binding generation
- Documentation for `get_watcher_count` in `docs/watcher-registry.md`
- Comprehensive README and usage examples for TypeScript bindings package
- `CHANGELOG.md` to track version history (#75)
- `SECURITY.md` with responsible disclosure policy (#76)
- `docs/ttl.md` documenting TTL values and their implications (#77)
- Inline rustdoc comments on all public and key private functions (#78)
- Expanded `.gitignore` to exclude build artifacts and test snapshots
- `bindings/watcher-registry` — TypeScript bindings package `@tx-wat/watcher-registry` generated via `stellar contract bindings typescript`
- `.github/workflows/publish-bindings.yml` — CI workflow that generates and publishes TypeScript bindings to npm on every GitHub release
- `docs/ecosystem-submission.md` — step-by-step guide for submitting to the Stellar Developer Tools ecosystem listing and the `stellar/soroban-examples` repository
- `contracts/watcher-registry/README.md` and `contracts/alert-registry/README.md` — per-contract READMEs required for the soroban-examples submission

### Documentation

- **`docs/watcher-registry.md` synced with the contract.** The stale single-admin
  `"ADMIN"` / `"WATCHERS"` storage table is replaced with the real `DataKey::Admins`
  (multi-admin `Vec<Address>`), `DataKey::Watchers`, and the previously undocumented
  `symbol_short!("W_CNT")` u32 counter. Added the missing `add_admin`, `remove_admin`,
  `replace_watcher`, `clear_all_watchers`, and `get_admins` function entries, each with
  its emitted-event topic/data shape, and corrected the `Result`-returning signatures
  that the doc still described as panicking (#63, #64, #68).
- **`docs/storage.md` WatcherRegistry keys** updated to `Admins` / `Watchers` / `W_CNT`
  to match, including the storage-tier summary row (#63).
- **`docs/events.md` intro banner** rewrote the stale "only `register_alert` and
  `remove_alert` emit; everything else is planned" note; the WatcherRegistry
  `admin.init` status and the `admin.add` / `admin.remove` / `watcher.replace` entries
  were already corrected in a prior fix. Audited every remaining status line against
  the contract source (#67).
- **Role policy documented.** `WatcherRegistry` now states in its rustdoc and in
  `docs/watcher-registry.md` that an address may hold both the admin and watcher roles,
  and that contract addresses are accepted for either role. Covered by
  `test_address_can_be_admin_and_watcher` and `test_contract_address_can_hold_roles` (#69).

## [0.1.0] - 2025-05-28

### Added
- `AlertRegistry` contract: register, update, remove, and query alert configs on-chain
- `WatcherRegistry` contract: manage authorized watcher node addresses with admin controls
- Persistent storage with TTL extension on every write
- Owner-keyed and contract-keyed index lookups for alert configs
- Stellar CLI, JavaScript SDK, and Rust SDK usage examples in `README.md`
- Deployment addresses tracked in `DEPLOYMENTS.md`
- Function reference docs in `docs/alert-registry.md` and `docs/watcher-registry.md`
- Contribution guidelines in `CONTRIBUTING.md`

[Unreleased]: https://github.com/Tx-wat/stellar-txwatch-contracts/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Tx-wat/stellar-txwatch-contracts/releases/tag/v0.1.0
