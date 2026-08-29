# Watcher Registry — Function Reference

Contract that stores authorized watcher node addresses on-chain. Only registered watchers (trusted instances of `stellar-txwatch-core`) may interact with the alert registry.

The registry uses a **set of admins** (N independent signers). Any single admin can perform every privileged operation. All admin and watcher mutations emit Soroban events so changes are auditable on-chain.

All mutating entrypoints return `Result<(), ContractError>`; read entrypoints either return their value directly or a `Result` where noted. See [Errors](#errors) for the full variant list and [docs/events.md](events.md) for the authoritative event topic and data shapes.
Privileged operations are gated on a **multi-admin set**: any single admin may register or remove watchers and add or remove other admins. See [Role policy](#role-policy) for what an address is and is not allowed to be. Every admin and watcher mutation emits a Soroban event; see [events.md](events.md#watcherregistry) for the full topic/data catalogue.

---

## Error handling: raw vs `try_` calls

Every mutating entrypoint returns `Result<(), ContractError>` rather than
panicking on a business-rule violation. How that surfaces through the generated
SDK client depends on which method you call:

- **Raw call** (`client.register_watcher(...)`) — an `Err(ContractError::X)`
  from the contract is turned into a host error and the call **panics** with
  `Error(Contract, #N)`, where `N` is the discriminant from the
  [Errors](#errors) table.
- **`try_` call** (`client.try_register_watcher(...)`) — returns
  `Ok(Ok(()))` on success and `Ok(Err(ContractError::X))` on a typed failure,
  so you can match the variant without catching a panic.

Use `try_*` whenever a failure is an expected outcome you want to handle (for
example, `WatcherNotFound` from `replace_watcher`). This mirrors the SDK
behaviour described in [testing.md](./testing.md).

---

## Functions

### `initialize`

Initializes the registry with a single bootstrap admin. Can only be called once.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Initial (bootstrap) admin of the registry |

**Returns:** `Result<(), ContractError>`

**Errors:** returns `ContractError::AlreadyInitialized` if called more than once. Through a raw client call this surfaces as a panic with `Error(Contract, #1)`; via `try_initialize` it is `Ok(Err(ContractError::AlreadyInitialized))`.
**Returns:** `Result<(), ContractError>` — `AlreadyInitialized` if called more than once.

**Events:** `("admin", "init")` with data `(admin: Address)`

**Errors:** `AlreadyInitialized` if called more than once.

---

### `add_admin`

Adds a new admin to the admin set. Any existing admin may call this. Idempotent — adding an address that is already an admin succeeds without emitting an event.

**Requires auth:** `caller` (must be an existing admin)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | An existing admin authorizing the change |
| `new_admin` | `Address` | Address to add to the admin set |

**Returns:** nothing

**Events:** `("admin", "add")` with data `(caller: Address, new_admin: Address)` — not emitted when `new_admin` is already an admin.

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `caller` is not in the admin set.

---

### `remove_admin`

Removes an admin from the admin set. Any existing admin may call this. Refuses to remove the last admin so the contract can never become permanently unmanageable.

Removing an address that is not currently an admin still succeeds and still emits the event; the stored set is simply rewritten unchanged.

**Requires auth:** `caller` (must be an existing admin)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | An existing admin authorizing the change |
| `target_admin` | `Address` | Address to remove from the admin set |

**Returns:** nothing

**Events:** `("admin", "remove")` with data `(caller: Address, target_admin: Address)`

**Errors:**

- `LastAdmin` if removing this admin would leave the contract with no admins.
- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `caller` is not in the admin set.

---

### `transfer_admin`

Replaces the **entire** admin set with a single new admin. Any existing admin may call this. Use `add_admin` + `remove_admin` to rotate one member of a multi-admin set without dropping the others.

**Requires auth:** `admin` (must be an existing admin)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | An existing admin authorizing the transfer |
| `new_admin` | `Address` | Address to become the sole admin |

**Returns:** nothing

**Events:** `("admin", "transfer")` with data `(admin: Address, new_admin: Address)`

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.

---

### `get_admins`

Returns every current admin address.

**Parameters:** none

**Returns:** `Vec<Address>`

**Errors:** `NotInitialized` if the contract has not been initialized.

---

### `get_admin`

Returns the primary admin address (first entry in the admin set). Kept for backwards compatibility; prefer `get_admins` when you need the full set.

**Parameters:** none

**Returns:** `Address`

**Errors:** `NotInitialized` if the contract has not been initialized.
**Events:** `(Symbol("admin"), Symbol("init"))` with data `(admin: Address)`.

---

### `register_watcher`

Adds an address to the authorized watcher set. Any admin may call this. Idempotent — registering an already-registered watcher is a no-op with no event.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | An existing admin |
| `admin` | `Address` | Any current admin |
| `watcher` | `Address` | Watcher address to authorize |

**Returns:** `Result<(), ContractError>`

**Errors:**

- `ContractError::NotInitialized` (`#3`) if the contract has not been initialized.
- `ContractError::Unauthorized` (`#2`) if `admin` is not in the admin set.

A raw client call panics with `Error(Contract, #N)`; `try_register_watcher` returns `Ok(Err(ContractError::…))`.
**Returns:** `Result<(), ContractError>` — `NotInitialized` if never initialized, `Unauthorized` if `admin` is not in the admin set.

**Events:** `("watcher", "register")` with data `(watcher: Address)` — not emitted when the watcher was already registered.

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.
**Events:** `(Symbol("watcher"), Symbol("register"))` with data `(watcher: Address)`, only on the first registration of that address (skipped on idempotent repeats).

---

### `remove_watcher`

Removes an address from the authorized watcher set. Any admin may call this. If the address is not currently registered the call succeeds silently and no event is emitted.
Removes an address from the authorized watcher set. If the address is not currently registered this is a silent no-op — the call succeeds and no event is emitted.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | An existing admin |
| `admin` | `Address` | Any current admin |
| `watcher` | `Address` | Watcher address to remove |

**Returns:** `Result<(), ContractError>`

**Errors:**

- `ContractError::NotInitialized` (`#3`) if the contract has not been initialized.
- `ContractError::Unauthorized` (`#2`) if `admin` is not in the admin set.

Removing an address that is not registered is a no-op — it returns `Ok(())` and emits no event. A raw client call panics with `Error(Contract, #N)`; `try_remove_watcher` returns `Ok(Err(ContractError::…))`.
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

**Events:** `("watcher", "remove")` with data `(watcher: Address)` — emitted only when the watcher was actually present. Dependent systems must subscribe to this event to revoke trust immediately.

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.
**Returns:** `Result<(), ContractError>` — `NotInitialized` / `Unauthorized`.

**Events:** one `(Symbol("watcher"), Symbol("remove"))` with data `(watcher: Address)` per removed address, so dependent systems can revoke trust for each.

---

### `replace_watcher`

Atomically deauthorizes `old_watcher` and authorizes `new_watcher` in a single transaction, with no gap between the two operations. Any admin may call this. Useful for key rotation.

If `new_watcher` is already registered the call still succeeds: `old_watcher` is removed and the watcher count decreases by one.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | An existing admin |
| `old_watcher` | `Address` | Currently registered watcher to remove |
| `new_watcher` | `Address` | Watcher address to authorize in its place |

**Returns:** nothing

**Events:** `("watcher", "remove")` with data `(old_watcher: Address)`, then `("watcher", "replace")` with data `(old_watcher: Address, new_watcher: Address)`.

**Errors:**

- `WatcherNotFound` if `old_watcher` is not currently registered.
- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.

---

### `clear_all_watchers`

Removes every registered watcher in a single admin call and resets the watcher count to zero. Any admin may call this. Calling it on an empty set is a no-op.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | An existing admin |

**Returns:** nothing

**Events:** one `("watcher", "remove")` per previously registered watcher, each with data `(watcher: Address)`. No events when the set was already empty.

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.

---

### `is_watcher_authorized`

Checks whether an address is a currently authorized watcher.

Renamed from `is_authorized` for clarity in cross-contract call contexts — the name makes explicit *what* the address is being authorized as.
Returns the number of registered watchers as a cheap integer read, backed by the `W_CNT` counter key, so callers avoid fetching and counting the full list.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `watcher` | `Address` | Address to check |

**Returns:** `bool`

---

### `is_authorized`

Backwards-compatible alias for `is_watcher_authorized`. Identical behavior and return value; retained so existing callers do not break.
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
| `watcher` | `Address` | Address to check |

**Returns:** `bool`

---

### `get_watchers`

Returns all currently authorized watcher addresses.

**Parameters:** none

**Returns:** `Vec<Address>` — may be empty.
| `admin` | `Address` | Any current admin |
| `new_admin` | `Address` | Sole address to become the new admin set |

**Returns:** `Result<(), ContractError>` — `NotInitialized` / `Unauthorized`.

**Events:** `(Symbol("admin"), Symbol("transfer"))` with data `(old_admin: Address, new_admin: Address)`.

---

**Returns:** `Result<(), ContractError>`

**Errors:**

- `ContractError::NotInitialized` (`#3`) if the contract has not been initialized.
- `ContractError::Unauthorized` (`#2`) if `admin` is not in the admin set.

Replaces the **entire** admin set with `new_admin`. A raw client call panics with `Error(Contract, #N)`; `try_transfer_admin` returns `Ok(Err(ContractError::…))`.
### `get_admins`

Returns every address in the current admin set.

**Parameters:** none

**Returns:** `Result<Vec<Address>, ContractError>` — `NotInitialized` if the contract has not been initialized.

---

### `get_watcher_count`

Returns the number of registered watchers as a cheap integer read, avoiding the cost of fetching and deserializing the full watcher list.

**Parameters:** none

**Returns:** `u32` — the number of authorized watchers.

---

## Errors

`ContractError` is returned as the `Err` variant of every fallible entrypoint.

| Variant | Code | Meaning |
|---|---|---|
| `AlreadyInitialized` | 1 | `initialize` was called on an already-initialized contract. |
| `Unauthorized` | 2 | The caller is not in the admin set. |
| `NotInitialized` | 3 | A privileged or admin-reading entrypoint was called before `initialize`. |
| `LastAdmin` | 4 | `remove_admin` would have left the contract with no admins. |
| `WatcherNotFound` | 5 | `replace_watcher` was given an `old_watcher` that is not registered. |
Returns the primary admin address (first entry in the admin set). Kept for backwards compatibility; prefer `get_admins` when you need the full set.

**Parameters:** none

**Returns:** `Result<Address, ContractError>`

**Errors:** returns `ContractError::NotInitialized` (`#3`) if the contract has not been initialized. A raw client call panics with `Error(Contract, #3)`; `try_get_admin` returns `Ok(Err(ContractError::NotInitialized))`.

---

## Errors

The contract defines a single error enum ([`lib.rs`](../contracts/watcher-registry/src/lib.rs)). Each variant is returned as `Err(ContractError::…)` from the relevant entrypoint.

| Variant | Discriminant | Returned by | Meaning |
|---|---|---|---|
| `AlreadyInitialized` | `1` | `initialize` | `initialize` was called after the registry was already set up. |
| `Unauthorized` | `2` | `add_admin`, `remove_admin`, `transfer_admin`, `register_watcher`, `remove_watcher`, `replace_watcher`, `clear_all_watchers` | The caller is not a member of the admin set. |
| `NotInitialized` | `3` | every admin-gated entrypoint, `get_admins`, `get_admin` | A privileged call or admin read happened before `initialize`. |
| `LastAdmin` | `4` | `remove_admin` | Removing this admin would leave the registry with no admins, permanently locking it. |
| `WatcherNotFound` | `5` | `replace_watcher` | `old_watcher` is not currently registered, so there is nothing to replace. |

See [Error handling: raw vs `try_` calls](#error-handling-raw-vs-try_-calls) for how these surface through the generated SDK client.
**Returns:** `Result<Address, ContractError>` — `NotInitialized` if the contract has not been initialized.

---

## Role policy

Address roles in this registry are **not mutually exclusive and are not restricted to external accounts**:

- **An address may be both an admin and a registered watcher.** This is permitted and unvalidated by design. The watcher role grants no privileges beyond "is an authorized watcher", so an admin that is also a watcher gains nothing it could not already do.
- **Contract addresses are accepted** anywhere an `Address` is taken (admins and watchers alike). Soroban treats account and contract addresses uniformly, and the registry does not distinguish them.

Callers that need external-account-only or disjoint-role guarantees must enforce that off-chain before calling `initialize`, `add_admin`, or `register_watcher`.

---

## Consumers

### `AlertRegistry`

`AlertRegistry` can be configured to gate its own read queries on this registry. When an admin calls `AlertRegistry::set_watcher_registry` with this contract's address, `AlertRegistry`'s `get_alerts_for_contract`, `get_alerts_by_owner`, and their paginated variants perform a cross-contract call to `is_watcher_authorized` before returning data, rejecting callers that are not registered watchers with `ContractError::NotAWatcher`.

Gating is optional and off by default — if no watcher registry address is configured on `AlertRegistry`, these reads are unrestricted. See `docs/alert-registry.md`'s [`set_watcher_registry`](./alert-registry.md#set_watcher_registry) for the consumer-side configuration.

Because gating is a live cross-contract read, removing a watcher here (via `remove_watcher`, `replace_watcher`, or `clear_all_watchers`) takes effect immediately for any `AlertRegistry` deployment that has gating enabled — there is no caching or delay on the consumer side.

---

## Storage

All state is stored in **instance storage**, addressed by the `DataKey` enum plus one short-symbol counter key:

| Key | Value | Description |
|---|---|---|
| `DataKey::Admins` | `Vec<Address>` | Current admin set (N independent signers) |
| `DataKey::Watchers` | `Vec<Address>` | Authorized watcher addresses |
| `W_CNT` | `u32` | Cached watcher count, kept in sync with `Watchers` for cheap reads |

| `DataKey::Admins` | `Vec<Address>` | The current admin set (multi-admin, N-of-N independent signers; any one admin can perform privileged operations) |
| `DataKey::Watchers` | `Vec<Address>` | List of authorized watcher node addresses |
| `symbol_short!("W_CNT")` | `u32` | Cached count of registered watchers, kept in sync by `register_watcher` / `remove_watcher` / `replace_watcher` / `clear_all_watchers` so `get_watcher_count` is a cheap read that never deserializes the full `Watchers` vec |
---

## Re-entrancy and cross-contract safety

This contract is safe to call from other Soroban contracts. Soroban executes contract calls atomically and does not allow classic callback-style re-entrancy into the same contract within the same transaction.

All mutating entrypoints in `WatcherRegistry` require `require_auth()` on the caller and check the admin set before updating storage, and no state-changing operation performs external callbacks. This makes the registry resistant to standard cross-contract re-entrancy attacks.
