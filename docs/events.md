# Event Reference

This document specifies the on-chain events emitted by both contracts.
Events follow the Soroban two-topic convention: `(category, action)`.

> **Status:** each entry below carries its own status line. Most events are now
> implemented; the remaining `🔲 planned` entries define the topic and data
> shapes that implementors MUST follow when the corresponding function is wired
> up to emit. The per-entry status line is authoritative — this table is
> audited against the contract source (see issue #110 for the automated check).

---

## AlertRegistry

### `alert.register`

Emitted when a new alert is successfully registered.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("alert")` |
| Topic 1 | `Symbol("register")` |
| Data | `(id: u64, owner: Address, target_contract: Address)` |

**Status:** ✅ implemented (`register_alert`)

---

### `alert.update`

Emitted when an alert's rules or active flag are changed.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("alert")` |
| Topic 1 | `Symbol("update")` |
| Data | `(id: u64, owner: Address, active: bool)` |

**Status:** 🔲 planned (`update_alert`)

---

### `alert.webhook`

Emitted when an alert's webhook hash is rotated.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("alert")` |
| Topic 1 | `Symbol("webhook")` |
| Data | `(id: u64, caller: Address)` |

> The new hash is intentionally omitted from the event data — it is already
> stored on-chain and can be read via `get_alert`.  Omitting it keeps the
> event payload small and avoids redundancy.

**Status:** 🔲 planned (`update_webhook`)

---

### `alert.wh_prop`

Emitted when a webhook rotation is staged via `propose_webhook`. The live
`webhook_hash` is unchanged at this point.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("alert")` |
| Topic 1 | `Symbol("wh_prop")` |
| Data | `(id: u64, caller: Address)` |

**Status:** ✅ implemented (`propose_webhook`)

---

### `alert.wh_conf`

Emitted when a staged webhook hash is promoted to the live one via
`confirm_webhook`.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("alert")` |
| Topic 1 | `Symbol("wh_conf")` |
| Data | `(id: u64, caller: Address)` |

**Status:** ✅ implemented (`confirm_webhook`)

---

### `alert.remove`

Emitted when an alert is removed by its owner or by an admin.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("alert")` |
| Topic 1 | `Symbol("remove")` |
| Data | `(id: u64, caller: Address)` |

**Status:** ✅ implemented (`remove_alert`, `remove_alert_by_admin`)

---

### `alert.bump`

Emitted when an alert's TTL is extended via `bump_alert`.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("alert")` |
| Topic 1 | `Symbol("bump")` |
| Data | `(id: u64, ttl: u32)` |

> `ttl` is the **effective** TTL after clamping to `MAX_TTL` (535 680 ledgers
> ≈ 31 days).  Off-chain indexers can use this event to track renewal activity
> and predict when alerts will next expire.

**Status:** ✅ implemented (`bump_alert`)

---

### `admin.init`

Emitted when the admin role is first initialised.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("admin")` |
| Topic 1 | `Symbol("init")` |
| Data | `(admin: Address)` |

**Status:** 🔲 planned (`initialize`)

---

### `admin.transfer`

Emitted when the admin role is transferred to a new address.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("admin")` |
| Topic 1 | `Symbol("transfer")` |
| Data | `(old_admin: Address, new_admin: Address)` |

**Status:** ✅ implemented (`transfer_admin`)

---

### `admin.limit`

Emitted when the per-owner alert limit is changed.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("admin")` |
| Topic 1 | `Symbol("limit")` |
| Data | `(admin: Address, limit: u32)` |

**Status:** 🔲 planned (`set_per_owner_alert_limit`)

---

## WatcherRegistry

### `watcher.register`

Emitted when a new watcher address is authorised.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("watcher")` |
| Topic 1 | `Symbol("register")` |
| Data | `(watcher: Address)` |

**Status:** ✅ implemented (`register_watcher`)

---

### `watcher.remove`

Emitted when a watcher address is de-authorised.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("watcher")` |
| Topic 1 | `Symbol("remove")` |
| Data | `(watcher: Address)` |

**Status:** ✅ implemented (`remove_watcher`, `clear_all_watchers`)

> Dependent systems (e.g. `AlertRegistry` watcher-gating, off-chain trust
> stores) **must** subscribe to this event to revoke trust immediately when a
> watcher is deauthorized.  The event is only emitted when the watcher was
> actually present in the registry — removing an unregistered address is a
> silent no-op.

> **Emitted alongside `watcher.replace`.** `replace_watcher` emits *both* a
> `watcher.remove` for `old_watcher` and a `watcher.replace` carrying the same
> `old_watcher`. A listener that reacts to both event types will see the old
> watcher's revocation twice — see
> [`watcher.replace`](#watcherreplace) for how to de-duplicate.

---

### `watcher.replace`

Emitted when an existing watcher address is atomically replaced with a new watcher address.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("watcher")` |
| Topic 1 | `Symbol("replace")` |
| Data | `(old_watcher: Address, new_watcher: Address)` |

> A self-replace (`old_watcher == new_watcher`) is a no-op: the address stays
> authorized throughout, so neither `watcher.replace` nor `watcher.remove` is
> emitted.

**Status:** ✅ implemented (`replace_watcher`)

#### Dual-event behaviour

A single `replace_watcher` call emits **two** events, in this order:

1. `watcher.remove` with data `old_watcher`
2. `watcher.replace` with data `(old_watcher, new_watcher)`

Both events revoke trust for the **same** `old_watcher`. An indexer that
tracks revocations from the `watcher.remove` topic *and* separately reacts to
`watcher.replace` will therefore process the old watcher's revocation twice
for one on-chain action. De-duplicate on `(ledger sequence, contract id,
revoked address)` — every event from the same transaction shares a ledger
sequence, so the two signals collapse to one.

```ts
// Pseudocode: fold both event types into a single revocation set.
const revoked = new Set<string>();

function key(ledgerSeq: number, contractId: string, addr: string): string {
  return `${ledgerSeq}:${contractId}:${addr}`;
}

for (const ev of events) {
  const [topic0, topic1] = ev.topics;
  if (topic0 !== "watcher") continue;

  if (topic1 === "remove") {
    const addr = ev.data as string; // old/removed watcher
    revoked.add(key(ev.ledgerSeq, ev.contractId, addr));
  } else if (topic1 === "replace") {
    const [oldWatcher, newWatcher] = ev.data as [string, string];
    // `oldWatcher` may already be in `revoked` from the paired
    // `watcher.remove` above — Set membership makes this idempotent.
    revoked.add(key(ev.ledgerSeq, ev.contractId, oldWatcher));
    authorize(newWatcher); // grant trust to the replacement
  }
}
// `revoked` now holds one entry per revoked watcher, not two.
```

If you only need revocations, subscribing to `watcher.remove` alone is
sufficient — `replace_watcher` always emits it. Subscribe to
`watcher.replace` as well only when you also need the `new_watcher` half of
the rotation.

---

### `admin.init`

Emitted when the watcher registry admin is first initialised.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("admin")` |
| Topic 1 | `Symbol("init")` |
| Data | `(admin: Address)` |

**Status:** ✅ implemented (`initialize`)

---

### `admin.add`

Emitted when a new admin is added to the admin set.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("admin")` |
| Topic 1 | `Symbol("add")` |
| Data | `(caller: Address, new_admin: Address)` |

**Status:** ✅ implemented (`add_admin`)

---

### `admin.remove`

Emitted when an admin is removed from the admin set.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("admin")` |
| Topic 1 | `Symbol("remove")` |
| Data | `(caller: Address, target_admin: Address)` |

**Status:** ✅ implemented (`remove_admin`)

---

### `admin.transfer`

Emitted when the watcher registry admin role is transferred.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("admin")` |
| Topic 1 | `Symbol("transfer")` |
| Data | `(old_admin: Address, new_admin: Address)` |

**Status:** ✅ implemented (`transfer_admin`)

---

### `admin.timelock`

Emitted when the timelock delay applied to sensitive admin actions is changed.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("admin")` |
| Topic 1 | `Symbol("timelock")` |
| Data | `(caller: Address, delay_ledgers: u32)` |

**Status:** ✅ implemented (`set_timelock_delay`, `execute_admin_action`)

---

### `admin.propose`

Emitted when a sensitive admin action is queued behind the timelock.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("admin")` |
| Topic 1 | `Symbol("propose")` |
| Data | `(proposer: Address, ready_at: u32)` |

**Status:** ✅ implemented (`propose_admin_action`)

> `ready_at` is the ledger sequence at or after which the action may be
> executed. Co-admins should watch this event and cancel anything they did not
> expect before that ledger is reached.

---

### `admin.cancel`

Emitted when a queued admin action is cancelled before execution.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("admin")` |
| Topic 1 | `Symbol("cancel")` |
| Data | `(caller: Address)` |

**Status:** ✅ implemented (`cancel_admin_action`)

---

### `admin.execute`

Emitted when a queued admin action is executed after its delay has elapsed.
The action's own event (`admin.add`, `admin.transfer`, `watcher.remove`, …) is
emitted alongside it.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("admin")` |
| Topic 1 | `Symbol("execute")` |
| Data | `(caller: Address)` |

**Status:** ✅ implemented (`execute_admin_action`)

---

## Implementation Notes

- All topics use `symbol_short!` macros, which accept strings up to 9 characters.
- Data tuples are XDR-encoded by the Soroban host; keep them small (≤ 3 fields).
- Off-chain indexers should filter by `(topic0, topic1)` pairs, not by contract
  address alone, to support multi-contract deployments.
- When implementing a planned event, add a test that calls
  `env.events().all()` and asserts the emitted topic/data shape matches this
  spec exactly.
