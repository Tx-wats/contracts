# TTL Values and Storage Expiry

## What is TTL in Soroban?

Soroban persistent storage entries have a **Time-To-Live (TTL)** measured in **ledgers**. Once an entry's TTL reaches zero, the Stellar network archives it and it becomes inaccessible until restored via a fee-paying restore operation.

## Current TTL Configuration

All persistent storage entries in `alert-registry` use a **default TTL of
17 280 ledgers (~24 hours)**:

```rust
// DEFAULT_TTL = 17_280  (~24 hours at 5 s/ledger)
env.storage().persistent().extend_ttl(&key, DEFAULT_TTL, DEFAULT_TTL);
```

This applies to:
- `DataKey::Alert(id)` — individual alert configs
- `DataKey::AlertActive(id)` — per-alert active flag
- `DataKey::OwnerIndex(address)` — per-owner alert ID lists
- `DataKey::ContractIndex(address)` — per-contract alert ID lists

The `watcher-registry` uses **instance storage**, which has its own TTL managed
by the network. No registry entrypoint extends it as a side effect, so see
[Keeping the watcher registry alive](#keeping-the-watcher-registry-alive) below.

## Keeping the Watcher Registry Alive

All `WatcherRegistry` state (admins, watchers, timelock) lives in a single
instance entry. Its TTL is only refreshed when a transaction touches the
contract — and a registry whose watchers are registered once and then rarely
changed may go untouched long enough for the entry to be archived, taking the
whole registry offline until it is restored for a fee.

`bump_instance_ttl` is the keep-alive entrypoint:

```rust
// Anyone may call this — no auth, no state change.
watcher_client.bump_instance_ttl();
```

It extends the instance TTL to `INSTANCE_BUMP_AMOUNT` (535 680 ledgers, ~31
days) whenever it has dropped below `INSTANCE_BUMP_THRESHOLD` (17 280 ledgers,
~24 hours), and is the `WatcherRegistry` analogue of `AlertRegistry::bump_alert`.

**Operators should schedule a keeper to call it well inside the ~31-day
window** — a call every few days is cheap and leaves ample margin. Any other
transaction against the contract in the meantime does *not* refresh the TTL on
its own, so do not rely on incidental traffic.

## Configurable TTL via `bump_alert`

Callers can extend the TTL of any alert up to the **protocol maximum of
535 680 ledgers (~31 days)** by calling `bump_alert`:

```rust
// Extend alert 42 to the maximum lifetime (~31 days)
alert_client.bump_alert(&42u64, &535_680u32);

// Or pass u32::MAX — it is silently clamped to MAX_TTL
alert_client.bump_alert(&42u64, &u32::MAX);
```

The function:
1. Clamps the requested TTL to `MAX_TTL` (535 680 ledgers).
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

The TTL is extended on every mutating call that touches the entry:

| Function              | Entries extended |
|-----------------------|-----------------|
| `register_alert`      | `Alert(id)`, `AlertActive(id)`, `OwnerIndex`, `ContractIndex` |
| `update_alert`        | `Alert(id)`, `OwnerIndex`, `ContractIndex` |
| `update_webhook`      | `Alert(id)`, `OwnerIndex`, `ContractIndex` |
| `update_label`        | `Alert(id)`, `OwnerIndex`, `ContractIndex` |
| `update_target_contract` | `Alert(id)`, old and new `ContractIndex` |
| `bump_alert`          | `Alert(id)`, `AlertActive(id)`, `OwnerIndex`, `ContractIndex` |
| `remove_alert`        | Entry is deleted (no TTL needed) |

Read-only functions (`get_alert`, `get_alerts_for_contract`, `get_alerts_by_owner`) do **not** extend the TTL.

## Implications

- **Inactive alerts expire quickly.** An alert that is registered but never updated will be archived after ~8 minutes.
- **Watchers must keep alerts alive.** Any off-chain service relying on alert data should periodically call `update_alert` (or a dedicated bump function) to prevent expiry.
- **Archived entries are not deleted.** They can be restored via `RestoreFootprintOp`, but this requires paying a fee and is not currently automated.

## Recommendations

For production use, the TTL should be increased significantly. Common choices:

| Use case | Suggested TTL (ledgers) | Wall-clock |
|----------|------------------------|------------|
| Short-lived alerts | 17,280 | ~24 hours |
| Standard alerts | 120,960 | ~7 days |
| Long-lived alerts | 535,680 | ~31 days |

To change the TTL, update the `extend_ttl` calls in `contracts/alert-registry/src/lib.rs`:

```rust
// Example: extend to ~7 days
env.storage().persistent().extend_ttl(&DataKey::Alert(id), 120_960, 120_960);
```

## Further Reading

- [Soroban State Archival](https://developers.stellar.org/docs/learn/encyclopedia/storage/state-archival)
- [Soroban Storage TTL Reference](https://developers.stellar.org/docs/build/smart-contracts/getting-started/storing-data)
