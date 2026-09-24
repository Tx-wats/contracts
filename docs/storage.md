# Storage Reference

This document describes every storage key used by both contracts, its value type, storage tier (instance vs persistent), and TTL behavior.

---

## AlertRegistry

Source: `contracts/alert-registry/src/lib.rs`

### Storage Keys

Generated from the `DataKey` enum and every `symbol_short!` key the contract reads or writes.

| Key | Tier | Value Type | Description |
|---|---|---|---|
| `DataKey::Alert(id: u64)` | Persistent | `AlertConfig` | A single alert configuration, keyed by its numeric ID |
| `DataKey::AlertActive(id: u64)` | Persistent | `bool` | The `active` flag stored separately so it can be read without deserializing the full `AlertConfig` (see `get_alert_active`) |
| `DataKey::OwnerIndex(addr: Address)` | Persistent | `Vec<u64>` | List of alert IDs owned by a given address |
| `DataKey::OwnerActiveCount(addr: Address)` | Persistent | `u32` | Running count of currently live (non-removed) alerts owned by `addr`, maintained incrementally alongside `OwnerIndex` so `get_non_removed_alert_count` is O(1) instead of rescanning the index. `get_active_alert_count` instead scans `OwnerIndex` and filters by the `AlertActive` flag, so deactivated-but-not-removed alerts are excluded |
| `DataKey::ContractIndex(addr: Address)` | Persistent | `Vec<u64>` | List of alert IDs watching a given contract address |
| `DataKey::NextId` | — | — | Declared in the enum but **not used**: the counter is stored under the `NEXT_ID` symbol key below |
| `symbol_short!("NEXT_ID")` | Instance | `u64` | Monotonic counter used to generate unique alert IDs; also the value returned by `get_alert_count` |
| `symbol_short!("ADMIN")` | Instance | `Address` | Admin address that may pause the contract, remove alerts and set limits |
| `symbol_short!("PAUSED")` | Instance | `bool` | Circuit-breaker flag set by `pause` / `unpause`; absent means not paused |
| `symbol_short!("LIMIT")` | Instance | `u32` | Optional per-owner active alert limit (`set_per_owner_alert_limit`) |
| `symbol_short!("CLIMIT")` | Instance | `u32` | Optional per-contract alert limit (`set_per_contract_alert_limit`) |
| `symbol_short!("GLIMIT")` | Instance | `u32` | Optional global ceiling on total alerts ever registered (`set_global_alert_limit`) |
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

All instance keys (`NEXT_ID`, `ADMIN`, `PAUSED`, `LIMIT`, `CLIMIT`, `GLIMIT`, `WATCHREG`) share the contract instance entry's TTL. **AlertRegistry currently never extends its instance TTL** — there is no `extend_ttl` on `instance()` anywhere in the contract, so these keys are *not* safe to ignore: if the instance entry is archived, the counter, admin and limits all become unreachable (and with them every alert) until the entry is restored. See #206, which tracks adding a `bump_instance_ttl` to AlertRegistry.

> See [docs/ttl.md](ttl.md) for implications of the DEFAULT_TTL setting and recommended production values.

---

## WatcherRegistry

Source: `contracts/watcher-registry/src/lib.rs`

### Storage Keys

| Key | Tier | Value Type | Description |
|---|---|---|---|
| `DataKey::Admins` | Instance | `Vec<Address>` | The current admin set (multi-admin; any one admin can perform privileged operations) |
| `DataKey::Watchers` | Instance | `Vec<Address>` | List of authorized watcher node addresses |
| `DataKey::PendingAdminTransfer` | Instance | `Address` | Address proposed by `propose_admin_transfer`, awaiting `accept_admin_transfer`; removed on accept or `cancel_admin_transfer` |
| `DataKey::TimelockDelay` | Instance | `u32` | Timelock delay in ledgers for sensitive admin actions; absent or `0` means disabled |
| `DataKey::PendingAction` | Instance | `PendingAction` | The single queued timelocked `AdminAction` with its proposer and `ready_at` ledger; removed on execute or `cancel_admin_action` |
| `DataKey::Paused` | Instance | `bool` | Circuit-breaker flag set by `pause` / `unpause`; absent means not paused |
| `symbol_short!("W_CNT")` | Instance | `u32` | Cached count of registered watchers, kept in sync by `register_watcher` / `remove_watcher` / `replace_watcher` / `clear_all_watchers` so `get_watcher_count` never deserializes the full `Watchers` vec |

### TTL Behavior

WatcherRegistry uses **instance storage exclusively**, so every key above shares the single instance entry's TTL. The contract extends it explicitly through `bump_instance_ttl`: a permissionless, auth-free call that, when the remaining TTL is below `INSTANCE_BUMP_THRESHOLD` (17,280 ledgers, ≈ 24 hours), extends it to `INSTANCE_BUMP_AMOUNT` (535,680 ledgers, ≈ 31 days). No other entrypoint extends the TTL, so low-traffic deployments should have a keeper call `bump_instance_ttl` periodically — see [Keeping the Watcher Registry Alive](ttl.md#keeping-the-watcher-registry-alive).

There are no persistent storage entries in WatcherRegistry.

---

## Storage Tier Summary

| Contract | Tier | Keys | TTL Managed By |
|---|---|---|---|
| AlertRegistry | Persistent | `Alert`, `AlertActive`, `OwnerIndex`, `OwnerActiveCount`, `ContractIndex` | Contract (`extend_ttl` to `DEFAULT_TTL` = 17,280 ledgers on write; `bump_alert` up to `MAX_TTL`) |
| AlertRegistry | Instance | `NEXT_ID`, `ADMIN`, `PAUSED`, `LIMIT`, `CLIMIT`, `GLIMIT`, `WATCHREG` | **Not extended** by the contract (see #206) |
| WatcherRegistry | Instance | `Admins`, `Watchers`, `PendingAdminTransfer`, `TimelockDelay`, `PendingAction`, `Paused`, `W_CNT` | Contract (`bump_instance_ttl`, extends to 535,680 ledgers) |
