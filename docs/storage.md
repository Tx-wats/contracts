# Storage Reference

This document describes every storage key used by both contracts, its value type, storage tier (instance vs persistent), and TTL behavior.

---

## AlertRegistry

Source: `contracts/alert-registry/src/lib.rs`

### Storage Keys

Every `DataKey` variant and `instance_key` constant the contract reads or writes is listed here. CI runs `scripts/check_storage_doc.py`, which fails if a `DataKey` variant is missing from this table or from the Storage Tier Summary.

| Key | Tier | Value Type | Description |
|---|---|---|---|
| `DataKey::Alert(id: u64)` | Persistent | `AlertConfig` | A single alert configuration, keyed by its numeric ID |
| `DataKey::RemovedAlert(id: u64)` | Persistent | `AlertConfig` | Removal tombstone: the last config of a removed alert, with `active == false`, so incremental sync can deliver it. Written once with `TOMBSTONE_TTL` and not extended afterwards (see [docs/ttl.md](ttl.md#other-persistent-entries)) |
| `DataKey::AlertActive(id: u64)` | Persistent | `bool` | The `active` flag stored separately so it can be read without deserializing the full `AlertConfig` (see `get_alert_active`) |
| `DataKey::OwnerIndex(addr: Address)` | Persistent | `Vec<u64>` | List of alert IDs owned by a given address |
| `DataKey::OwnerLiveCount(addr: Address)` | Persistent | `u32` | Running count of currently live (non-removed) alerts owned by `addr`, deactivated alerts included. Renamed from `OwnerActiveCount` (the name wrongly implied an `active` filter); entries under the legacy key are migrated to the new key on first read. Maintained incrementally alongside `OwnerIndex` so `get_non_removed_alert_count` is O(1) instead of rescanning the index. `get_active_alert_count` instead scans `OwnerIndex` and filters by the `AlertActive` flag, so deactivated-but-not-removed alerts are excluded |
| `DataKey::PendingTransfer(id: u64)` | Persistent | `PendingAlertTransfer` | Ownership transfer proposed by `propose_alert_transfer` and awaiting `accept_alert_transfer`: the recipient (`new_owner`) and the last ledger it can be accepted on (`expires_at_ledger`). Removed on accept, reject, cancel, alert removal, or retarget. Written with a TTL of `ALERT_TRANSFER_EXPIRY_LEDGERS` |
| `DataKey::AdminSuspended(id: u64)` | Persistent | `bool` | Present while an admin has suspended the alert with `deactivate_alert_by_admin`; blocks owner reactivation until `unlock_alert_by_admin`. Extended with the alert's other entries by `touch_alert`; removed with the alert |
| `DataKey::ContractIndex(addr: Address)` | Persistent | `Vec<u64>` | List of alert IDs watching a given contract address |
| `DataKey::AlertTarget(id: u64)` | Persistent | `Address` | Target contract of the alert, kept beside the ID so pruning an expired alert can fix `ContractIndex` and `ContractLiveCount`. Written by `persist_alert`, extended by `touch_alert`, deleted on removal and by pruning |
| `DataKey::ContractLiveCount(addr: Address)` | Persistent | `u32` | Running count of currently live (non-removed) alerts targeting `addr`, the per-contract analogue of `OwnerLiveCount`. Incremented by `push_contract_index` on registration and on retarget into `addr`, decremented by `remove_from_contract_index` on removal and on retarget out of `addr`, and lazily rebuilt from `ContractIndex` by `contract_live_count` for contracts written before the counter existed. Internal only: no public getter reads it (`get_active_contract_alert_count` scans `ContractIndex`) |
| `DataKey::DeactivationCursor(addr: Address)` | Persistent | `u32` | Position in `OwnerIndex` where a bulk `deactivate_all_alerts` call for `addr` stopped, so a large owner can be deactivated across several calls. Written by `deactivate_all_alerts` only while entries remain (with `DEFAULT_TTL`) and removed once the scan completes or when `remove_from_owner_index` runs |
| `DataKey::RemovedAlert(id: u64)` | Persistent | `AlertConfig` | Tombstone holding the last config of a removed alert (`active = false`, `removed = true`, `pending_webhook_hash` cleared), so incremental sync can deliver an inactive record. Written once by `remove_alert` with `TOMBSTONE_TTL`; never rewritten, and not extended by `touch_alert` or `bump_alert` |
| `DataKey::ContractLiveCount(addr: Address)` | Persistent | `u32` | Running count of live (non-removed) alerts targeting `addr`, maintained alongside `ContractIndex`. Rebuilt from `ContractIndex` if the entry is missing or expired |
| `DataKey::DeactivationCursor(addr: Address)` | Persistent | `u32` | Next owner-index position to inspect for a resumable `deactivate_all_alerts` sweep. Present only while a sweep is unfinished; removed on completion or when the owner's index changes |
| `DataKey::AlertExpiry(id: u64)` | Persistent | `Option<u32>` | Optional ledger sequence at which the alert stops being active, set by the owner. Stored under a separate key so `AlertConfig` is not re-encoded. Past-expiry alerts are treated as inactive by the active getters and sync results. Extended by `touch_alert` and removed with the alert |
| `instance_key::NEXT_ID` (`symbol_short!("NEXT_ID")`) | Instance | `u64` | Monotonic counter used to generate unique alert IDs; also the value returned by `get_alert_count` |
| `instance_key::ADMIN` (`symbol_short!("ADMIN")`) | Instance | `Address` | Admin address that may pause the contract, remove alerts and set limits |
| `DataKey::Guardian` | Instance | `Option<Address>` | Optional guardian address set by `set_guardian` (admin only) and read by `get_guardian`. The guardian may call `pause` but never `unpause`; `None` when unset. Shares the instance entry's TTL |
| `instance_key::PAUSED` (`symbol_short!("PAUSED")`) | Instance | `bool` | Circuit-breaker flag set by `pause` / `unpause`; absent means not paused |
| `instance_key::LIMIT` (`symbol_short!("LIMIT")`) | Instance | `u32` | Optional per-owner active alert limit (`set_per_owner_alert_limit`) |
| `instance_key::CLIMIT` (`symbol_short!("CLIMIT")`) | Instance | `u32` | Optional per-contract alert limit (`set_per_contract_alert_limit`) |
| `instance_key::GLIMIT` (`symbol_short!("GLIMIT")`) | Instance | `u32` | Optional global ceiling on currently live alerts (`set_global_alert_limit`) |
| `instance_key::LIVE` (`symbol_short!("LIVE")`) | Instance | `u64` | Number of currently live alert records used by the global ceiling |
| `instance_key::WATCHREG` (`symbol_short!("WATCHREG")`) | Instance | `Address` | Optional `WatcherRegistry` contract address; when set, read queries are gated to registered watchers |
| `DataKey::PendingAdminTransfer` | Instance | `(Address, u32)` (new admin, expiry ledger) | Address proposed by `propose_admin_transfer`, awaiting `accept_admin_transfer`. Written by `propose_admin_transfer` (a new proposal overwrites any earlier one); removed by `accept_admin_transfer` and `cancel_admin_transfer`. Shares the instance entry's TTL |
| `instance_key::GUARDIAN` (`symbol_short!("GUARDIAN")`) | Instance | `Option<Address>` | Optional guardian address, the instance-storage backing of `set_guardian` / `get_guardian`. The guardian may call `pause` but never `unpause`; absent when unset. Shares the instance entry's TTL |
| `instance_key::SCHEMAV` (`symbol_short!("SCHEMAV")`) | Instance | `u32` | Schema version stamped by `migrate` / read by `get_schema_version`, used to gate storage migrations. Shares the instance entry's TTL |
| `DataKey::WebhookProposalLedger(id: u64)` | Persistent | `u32` | Ledger sequence at which the pending webhook proposal for alert `id` was made, used to expire stale proposals. Written by `propose_webhook` with `DEFAULT_TTL`; removed by `confirm_webhook`, `cancel_webhook_proposal`, `remove_alert` and pruning |
| `DataKey::AlertTags(id: u64)` | Persistent | `Vec<String>` | Owner-supplied tags for alert `id` (each tag capped in length, vector capped in count). Written by `set_alert_tags` with `DEFAULT_TTL`, extended by `touch_alert`, removed with the alert |
| `DataKey::SnoozeUntil(id: u64)` | Persistent | `u32` | Ledger sequence until which alert `id` is snoozed and suppressed from notifications. Written by `snooze_alert` with `DEFAULT_TTL`, cleared by `unsnooze_alert`, extended by `touch_alert`, removed with the alert |
| `DataKey::PruneCursor` | Persistent | `u64` | Position of the resumable expired-alert pruning sweep, so `prune_expired_alerts` can continue across calls. Written with `DEFAULT_TTL` while a sweep is unfinished and removed when it completes |
| `DataKey::OwnerActiveCount(addr: Address)` | Persistent | `u32` | Legacy per-owner live-alert counter, superseded by `OwnerLiveCount`. Read only to migrate an existing entry to the new key on first access; never written by new code |

Instance keys are the constants in `alert_registry::instance_key`. Each is the same bare `symbol_short!` the contract has always used, so the storage encoding is unchanged across the switch (#211); the constants only make a mistyped key a compile error. They are intentionally not `DataKey` variants, whose encoding (`[Symbol("Name"), …]`) would differ from the stored keys. The `DataKey::NextId` variant is still declared in the enum but is never read or written; the ID counter lives only under `instance_key::NEXT_ID`. Do not use `DataKey::NextId` for new code.

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

All persistent key variants (`Alert`, `AlertActive`, `OwnerIndex`, `OwnerLiveCount`, `ContractIndex`, `ContractLiveCount`) are extended by `DEFAULT_TTL` (**17,280 ledgers**, ≈ 24 hours at 5 s/ledger) on every write that touches them. `DeactivationCursor` and `RemovedAlert` are also written with `DEFAULT_TTL`, but nothing else extends them. `bump_alert` can extend an alert's TTL further, up to `MAX_TTL` (535,680 ledgers, ≈ 31 days).

Every mutator that rewrites an alert goes through `persist_alert`, which writes `Alert(id)` and `AlertActive(id)` and then calls `touch_alert` to extend the alert's full set of entries: `Alert(id)`, `AlertActive(id)`, `OwnerIndex(owner)`, `OwnerLiveCount(owner)`, `ContractIndex(target)`, and `AlertExpiry(id)` when present. `renew_alert_ttl` and `bump_alert` call `touch_alert` directly without rewriting data. `OwnerLiveCount(owner)` is only extended when present (it may not yet be migrated from the legacy `OwnerActiveCount` key, or may have expired). `AlertExpiry(id)` is likewise only extended when present, and is deleted by `remove_alert` along with the alert's other entries. See [docs/ttl.md](ttl.md) for the per-function table.

| Function | Keys Extended |
|---|---|
| `register_alert`, `update_alert`, `update_webhook`, `update_label`, `propose_webhook`, `confirm_webhook`, `cancel_webhook_proposal`, `deactivate_alert_by_admin`, `renew_alert_ttl` | Full set, to `DEFAULT_TTL` |
| `accept_alert_transfer` | Full set with the new owner's `OwnerIndex`/`OwnerLiveCount`; the old owner's index and counter are rewritten and extended; `PendingTransfer(id)` removed |
| `propose_alert_transfer` | Writes `PendingTransfer(id)` with a TTL of `ALERT_TRANSFER_EXPIRY_LEDGERS`; the alert's own entries are unchanged |
| `update_target_contract` | Full set with the new target's `ContractIndex`; the old target's index is rewritten and extended |
| `deactivate_all_alerts` | Full set for each deactivated alert |
| `bump_alert` | Full set, to the requested TTL (capped at `MAX_TTL`) |
| `remove_alert` | `Alert(id)`, `AlertActive(id)`, `AlertExpiry(id)` deleted; `OwnerIndex(owner)`, `OwnerLiveCount(owner)`, `ContractIndex(target)` updated and TTL-extended |

Read-only functions (`get_alert`, `get_alerts_for_contract`, `get_alerts_by_owner`, paginated variants, `get_alert_count`) do **not** extend any TTL.

All instance keys (`NEXT_ID`, `LIVE`, `ADMIN`, `PAUSED`, `LIMIT`, `CLIMIT`, `GLIMIT`, `WATCHREG`, `PendingAdminTransfer`, `Guardian`, `GUARDIAN`, `SCHEMAV`) share the contract instance entry's TTL. The contract extends it to `INSTANCE_BUMP_AMOUNT` (535,680 ledgers, ≈ 31 days) whenever it has dropped below `INSTANCE_BUMP_THRESHOLD` (17,280 ledgers) on every write to instance storage: the ID counter and live count in `register_alert`, and every admin setter. The permissionless `bump_instance_ttl` does the same for quiet periods; if the instance entry is archived, the counter, live count, admin and limits (and with them every alert) are unreachable until it is restored. See [docs/ttl.md](ttl.md#keeping-the-alert-registry-instance-alive).

> See [docs/ttl.md](ttl.md) for implications of the DEFAULT_TTL setting and recommended production values.

---

## WatcherRegistry

Source: `contracts/watcher-registry/src/lib.rs`

### Storage Keys

| Key | Tier | Value Type | Description |
|---|---|---|---|
| `DataKey::Admins` | Instance | `Vec<Address>` | The current admin set (multi-admin; any one admin can perform privileged operations) |
| `DataKey::Watchers` | Instance | `Vec<Address>` | List of authorized watcher node addresses |
| `DataKey::PendingAdminTransfer` | Instance | `(Address, u32)` (new admin, expiry ledger) | Address proposed by `propose_admin_transfer`, awaiting `accept_admin_transfer`; removed on accept or `cancel_admin_transfer` |
| `DataKey::TimelockDelay` | Instance | `u32` | Timelock delay in ledgers for sensitive admin actions; absent or `0` means disabled |
| `DataKey::PendingAction` | Instance | `PendingAction` | The single queued timelocked `AdminAction` with its proposer and `ready_at` ledger; removed on execute or `cancel_admin_action` |
| `DataKey::Paused` | Instance | `bool` | Circuit-breaker flag set by `pause` / `unpause`; absent means not paused. The guardian may call `pause` but never `unpause` |
| `DataKey::Guardian` | Instance | `Option<Address>` | Optional guardian address set by `set_guardian` (admin only) and read by `get_guardian`. The guardian is a low-privilege on-call role that may call `pause` but never `unpause`; `None` when unset |
| `DataKey::WatcherInfo(addr: Address)` | Persistent | `WatcherInfo` | Per-watcher metadata (registration ledger, label, etc.). Deleted by `remove_watcher`, `clear_watchers_batch` and `clear_all_watchers`, so a re-registered address starts with fresh metadata |
| `DataKey::WatcherScope(addr: Address)` | Persistent | `WatcherScope` | Per-watcher scope restricting which contracts the watcher may observe. Deleted alongside `WatcherInfo(addr)` on every removal path, so a re-registered address starts unscoped |
| `symbol_short!("W_CNT")` | Instance | `u32` | Cached count of registered watchers, kept in sync by `register_watcher` / `remove_watcher` / `replace_watcher` / `clear_all_watchers` so `get_watcher_count` never deserializes the full `Watchers` vec |
| `DataKey::WatcherInfo(addr: Address)` | Persistent | `WatcherInfo` | Per-watcher metadata (label and operator strings, each capped in length) written by `set_watcher_info`. Only registered watchers are accepted, the setter is pause-gated, and the entry is written with `DEFAULT_TTL` and re-extended on every update |
| `DataKey::WatcherScope(addr: Address)` | Persistent | `Vec<Address>` | Per-watcher scope list (capped in length) written by `set_watcher_scope`. Only registered watchers are accepted, the setter is pause-gated, and the entry is written with `DEFAULT_TTL` and re-extended on every update |

`clear_all_watchers` (and the timelocked `AdminAction::ClearAllWatchers`) clears every watcher except a caller-supplied kept set, so it can succeed while `MIN_WATCHERS` watchers remain registered. Each cleared watcher's `WatcherInfo(addr)` and `WatcherScope(addr)` entries are deleted along with its entry in `Watchers`.
=======

### TTL Behavior

All instance keys above share the single instance entry's TTL. The contract extends it explicitly through `bump_instance_ttl`: a permissionless, auth-free call that, when the remaining TTL is below `INSTANCE_BUMP_THRESHOLD` (17,280 ledgers, ≈ 24 hours), extends it to `INSTANCE_BUMP_AMOUNT` (535,680 ledgers, ≈ 31 days). No other entrypoint extends the TTL, so low-traffic deployments should have a keeper call `bump_instance_ttl` periodically — see [Keeping the Watcher Registry Alive](ttl.md#keeping-the-watcher-registry-alive).

`WatcherInfo(addr)` and `WatcherScope(addr)` are persistent entries written by `register_watcher` with `DEFAULT_TTL` (17,280 ledgers) and extended on update. They are deleted — not merely left to expire — by `remove_watcher`, `clear_watchers_batch` and `clear_all_watchers`, so a removed address that registers again starts with no metadata and no scope.
Most WatcherRegistry keys live in **instance storage**, so they share the single instance entry's TTL. The contract extends it explicitly through `bump_instance_ttl`: a permissionless, auth-free call that, when the remaining TTL is below `INSTANCE_BUMP_THRESHOLD` (17,280 ledgers, ≈ 24 hours), extends it to `INSTANCE_BUMP_AMOUNT` (535,680 ledgers, ≈ 31 days). No other entrypoint extends the instance TTL, so low-traffic deployments should have a keeper call `bump_instance_ttl` periodically — see [Keeping the Watcher Registry Alive](ttl.md#keeping-the-watcher-registry-alive).

`WatcherInfo(addr)` and `WatcherScope(addr)` are **persistent** entries, written with `DEFAULT_TTL` (17,280 ledgers, ≈ 24 hours) by `set_watcher_info` / `set_watcher_scope` and re-extended on each subsequent write. They are removed by `remove_watcher` and `clear_all_watchers` along with the watcher itself.

---

## Storage Tier Summary

| Contract | Tier | Keys | TTL Managed By |
|---|---|---|---|
| AlertRegistry | Persistent | `Alert`, `AlertActive`, `AdminSuspended`, `PendingTransfer`, `OwnerIndex`, `OwnerLiveCount`, `ContractIndex`, `ContractLiveCount`, `DeactivationCursor`, `RemovedAlert`, `AlertExpiry` | Contract (`extend_ttl` to `DEFAULT_TTL` = 17,280 ledgers on write; `bump_alert` up to `MAX_TTL`) |
| AlertRegistry | Instance | `NEXT_ID`, `LIVE`, `ADMIN`, `PAUSED`, `LIMIT`, `CLIMIT`, `GLIMIT`, `WATCHREG`, `PendingAdminTransfer`, `Guardian` | Contract (extended on every instance write and by `bump_instance_ttl`, to 535,680 ledgers) |
| AlertRegistry | Persistent | `Alert`, `RemovedAlert`, `AlertActive`, `AdminSuspended`, `OwnerIndex`, `OwnerLiveCount`, `ContractIndex`, `ContractLiveCount`, `DeactivationCursor`, `PendingTransfer`, `AlertExpiry` | Contract (`extend_ttl` to `DEFAULT_TTL` = 17,280 ledgers on write, except `PendingTransfer` at `ALERT_TRANSFER_EXPIRY_LEDGERS`; `bump_alert` up to `MAX_TTL`) |
| AlertRegistry | Instance | `NEXT_ID`, `LIVE`, `ADMIN`, `PAUSED`, `LIMIT`, `CLIMIT`, `GLIMIT`, `WATCHREG`, `Guardian` | Contract (extended on every instance write and by `bump_instance_ttl`, to 535,680 ledgers) |
| WatcherRegistry | Instance | `Admins`, `Watchers`, `PendingAdminTransfer`, `TimelockDelay`, `PendingAction`, `Paused`, `Guardian`, `W_CNT` | Contract (`bump_instance_ttl`, extends to 535,680 ledgers) |
| WatcherRegistry | Persistent | `WatcherInfo`, `WatcherScope` | Contract (`extend_ttl` to `DEFAULT_TTL` = 17,280 ledgers on write; deleted on removal) |
| AlertRegistry | Instance | `NEXT_ID`, `LIVE`, `ADMIN`, `PAUSED`, `LIMIT`, `CLIMIT`, `GLIMIT`, `WATCHREG`, `PendingAdminTransfer`, `Guardian`, `GUARDIAN`, `SCHEMAV` | Contract (extended on every instance write and by `bump_instance_ttl`, to 535,680 ledgers) |
| AlertRegistry | Persistent | `Alert`, `RemovedAlert`, `AlertActive`, `AdminSuspended`, `OwnerIndex`, `OwnerLiveCount`, `OwnerActiveCount`, `ContractIndex`, `ContractLiveCount`, `DeactivationCursor`, `PendingTransfer`, `AlertExpiry`, `WebhookProposalLedger`, `AlertTags`, `SnoozeUntil`, `PruneCursor` | Contract (`extend_ttl` to `DEFAULT_TTL` = 17,280 ledgers on write, except `PendingTransfer` at `ALERT_TRANSFER_EXPIRY_LEDGERS`; `bump_alert` up to `MAX_TTL`) |
| AlertRegistry | Instance | `NEXT_ID`, `LIVE`, `ADMIN`, `PAUSED`, `LIMIT`, `CLIMIT`, `GLIMIT`, `WATCHREG`, `Guardian`, `GUARDIAN`, `SCHEMAV` | Contract (extended on every instance write and by `bump_instance_ttl`, to 535,680 ledgers) |
| WatcherRegistry | Instance | `Admins`, `Watchers`, `PendingAdminTransfer`, `TimelockDelay`, `PendingAction`, `Paused`, `W_CNT` | Contract (`bump_instance_ttl`, extends to 535,680 ledgers) |
| WatcherRegistry | Persistent | `WatcherInfo`, `WatcherScope` | Contract (`extend_ttl` to `DEFAULT_TTL` = 17,280 ledgers on write by `set_watcher_info` / `set_watcher_scope`) |
