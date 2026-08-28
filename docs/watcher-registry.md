# Watcher Registry — Function Reference

Contract that stores authorized watcher node addresses on-chain. Only registered watchers (trusted instances of `stellar-txwatch-core`) may interact with the alert registry.

The registry uses a **set of admins** (N independent signers). Any single admin can perform every privileged operation. All admin and watcher mutations emit Soroban events so changes are auditable on-chain.

All mutating entrypoints return `Result<(), ContractError>`; read entrypoints either return their value directly or a `Result` where noted. See [Errors](#errors) for the full variant list and [docs/events.md](events.md) for the authoritative event topic and data shapes.

---

## Functions

### `initialize`

Initializes the registry with a single bootstrap admin. Can only be called once.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Initial admin of the registry |

**Returns:** nothing

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

---

### `register_watcher`

Adds an address to the authorized watcher set. Any admin may call this. Idempotent — registering an already-registered watcher is a no-op with no event.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | An existing admin |
| `watcher` | `Address` | Watcher address to authorize |

**Returns:** nothing

**Events:** `("watcher", "register")` with data `(watcher: Address)` — not emitted when the watcher was already registered.

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.

---

### `remove_watcher`

Removes an address from the authorized watcher set. Any admin may call this. If the address is not currently registered the call succeeds silently and no event is emitted.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | An existing admin |
| `watcher` | `Address` | Watcher address to remove |

**Returns:** nothing

**Events:** `("watcher", "remove")` with data `(watcher: Address)` — emitted only when the watcher was actually present. Dependent systems must subscribe to this event to revoke trust immediately.

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.

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

**Parameters**

| Name | Type | Description |
|---|---|---|
| `watcher` | `Address` | Address to check |

**Returns:** `bool`

---

### `is_authorized`

Backwards-compatible alias for `is_watcher_authorized`. Identical behavior and return value; retained so existing callers do not break.

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

---

## Storage

All state is stored in **instance storage**:

| Key | Value | Description |
|---|---|---|
| `DataKey::Admins` | `Vec<Address>` | Current admin set (N independent signers) |
| `DataKey::Watchers` | `Vec<Address>` | Authorized watcher addresses |
| `W_CNT` | `u32` | Cached watcher count, kept in sync with `Watchers` for cheap reads |

---

## Re-entrancy and cross-contract safety

This contract is safe to call from other Soroban contracts. Soroban executes contract calls atomically and does not allow classic callback-style re-entrancy into the same contract within the same transaction.

All mutating entrypoints in `WatcherRegistry` require `require_auth()` on the caller and check the admin set before updating storage, and no state-changing operation performs external callbacks. This makes the registry resistant to standard cross-contract re-entrancy attacks.
