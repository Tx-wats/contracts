# Event Reference

This document specifies the planned on-chain events emitted by both contracts.
Events follow the Soroban two-topic convention: `(category, action)`.

> **Status:** `register_alert` and `remove_alert` already emit events.
> All other entries below are **planned** — they define the topic and data
> shapes that implementors MUST follow when wiring up the remaining events.

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

---

### `watcher.replace`

Emitted when an existing watcher address is atomically replaced with a new watcher address.

| Field | Value |
|---|---|
| Topic 0 | `Symbol("watcher")` |
| Topic 1 | `Symbol("replace")` |
| Data | `(old_watcher: Address, new_watcher: Address)` |

**Status:** ✅ implemented (`replace_watcher`)

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

## Implementation Notes

- All topics use `symbol_short!` macros, which accept strings up to 9 characters.
- Data tuples are XDR-encoded by the Soroban host; keep them small (≤ 3 fields).
- Off-chain indexers should filter by `(topic0, topic1)` pairs, not by contract
  address alone, to support multi-contract deployments.
- When implementing a planned event, add a test that calls
  `env.events().all()` and asserts the emitted topic/data shape matches this
  spec exactly.
