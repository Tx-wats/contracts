# Watcher Registry — Function Reference

Contract that stores authorized watcher node addresses on-chain. Only registered watchers (trusted instances of `stellar-txwatch-core`) may interact with the alert registry.

---

## Functions

### `initialize`

Initializes the registry with an admin address. Can only be called once.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Initial admin of the registry |

**Returns:** nothing

**Panics:** `"already initialized"` if called more than once.

---

### `register_watcher`

Adds an address to the authorized watcher set. Idempotent — registering an already-registered watcher is a no-op.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin |
| `watcher` | `Address` | Watcher address to authorize |

**Returns:** nothing

**Panics:** `"unauthorized"` if `admin` does not match the stored admin.

---

### `remove_watcher`

Removes an address from the authorized watcher set.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin |
| `watcher` | `Address` | Watcher address to remove |

**Returns:** nothing

**Panics:** `"unauthorized"` if `admin` does not match the stored admin.

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

Returns the number of registered watchers as a cheap integer read.

This function provides an efficient way to get the count of authorized watchers without requiring callers to fetch and count the full list.

**Parameters:** none

**Returns:** `u32` — the number of authorized watchers.

---

### `propose_admin_transfer`

Proposes transferring the admin role to a new address. Does **not** take effect until `new_admin` calls `accept_admin_transfer` with their own signature — this two-step flow prevents a typo'd or unowned `new_admin` from permanently locking the contract. Replaces any previously pending proposal.

**Requires auth:** `admin` (must be an existing admin)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin proposing the transfer |
| `new_admin` | `Address` | Address proposed to become the new admin |

**Returns:** nothing

**Errors:** `Unauthorized` if `admin` is not an existing admin; `NotInitialized` if the contract has not been initialized.

---

### `accept_admin_transfer`

Accepts a pending admin transfer, requiring `new_admin`'s own signature. Replaces the entire admin set with `new_admin`.

**Requires auth:** `new_admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `new_admin` | `Address` | Address accepting the proposed transfer |

**Returns:** nothing

**Errors:** `NoPendingTransfer` if no transfer is pending, or the pending proposal names a different address.

---

### `cancel_admin_transfer`

Cancels a pending admin transfer.

**Requires auth:** `admin` (must be an existing admin)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Existing admin cancelling the proposal |

**Returns:** nothing

**Errors:** `Unauthorized` if `admin` is not an existing admin; `NoPendingTransfer` if no transfer is pending.

---

### `get_admin`

Returns the current admin address.

**Parameters:** none

**Returns:** `Address`

**Panics:** `"not initialized"` if the contract has not been initialized.

---

## Storage

All state is stored in **instance storage**:

| Key | Value | Description |
|---|---|---|
| `"ADMIN"` | `Address` | Current admin address |
| `"WATCHERS"` | `Vec<Address>` | List of authorized watcher addresses |
---

## Re-entrancy and cross-contract safety

This contract is safe to call from other Soroban contracts. Soroban executes contract calls atomically and does not allow classic callback-style re-entrancy into the same contract within the same transaction.

All mutating entrypoints in `WatcherRegistry` require `admin.require_auth()` before updating storage, and no state-changing operation performs external callbacks. This makes the registry resistant to standard cross-contract re-entrancy attacks.
