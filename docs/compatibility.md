# Compatibility Matrix

This document defines the compatibility matrix between `stellar-txwatch-contracts`, published TypeScript bindings, and the sister repositories (`stellar-txwatch-core`, `stellar-txwatch-web`).

---

## Sister Repositories

| Repository | Purpose | URL |
|---|---|---|
| **`stellar-txwatch-contracts`** (this repo) | Soroban smart contracts (`AlertRegistry`, `WatcherRegistry`) & TypeScript bindings | https://github.com/Tx-wat/stellar-txwatch-contracts |
| **`stellar-txwatch-core`** | Backend watcher daemon polling Horizon, evaluating rules, and dispatching webhooks | https://github.com/Tx-wat/stellar-txwatch-core |
| **`stellar-txwatch-web`** | Web dashboard and frontend interface for managing alerts and watcher nodes | https://github.com/Tx-wat/stellar-txwatch-web |

---

## Compatibility Table

| Contract Tag / Version | Soroban SDK / Protocol | npm Bindings (`@tx-wat/watcher-registry`) | Core Engine (`stellar-txwatch-core`) | Web Dashboard (`stellar-txwatch-web`) | Compatibility Status & Notes |
|---|---|---|---|---|---|
| **`v0.1.0`** | Soroban SDK 22.0.0 (Protocol 22) | `^0.1.0` | `^0.1.0` | `^0.1.0` | **Stable**. Initial release supporting basic alert and watcher registration, owner/contract index lookups, and TTL management. |
| **`v0.2.0`** *(main / unreleased)* | Soroban SDK 22.0.0 (Protocol 22) | `^0.2.0` | `^0.2.0` | `^0.2.0` | **Active Development**. Adds `is_watcher_gating_enabled` convenience getter, on-chain `contractmeta!` discoverability, two-phase webhook rotation, and bulk deauthorization. |

---

## Interface & Compatibility Policy

1. **Storage Compatibility:**
   - Contract storage key schemas (`DataKey`) and struct encodings (`AlertConfig`) must remain backward-compatible across minor versions.
   - Any breaking storage schema change requires a migration path or a major version bump (`v1.0.0`).

2. **Function Signatures:**
   - New getters and convenience functions (such as `is_watcher_gating_enabled`) are additive and backward-compatible.
   - Function parameter types or return types must not be modified in minor releases without backwards-compatible aliases (e.g. `is_authorized` alias for `is_watcher_authorized`).

3. **Bindings & Types:**
   - When contract interfaces change, TypeScript bindings in `bindings/watcher-registry` and `bindings/alert-registry` must be re-generated via `stellar contract bindings typescript` and published in lockstep.

4. **Event Compatibility:**
   - Soroban event topics and data shapes (such as `("watcher", "remove")` and `("alert", "bump")`) are consumed by `stellar-txwatch-core`. Any change to topic symbols or payload tuples constitutes a breaking change for core indexers.

---

## Release Checklist (Issue #98)

When preparing a new release tag:

1. [ ] **Build & Test:** Ensure all workspace tests and clippy checks pass:
   ```bash
   cargo test --workspace --locked
   cargo clippy --workspace --all-targets --locked -- -D warnings
   cargo fmt --check
   ```
2. [ ] **WASM Verification:** Build release WASM and inspect metadata:
   ```bash
   cargo build --release --target wasm32-unknown-unknown --locked -p alert-registry -p watcher-registry
   stellar contract inspect --wasm target/wasm32-unknown-unknown/release/alert_registry.wasm
   stellar contract inspect --wasm target/wasm32-unknown-unknown/release/watcher_registry.wasm
   ```
3. [ ] **Sister Repo Integration:** Run integration and end-to-end tests in `stellar-txwatch-core` and `stellar-txwatch-web` against the newly built WASM contracts.
4. [ ] **Update Compatibility Matrix:** Add or update the row for the target release tag in both `docs/compatibility.md` and `README.md`.
5. [ ] **Update Changelog:** Update `CHANGELOG.md` with version notes and link definitions.
6. [ ] **Tag & Release:** Push the release tag (e.g. `v0.1.0`) to trigger GitHub Actions publishing for ABIs and npm bindings.
