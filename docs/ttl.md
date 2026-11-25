# TTL Values and Storage Expiry

## What is TTL in Soroban?

Soroban persistent storage entries have a **Time-To-Live (TTL)** measured in **ledgers**. Once an entry's TTL reaches zero, the Stellar network archives it and it becomes inaccessible until restored via a fee-paying restore operation.

## Current TTL Configuration

`alert-registry` has **ten persistent keys**. Most are written with a **default
TTL of 17 280 ledgers (~24 hours)**:

```rust
// DEFAULT_TTL = 17_280  (~24 hours at 5 s/ledger)
env.storage().persistent().extend_ttl(&key, DEFAULT_TTL, DEFAULT_TTL);
```

This applies to:
- `DataKey::Alert(id)` — individual alert configs
- `DataKey::AlertActive(id)` — per-alert active flag
- `DataKey::AlertExpiry(id)` — per-alert expiry timestamp
- `DataKey::AlertTags(id)` — per-alert tag list
- `DataKey::SnoozeUntil(id)` — per-alert snooze deadline
- `DataKey::AlertTarget(id)` — per-alert target contract
- `DataKey::WebhookProposalLedger(id)` — ledger at which the pending webhook proposal was made
- `DataKey::AdminSuspended(id)` — present while an admin has suspended the alert
- `DataKey::RemovedAlert(id)` — removal tombstone (see [Other Persistent Entries](#other-persistent-entries))
- `DataKey::OwnerIndex(address)` — per-owner alert ID lists
- `DataKey::OwnerLiveCount(address)` — per-owner live alert count
- `DataKey::OwnerActiveCount(address)` — per-owner active alert count
- `DataKey::ContractIndex(address)` — per-contract alert ID lists
- `DataKey::ContractLiveCount(address)` — per-contract live alert count
- `DataKey::DeactivationCursor(address)` — resume point for a bulk deactivation
- `DataKey::PruneCursor(address)` — resume point for an expired-alert prune

The exception is `DataKey::PendingTransfer(id)`, which uses
`ALERT_TRANSFER_EXPIRY_LEDGERS` (see [Non-Default TTLs](#non-default-ttls)).
For each key's tier and value type, see [docs/storage.md](storage.md).

The `watcher-registry` keeps its admins, watchers and timelock in **instance
The `watcher-registry` keeps its admin, watcher list and timelock in **instance
storage**, which has its own TTL managed by the network. No registry entrypoint
extends it as a side effect, so see
[Keeping the watcher registry alive](#keeping-the-watcher-registry-alive) below.
Its per-watcher metadata (registration ledger, label and similar bookkeeping)
lives in persistent entries written with `DEFAULT_TTL` and refreshed by the
registry's own mutators.

It also has **two persistent keys**, both written with `DEFAULT_TTL` and
extended on every read and write:

- `DataKey::WatcherInfo(address)` — the per-watcher `label` and `operator`
  metadata set by `set_watcher_info`
- `DataKey::WatcherScope(address)` — the per-watcher scope list set by
  `set_watcher_scope`

Both setters require the address to be a registered watcher, reject labels and
operators longer than `MAX_WATCHER_LABEL_LEN` / `MAX_WATCHER_OPERATOR_LEN`, and
reject scope vectors longer than `MAX_WATCHER_SCOPE_LEN` (each scope string is
capped at `MAX_WATCHER_SCOPE_ITEM_LEN`). They also honour the registry pause
flag. Because the entries are persistent, they are not loaded on every
invocation and cannot push the instance entry past its size limit. Reads
(`get_watcher_info`, `get_watcher_scope`) extend the TTL of the entry they
return, so a watcher that is queried regularly stays live; an entry that is
never read or rewritten expires after `DEFAULT_TTL` and reads back as absent.
=======

## Non-Default TTLs

Only these paths write a TTL other than `DEFAULT_TTL`:

| Entry | TTL (ledgers) | Wall-clock | Set by |
|-------|---------------|------------|--------|
| `PendingTransfer(id)` | `ALERT_TRANSFER_EXPIRY_LEDGERS` = 120 960 | ~7 days | `propose_alert_transfer` |
| `Alert`, `AlertActive`, `AlertExpiry`, `AlertTags`, `SnoozeUntil`, `AlertTarget`, `OwnerIndex`, `OwnerActiveCount`, `ContractIndex` (and the other alert-dependent entries) | Requested, capped at `MAX_TTL` = 535 680 (at least `DEFAULT_TTL`; smaller values are rejected with `InvalidTtl`) | up to ~31 days | `bump_alert` (anyone) |
| The same alert-dependent entries | Requested, between `DEFAULT_TTL` and `MAX_TTL` | up to ~31 days | `renew_alert_ttl_for` (owner only) |
| The same alert-dependent entries | at least `ALERT_TRANSFER_EXPIRY_LEDGERS` = 120 960 | ~7 days | `propose_alert_transfer`, so the alert cannot expire while the transfer is open |
| `WebhookProposalLedger(id)` | `DEFAULT_TTL` = 17 280, refreshed by `propose_webhook` | ~24 hours | `propose_webhook` |
| Instance entry (both registries) | `INSTANCE_BUMP_AMOUNT` = 535 680 | ~31 days | `bump_instance_ttl` |

`PendingTransfer` is written only by `propose_alert_transfer` and is not
refreshed by any other mutator, so a proposed transfer lapses after its own
~7-day window regardless of later activity on the alert.

### Other Persistent Entries

**Removal tombstones.** `remove_alert`, `remove_alert_by_admin` and
`batch_remove_alert` replace `Alert(id)` with a `RemovedAlert(id)` tombstone
(`active == false`) so incremental sync can report the removal. The tombstone
is written with `TOMBSTONE_TTL` (120,960 ledgers, about 7 days) and nothing
extends it afterwards: `touch_alert` does not cover it and `bump_alert` cannot
reach it. `get_min_sync_ledger()` returns `current_ledger - TOMBSTONE_TTL`
(saturating at 0), the oldest sync cursor guaranteed to see every removal. A
client whose `since_ledger` cursor is below it may have missed tombstones and
must do a full resync (discard local state, then sync from `0`).

**Bulk deactivation cursor.** `deactivate_all_alerts` stores
`DeactivationCursor(owner)` with `DEFAULT_TTL` only when a call stops at
`MAX_DEACTIVATIONS_PER_CALL` before the end of the owner's index. The cursor is
removed when the sweep finishes or the owner's index changes. If it expires,
the next call restarts from the beginning, which is harmless because alerts
already deactivated are skipped.

**Live counters.** `ContractLiveCount`, `OwnerLiveCount` and `OwnerActiveCount`
are written with `DEFAULT_TTL` and extended by `touch_alert` whenever they are
present. If `ContractLiveCount` expires, `contract_live_count` rebuilds it from
`ContractIndex` on the next read. `OwnerLiveCount` and `OwnerActiveCount` have
no such rebuild: if they expire, the corresponding getter reads them as `0`
until the next write.

**Per-alert metadata.** `AlertExpiry`, `AlertTags`, `SnoozeUntil` and
`AlertTarget` are written alongside the alert with `DEFAULT_TTL` and extended by
`touch_alert` whenever they are present, so they follow the alert's TTL.
`WebhookProposalLedger` is written by `propose_webhook` with `DEFAULT_TTL` and
is removed when the proposal is confirmed or cancelled.

**Admin suspension.** `AdminSuspended(id)` is written with the alert and
extended by `touch_alert` whenever it is present, so it follows the alert's
TTL.

## Keeping the Watcher Registry Alive

The `WatcherRegistry` admin, watcher list and timelock live in a single
instance entry. Its TTL is only refreshed when a transaction touches the
contract — and a registry whose watchers are registered once and then rarely
changed may go untouched long enough for the entry to be archived, taking the
whole registry offline until it is restored for a fee. (The per-watcher
`WatcherInfo` and `WatcherScope` entries are persistent and are not affected by
instance archival; see above.)
=======

`bump_instance_ttl` is the keep-alive entrypoint:

```rust
// Anyone may call this — no auth, no state change.
watcher_client.bump_instance_ttl();
```

It extends the instance TTL to `INSTANCE_BUMP_AMOUNT` (535 680 ledgers, ~31
days) whenever it has dropped below `INSTANCE_BUMP_THRESHOLD` (17 280 ledgers,
~24 hours), and is the `WatcherRegistry` analogue of `AlertRegistry::bump_alert`.

**Operators should schedule a keeper to call it every 12 hours.** The call only
extends the entry once its remaining TTL is at or below the 24-hour threshold,
so the cadence must be shorter than that window: a multi-day cadence can miss
it and let the entry expire. Any other transaction against the contract in the
meantime does *not* refresh the TTL on its own, so do not rely on incidental
traffic. See [docs/keeper-guide.md](keeper-guide.md) for the full cadence table.

## Keeping the Alert Registry Instance Alive

`AlertRegistry` also keeps state in its instance entry: the admin, the alert ID
counter (`NEXT_ID`), the per-owner, per-contract and global limits, the pause
flag and the watcher registry address. If that entry is archived, every alert
becomes unreachable until it is restored, even though the alerts themselves
are persistent entries.

Unlike `WatcherRegistry`, AlertRegistry extends its instance entry on **every
write to instance storage**: each `register_alert` / `batch_register_alert`
(through the ID counter) and every admin setter (`initialize`,
`transfer_admin`, `pause`, `unpause`, `set_per_owner_alert_limit`,
`set_per_contract_alert_limit`, `set_global_alert_limit`,
`set_watcher_registry`, `clear_watcher_registry`). Each extends the instance
TTL to `INSTANCE_BUMP_AMOUNT` (535 680 ledgers, ~31 days) once it has dropped
below `INSTANCE_BUMP_THRESHOLD` (17 280 ledgers, ~24 hours), the same values as
`WatcherRegistry`.

For quiet periods with no registrations or admin changes, it exposes the same
permissionless keep-alive:

```rust
// Anyone may call this — no auth, no state change.
alert_client.bump_instance_ttl();
```

**Operators should have a keeper call it every 12 hours**, alongside
`WatcherRegistry::bump_instance_ttl`, for the same threshold reason. Reads and
alert updates do not touch the instance entry's TTL.

## Configurable TTL via `bump_alert`

Callers can extend the TTL of any alert up to the **protocol maximum of
535 680 ledgers (~31 days)** by calling `bump_alert`:

```rust
// Extend alert 42 to the maximum lifetime (~31 days)
alert_client.bump_alert(&42u64, &MAX_TTL);
```

Request at most `MAX_TTL`. The current build caps larger values at `MAX_TTL`,
but whether they should be capped or rejected is still open (#28), so callers
should not rely on the capping.

The function:
1. Caps the requested TTL at `MAX_TTL` (535 680 ledgers).
2. Extends `Alert`, `AlertActive`, `OwnerIndex`, and `ContractIndex` entries.
3. Emits an `("alert", "bump")` event with `(id, effective_ttl)`.

No auth is required — any address (e.g. an off-chain keeper service) may
bump an alert's TTL without modifying its content.

## Wall-Clock Time Estimate

Stellar ledgers close approximately every **5 seconds**.

| Ledgers | Approximate wall-clock time |
|---------|-----------------------------|
| 100     | ~8 minutes 20 seconds       |
| 1,000   | ~1 hour 23 minutes          |
| 17,280  | ~24 hours (DEFAULT_TTL)     |
| 120,960 | ~7 days                     |
| 535,680 | ~31 days (MAX_TTL)          |

**The default TTL of 17 280 ledgers keeps an alert alive for ~24 hours after
the last write.**  Use `bump_alert` to extend beyond that without modifying
the alert's content.

## When Does the TTL Reset?

Every mutating call refreshes the **whole set** of entries an alert depends on
through a single internal helper, `touch_alert`:
`Alert(id)`, `AlertActive(id)`, `AlertExpiry(id)`, `AlertTags(id)`,
`SnoozeUntil(id)`, `AlertTarget(id)`, `OwnerIndex(owner)`, `OwnerLiveCount(owner)`,
`OwnerActiveCount(owner)`, `ContractIndex(target)`,
plus `ContractLiveCount(target)` and `AdminSuspended(id)` when they are present.
Mutators that rewrite the alert call `persist_alert`, which writes the config
and its `AlertActive` flag and then calls `touch_alert` with `DEFAULT_TTL`, so
no mutator can extend one entry and forget another. (`OwnerLiveCount` and
`OwnerActiveCount` are only extended when present; see `docs/storage.md`.)

| Function              | Entries extended |
|-----------------------|-----------------|
| `register_alert`      | Full set, to `DEFAULT_TTL` |
| `update_alert`        | Full set, to `DEFAULT_TTL` |
| `update_webhook`      | Full set, to `DEFAULT_TTL` |
| `update_label`        | Full set, to `DEFAULT_TTL` |
| `propose_webhook`     | Full set, to `DEFAULT_TTL` |
| `confirm_webhook`     | Full set, to `DEFAULT_TTL` |
| `cancel_webhook_proposal` | Full set, to `DEFAULT_TTL` |
| `deactivate_alert_by_admin` | Full set, to `DEFAULT_TTL` |
| `accept_alert_transfer` | Full set with the **new** owner's `OwnerIndex`/`OwnerLiveCount`, to `DEFAULT_TTL` |
| `propose_alert_transfer` | `PendingTransfer(id)` only, to `ALERT_TRANSFER_EXPIRY_LEDGERS` |
| `update_target_contract` | Full set with the **new** target's `ContractIndex`, to `DEFAULT_TTL`; the old target's index is rewritten and extended |
| `deactivate_all_alerts` | Full set for each deactivated alert, to `DEFAULT_TTL`; `DeactivationCursor(owner)` to `DEFAULT_TTL` while a sweep is unfinished |
| `renew_alert_ttl`     | Full set, to `DEFAULT_TTL` — data unchanged |
| `renew_alert_ttl_for` | Full set, to the requested TTL (between `DEFAULT_TTL` and `MAX_TTL`) — data unchanged |
| `bump_alert`          | Full set, to the requested TTL (capped at `MAX_TTL`) — data unchanged |
| `remove_alert`        | `Alert(id)` and `AlertActive(id)` are deleted; `RemovedAlert(id)` tombstone written to `DEFAULT_TTL` and not extended again; owner/contract indexes and both live counts are rewritten and extended |
| `remove_alert_by_admin` | Same as `remove_alert` |
| `batch_remove_alert`  | Same as `remove_alert`, for each ID |

Read-only functions (`get_alert`, `get_alerts_for_contract`, `get_alerts_by_owner`) do **not** extend the TTL.

## Implications

- **Inactive alerts expire eventually.** An alert that is registered but never updated will be archived after `DEFAULT_TTL` (17,280 ledgers, ~24 hours).
- **Watchers must keep alerts alive.** Any off-chain service relying on alert data should periodically call `bump_alert` (permissionless — any address may call it) or `renew_alert_ttl_for` (owner only) to prevent expiry. Do **not** use `update_alert` as a keep-alive: it rewrites the alert's content and bumps its revision, which is visible to sync clients and watchers.
- **Archived entries are not deleted.** They can be restored via `RestoreFootprintOp`, but this requires paying a fee and is not currently automated.

## Recommendations

For production use, the TTL should be increased significantly. Common choices:

| Use case | Suggested TTL (ledgers) | Wall-clock |
|----------|------------------------|------------|
| Short-lived alerts | 17,280 | ~24 hours |
| Standard alerts | 120,960 | ~7 days |
| Long-lived alerts | 535,680 | ~31 days |

To change the TTL, update `DEFAULT_TTL` in `contracts/alert-registry/src/lib.rs`.
Every mutator refreshes alert entries through `persist_alert` / `touch_alert`,
so the constant is the single place to change:

```rust
// Example: extend to ~7 days
pub const DEFAULT_TTL: u32 = 120_960;
```

`ALERT_TRANSFER_EXPIRY_LEDGERS` and the `INSTANCE_BUMP_*` constants are separate
and do not follow `DEFAULT_TTL`. The `RemovedAlert` tombstone uses its own
`TOMBSTONE_TTL`, which sets how long removals stay visible to sync.

## Further Reading

- [Soroban State Archival](https://developers.stellar.org/docs/learn/encyclopedia/storage/state-archival)
- [Soroban Storage TTL Reference](https://developers.stellar.org/docs/build/smart-contracts/getting-started/storing-data)
