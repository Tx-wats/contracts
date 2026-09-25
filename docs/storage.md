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
| `DataKey::OwnerLiveCount(addr: Address)` | Persistent | `u32` | Running count of currently live (non-removed) alerts owned by `addr`, deactivated alerts included. Renamed from `OwnerActiveCount` (the name wrongly implied an `active` filter); entries under the legacy key are migrated to the new key on first read. Maintained incrementally alongside `OwnerIndex` so `get_non_removed_alert_count` is O(1) instead of rescanning the index. `get_active_alert_count` instead scans `OwnerIndex` and filters by the `AlertActive` flag, so deactivated-but-not-removed alerts are excluded |
| `DataKey::PendingTransfer(id: u64)` | Persistent | `PendingAlertTransfer` | Ownership transfer proposed by `propose_alert_transfer` and awaiting `accept_alert_transfer`: the recipient (`new_owner`) and the last ledger it can be accepted on (`expires_at_ledger`). Removed on accept, reject, cancel, alert removal, or retarget. Written with a TTL of `ALERT_TRANSFER_EXPIRY_LEDGERS` |
| `DataKey::AdminSuspended(id: u64)` | Persistent | `bool` | Present while an admin has suspended the alert with `deactivate_alert_by_admin`; blocks owner reactivation until `unlock_alert_by_admin`. Extended with the alert's other entries by `touch_alert`; removed with the alert |
| `DataKey::ContractIndex(addr: Address)` | Persistent | `Vec<u64>` | List of alert IDs watching a given contract address |
| `instance_key::NEXT_ID` (`symbol_short!("NEXT_ID")`) | Instance | `u64` | Monotonic counter used to generate unique alert IDs; also the value returned by `get_alert_count` |
| `instance_key::ADMIN` (`symbol_short!("ADMIN")`) | Instance | `Address` | Admin address that may pause the contract, remove alerts and set limits |
| `instance_key::PAUSED` (`symbol_short!("PAUSED")`) | Instance | `bool` | Circuit-breaker flag set by `pause` / `unpause`; absent means not paused |
| `instance_key::LIMIT` (`symbol_short!("LIMIT")`) | Instance | `u32` | Optional per-owner active alert limit (`set_per_owner_alert_limit`) |
| `instance_key::CLIMIT` (`symbol_short!("CLIMIT")`) | Instance | `u32` | Optional per-contract alert limit (`set_per_contract_alert_limit`) |
| `instance_key::GLIMIT` (`symbol_short!("GLIMIT")`) | Instance | `u32` | Optional global ceiling on currently live alerts (`set_global_alert_limit`) |
| `instance_key::LIVE` (`symbol_short!("LIVE")`) | Instance | `u64` | Number of currently live alert records used by the global ceiling |
| `instance_key::WATCHREG` (`symbol_short!("WATCHREG")`) | Instance | `Address` | Optional `WatcherRegistry` contract address; when set, read queries are gated to registered watchers |

Instance keys are the constants in `alert_registry::instance_key`. Each is the same bare `symbol_short!` the contract has always used, so the storage encoding is unchanged across the switch (#211); the constants only make a mistyped key a compile error. They are intentionally not `DataKey` variants, whose encoding (`[Symbol("Name"), …]`) would differ from the stored keys. The former `DataKey::NextId` variant was never used for storage and has been removed.

### AlertConfig Fields

| Field | Type | Description |
|---|---|---|
| `label` | `String` | Human-readable name for the alert (max 128 bytes) |
| `webhook_hash` | `BytesN<32>` | SHA-256 digest of the webhook URL, as 32 raw bytes |
| `rules` | `Vec<String>` | Rule descriptor strings (e.g. `"rule:transfer"`) |
| `owner` | `Address` | Address that owns and may mutate this alert |
| `target_contract` | `Address` | Contract address being watched |
| `created_at` | `u64` | Ledger timestamp at registration |
| `updated_at` | `u64` | Ledger timestamp of the most recent update |
| `updated_ledger` | `u32` | Monotonic ledger sequence number of the most recent update |
| `active` | `bool` | Whether the alert is currently active |
| `pending_webhook_hash` | `Option<BytesN<32>>` | Pending webhook hash proposed via `propose_webhook`, not yet confirmed. `None` when no rotation is in progress. |

### TTL Behavior

All persistent key variants (`Alert`, `AlertActive`, `OwnerIndex`, `OwnerLiveCount`, `ContractIndex`) are extended by `DEFAULT_TTL` (**17,280 ledgers**, ≈ 24 hours at 5 s/ledger) on every write that touches them. `bump_alert` can extend an alert's TTL further, up to `MAX_TTL` (535,680 ledgers, ≈ 31 days).

Every mutator that rewrites an alert goes through `persist_alert`, which writes `Alert(id)` and `AlertActive(id)` and then calls `touch_alert` to extend the alert's full set of entries: `Alert(id)`, `AlertActive(id)`, `OwnerIndex(owner)`, `OwnerLiveCount(owner)`, `ContractIndex(target)`. `renew_alert_ttl` and `bump_alert` call `touch_alert` directly without rewriting data. `OwnerLiveCount(owner)` is only extended when present (it may not yet be migrated from the legacy `OwnerActiveCount` key, or may have expired). See [docs/ttl.md](ttl.md) for the per-function table.

| Function | Keys Extended |
|---|---|
| `register_alert`, `update_alert`, `update_webhook`, `update_label`, `propose_webhook`, `confirm_webhook`, `cancel_webhook_proposal`, `deactivate_alert_by_admin`, `renew_alert_ttl` | Full set, to `DEFAULT_TTL` |
| `accept_alert_transfer` | Full set with the new owner's `OwnerIndex`/`OwnerLiveCount`; the old owner's index and counter are rewritten and extended; `PendingTransfer(id)` removed |
| `propose_alert_transfer` | Writes `PendingTransfer(id)` with a TTL of `ALERT_TRANSFER_EXPIRY_LEDGERS`; the alert's own entries are unchanged |
| `update_target_contract` | Full set with the new target's `ContractIndex`; the old target's index is rewritten and extended |
| `deactivate_all_alerts` | Full set for each deactivated alert |
| `bump_alert` | Full set, to the requested TTL (capped at `MAX_TTL`) |
| `remove_alert` | `Alert(id)`, `AlertActive(id)` deleted; `OwnerIndex(owner)`, `OwnerLiveCount(owner)`, `ContractIndex(target)` updated and TTL-extended |

Read-only functions (`get_alert`, `get_alerts_for_contract`, `get_alerts_by_owner`, paginated variants, `get_alert_count`) do **not** extend any TTL.

All instance keys (`NEXT_ID`, `LIVE`, `ADMIN`, `PAUSED`, `LIMIT`, `CLIMIT`, `GLIMIT`, `WATCHREG`) share the contract instance entry's TTL. The contract extends it to `INSTANCE_BUMP_AMOUNT` (535,680 ledgers, ≈ 31 days) whenever it has dropped below `INSTANCE_BUMP_THRESHOLD` (17,280 ledgers) on every write to instance storage: the ID counter and live count in `register_alert`, and every admin setter. The permissionless `bump_instance_ttl` does the same for quiet periods; if the instance entry is archived, the counter, live count, admin and limits (and with them every alert) are unreachable until it is restored. See [docs/ttl.md](ttl.md#keeping-the-alert-registry-instance-alive).

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
| AlertRegistry | Persistent | `Alert`, `AlertActive`, `OwnerIndex`, `OwnerLiveCount`, `ContractIndex` | Contract (`extend_ttl` to `DEFAULT_TTL` = 17,280 ledgers on write; `bump_alert` up to `MAX_TTL`) |
| AlertRegistry | Instance | `NEXT_ID`, `LIVE`, `ADMIN`, `PAUSED`, `LIMIT`, `CLIMIT`, `GLIMIT`, `WATCHREG` | Contract (extended on every instance write and by `bump_instance_ttl`, to 535,680 ledgers) |
| WatcherRegistry | Instance | `Admins`, `Watchers`, `PendingAdminTransfer`, `TimelockDelay`, `PendingAction`, `Paused`, `W_CNT` | Contract (`bump_instance_ttl`, extends to 535,680 ledgers) |
