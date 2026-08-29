# Watcher Registry — Function Reference

Contract that stores authorized watcher node addresses on-chain. Only registered watchers (trusted instances of `stellar-txwatch-core`) may interact with the alert registry.

Privileged operations are gated on a **multi-admin set**: any single admin may register or remove watchers and add or remove other admins. See [Role policy](#role-policy) for what an address is and is not allowed to be. Every admin and watcher mutation emits a Soroban event; see [events.md](events.md#watcherregistry) for the full topic/data catalogue.

---

## Functions

### `initialize`

Initializes the registry with a single bootstrap admin. Can only be called once.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Initial (bootstrap) admin of the registry |

**Returns:** `Result<(), ContractError>` — `AlreadyInitialized` if called more than once.

**Events:** `(Symbol("admin"), Symbol("init"))` with data `(admin: Address)`.

---

### `register_watcher`

Adds an address to the authorized watcher set. Idempotent — registering an already-registered watcher is a no-op.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Any current admin |
| `watcher` | `Address` | Watcher address to authorize |

**Returns:** `Result<(), ContractError>` — `NotInitialized` if never initialized, `Unauthorized` if `admin` is not in the admin set.

**Events:** `(Symbol("watcher"), Symbol("register"))` with data `(watcher: Address)`, only on the first registration of that address (skipped on idempotent repeats).

---

### `remove_watcher`

Removes an address from the authorized watcher set. If the address is not currently registered this is a silent no-op — the call succeeds and no event is emitted.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Any current admin |
| `watcher` | `Address` | Watcher address to remove |

**Returns:** `Result<(), ContractError>` — `NotInitialized` if never initialized, `Unauthorized` if `admin` is not in the admin set.

**Events:** `(Symbol("watcher"), Symbol("remove"))` with data `(watcher: Address)`, emitted only when the watcher was actually present.

---

### `replace_watcher`

Atomically deauthorizes `old_watcher` and authorizes `new_watcher` in a single transaction, with no gap between the two operations. Useful for watcher key rotation. If `new_watcher` is already registered the call still succeeds (the old entry is removed and the new entry remains).

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Any current admin |
| `old_watcher` | `Address` | Currently registered watcher to remove |
| `new_watcher` | `Address` | Address to authorize in its place |

**Returns:** `Result<(), ContractError>` — `WatcherNotFound` if `old_watcher` is not registered, plus `NotInitialized` / `Unauthorized`.

**Events:** `(Symbol("watcher"), Symbol("remove"))` with data `(old_watcher: Address)`, then `(Symbol("watcher"), Symbol("replace"))` with data `(old_watcher: Address, new_watcher: Address)`.

---

### `clear_all_watchers`

Bulk-deauthorizes every registered watcher in one admin call and resets the watcher counter to zero.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Any current admin |

**Returns:** `Result<(), ContractError>` — `NotInitialized` / `Unauthorized`.

**Events:** one `(Symbol("watcher"), Symbol("remove"))` with data `(watcher: Address)` per removed address, so dependent systems can revoke trust for each.

---

### `is_watcher_authorized`

Checks whether an address is a currently authorized watcher.

Renamed from `is_authorized` for clarity in cross-contract call contexts — the name makes explicit *what* the address is being authorized as.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `watcher` | `Address` | Address to check |

**Returns:** `bool`

---

### `get_watchers`

Returns all currently authorized watcher addresses.

**Parameters:** none

**Returns:** `Vec<Address>` — may be empty.

---

### `get_watcher_count`

Returns the number of registered watchers as a cheap integer read, backed by the `W_CNT` counter key, so callers avoid fetching and counting the full list.

**Parameters:** none

**Returns:** `u32` — the number of authorized watchers.

---

### `add_admin`

Adds an address to the admin set. Any existing admin may call this. Idempotent — adding an address that is already an admin is a no-op.

**Requires auth:** `caller` (must be an existing admin)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Any current admin |
| `new_admin` | `Address` | Address to add to the admin set |

**Returns:** `Result<(), ContractError>` — `NotInitialized` / `Unauthorized`.

**Events:** `(Symbol("admin"), Symbol("add"))` with data `(caller: Address, new_admin: Address)`, skipped on idempotent repeats.

---

### `remove_admin`

Removes an address from the admin set. Any existing admin may call this. Refuses to remove the last admin.

**Requires auth:** `caller` (must be an existing admin)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Any current admin |
| `target_admin` | `Address` | Address to remove from the admin set |

**Returns:** `Result<(), ContractError>` — `LastAdmin` if this is the only admin, plus `NotInitialized` / `Unauthorized`.

**Events:** `(Symbol("admin"), Symbol("remove"))` with data `(caller: Address, target_admin: Address)`.

---

### `transfer_admin`

Replaces the **entire** admin set with a single new admin. Any existing admin may call this. Use `add_admin` + `remove_admin` to rotate one member of a multi-admin set without losing the others.

**Requires auth:** `admin` (must be an existing admin)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Any current admin |
| `new_admin` | `Address` | Sole address to become the new admin set |

**Returns:** `Result<(), ContractError>` — `NotInitialized` / `Unauthorized`.

**Events:** `(Symbol("admin"), Symbol("transfer"))` with data `(old_admin: Address, new_admin: Address)`.

---

### `get_admins`

Returns every address in the current admin set.

**Parameters:** none

**Returns:** `Result<Vec<Address>, ContractError>` — `NotInitialized` if the contract has not been initialized.

---

### `get_admin`

Returns the primary admin address (first entry in the admin set). Kept for backwards compatibility; prefer `get_admins` when you need the full set.

**Parameters:** none

**Returns:** `Result<Address, ContractError>` — `NotInitialized` if the contract has not been initialized.

---

## Role policy

Address roles in this registry are **not mutually exclusive and are not restricted to external accounts**:

- **An address may be both an admin and a registered watcher.** This is permitted and unvalidated by design. The watcher role grants no privileges beyond "is an authorized watcher", so an admin that is also a watcher gains nothing it could not already do.
- **Contract addresses are accepted** anywhere an `Address` is taken (admins and watchers alike). Soroban treats account and contract addresses uniformly, and the registry does not distinguish them.

Callers that need external-account-only or disjoint-role guarantees must enforce that off-chain before calling `initialize`, `add_admin`, or `register_watcher`.

---

## Storage

All state is stored in **instance storage**, addressed by the `DataKey` enum plus one short-symbol counter key:

| Key | Value | Description |
|---|---|---|
| `DataKey::Admins` | `Vec<Address>` | The current admin set (multi-admin, N-of-N independent signers; any one admin can perform privileged operations) |
| `DataKey::Watchers` | `Vec<Address>` | List of authorized watcher node addresses |
| `symbol_short!("W_CNT")` | `u32` | Cached count of registered watchers, kept in sync by `register_watcher` / `remove_watcher` / `replace_watcher` / `clear_all_watchers` so `get_watcher_count` is a cheap read that never deserializes the full `Watchers` vec |
---

## Re-entrancy and cross-contract safety

This contract is safe to call from other Soroban contracts. Soroban executes contract calls atomically and does not allow classic callback-style re-entrancy into the same contract within the same transaction.

All mutating entrypoints in `WatcherRegistry` require `admin.require_auth()` before updating storage, and no state-changing operation performs external callbacks. This makes the registry resistant to standard cross-contract re-entrancy attacks.
