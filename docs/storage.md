# Storage Reference

This document describes every storage key used by both contracts, its value type, storage tier (instance vs persistent), and TTL behavior.

---

## AlertRegistry

Source: `contracts/alert-registry/src/lib.rs`

### Storage Keys

| Key | Tier | Value Type | Description |
|---|---|---|---|
| `DataKey::Alert(id: u64)` | Persistent | `AlertConfig` | A single alert configuration, keyed by its numeric ID |
| `DataKey::AlertActive(id: u64)` | Persistent | `bool` | The `active` flag stored separately so it can be read without deserializing the full `AlertConfig` (see `get_alert_active`) |
| `DataKey::OwnerIndex(addr: Address)` | Persistent | `Vec<u64>` | List of alert IDs owned by a given address |
| `DataKey::OwnerActiveCount(addr: Address)` | Persistent | `u32` | Running count of currently live (non-removed) alerts owned by `addr`, maintained incrementally alongside `OwnerIndex` so `get_active_alert_count` is O(1) instead of rescanning the index |
| `DataKey::ContractIndex(addr: Address)` | Persistent | `Vec<u64>` | List of alert IDs watching a given contract address |
| `symbol_short!("NEXT_ID")` | Instance | `u64` | Monotonic counter used to generate unique alert IDs |
| `symbol_short!("ADMIN")` | Instance | `Address` | Optional admin address that may remove alerts and set owner limits |
| `symbol_short!("LIMIT")` | Instance | `u32` | Optional per-owner active alert limit |
| `symbol_short!("WATCHREG")` | Instance | `Address` | Optional `WatcherRegistry` contract address; when set, read queries are gated to registered watchers |

### AlertConfig Fields

| Field | Type | Description |
|---|---|---|
| `label` | `String` | Human-readable name for the alert (max 128 bytes) |
| `webhook_hash` | `String` | SHA-256 hex digest of the webhook URL |
| `rules` | `Vec<String>` | Rule descriptor strings (e.g. `"rule:transfer"`) |
| `owner` | `Address` | Address that owns and may mutate this alert |
| `target_contract` | `Address` | Contract address being watched |
| `created_at` | `u64` | Ledger timestamp at registration |
| `updated_at` | `u64` | Ledger timestamp of the most recent update |
| `active` | `bool` | Whether the alert is currently active |
| `pending_webhook_hash` | `Option<String>` | Pending webhook hash proposed via `propose_webhook`, not yet confirmed. `None` when no rotation is in progress. |

### TTL Behavior

All persistent key variants (`Alert`, `AlertActive`, `OwnerIndex`, `OwnerActiveCount`, `ContractIndex`) are extended by `DEFAULT_TTL` (**17,280 ledgers**, ≈ 24 hours at 5 s/ledger) on every write that touches them. `bump_alert` can extend an alert's TTL further, up to `MAX_TTL` (535,680 ledgers, ≈ 31 days).

| Function | Keys Extended |
|---|---|
| `register_alert` | `Alert(id)`, `AlertActive(id)`, `OwnerIndex(owner)`, `OwnerActiveCount(owner)`, `ContractIndex(target)` |
| `update_alert` | `Alert(id)`, `AlertActive(id)` |
| `update_webhook` | `Alert(id)` |
| `propose_webhook` | `Alert(id)`, `OwnerIndex(owner)`, `ContractIndex(target)` |
| `confirm_webhook` | `Alert(id)`, `OwnerIndex(owner)`, `ContractIndex(target)` |
| `renew_alert_ttl` | `Alert(id)`, `OwnerIndex(owner)`, `ContractIndex(target)` — data unchanged |
| `deactivate_all_alerts` | `Alert(id)`, `AlertActive(id)` for each deactivated alert; `ContractIndex(target)` for each touched contract; `OwnerIndex(caller)` once, if at least one alert was deactivated |
| `remove_alert` | `Alert(id)`, `AlertActive(id)` deleted; `OwnerIndex(owner)`, `OwnerActiveCount(owner)`, `ContractIndex(target)` updated and TTL-extended |

Read-only functions (`get_alert`, `get_alerts_for_contract`, `get_alerts_by_owner`, paginated variants, `get_alert_count`) do **not** extend any TTL.

The `NEXT_ID` instance key has no explicit TTL management — its lifetime is tied to the contract instance itself.

> See [docs/ttl.md](ttl.md) for implications of the DEFAULT_TTL setting and recommended production values.

---

## WatcherRegistry

Source: `contracts/watcher-registry/src/lib.rs`

### Storage Keys

| Key | Tier | Value Type | Description |
|---|---|---|---|
| `DataKey::Admins` | Instance | `Vec<Address>` | The current admin set (multi-admin; any one admin can perform privileged operations) |
| `DataKey::Watchers` | Instance | `Vec<Address>` | List of authorized watcher node addresses |
| `symbol_short!("W_CNT")` | Instance | `u32` | Cached count of registered watchers, kept in sync by `register_watcher` / `remove_watcher` / `replace_watcher` / `clear_all_watchers` so `get_watcher_count` never deserializes the full `Watchers` vec |

### TTL Behavior

WatcherRegistry uses **instance storage exclusively**. Instance storage TTL is managed by the Stellar network and is not explicitly extended by any function in this contract. The TTL resets whenever the contract instance is accessed by any transaction that bumps the footprint.

There are no persistent storage entries in WatcherRegistry.

---

## Storage Tier Summary

| Contract | Tier | Keys | TTL Managed By |
|---|---|---|---|
| AlertRegistry | Persistent | `Alert`, `OwnerIndex`, `OwnerActiveCount`, `ContractIndex` | Contract (`extend_ttl`, DEFAULT_TTL = 17,280 ledgers) |
| AlertRegistry | Instance | `NEXT_ID` | Network |
| WatcherRegistry | Instance | `Admins`, `Watchers`, `W_CNT` | Network |
