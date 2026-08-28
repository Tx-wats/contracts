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

### `transfer_admin`

Transfers the admin role to a new address.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin |
| `new_admin` | `Address` | Address to become the new admin |

**Returns:** nothing

**Panics:** `"unauthorized"` if `admin` does not match the stored admin.

---

### `get_admin`

Returns the current admin address.

**Parameters:** none

**Returns:** `Address`

**Panics:** `"not initialized"` if the contract has not been initialized.

---

## Consumers

### `AlertRegistry`

`AlertRegistry` can be configured to gate its own read queries on this registry. When an admin calls `AlertRegistry::set_watcher_registry` with this contract's address, `AlertRegistry`'s `get_alerts_for_contract`, `get_alerts_by_owner`, and their paginated variants perform a cross-contract call to `is_watcher_authorized` before returning data, rejecting callers that are not registered watchers with `ContractError::NotAWatcher`.

Gating is optional and off by default — if no watcher registry address is configured on `AlertRegistry`, these reads are unrestricted. See `docs/alert-registry.md`'s [`set_watcher_registry`](./alert-registry.md#set_watcher_registry) for the consumer-side configuration.

Because gating is a live cross-contract read, removing a watcher here (via `remove_watcher`, `replace_watcher`, or `clear_all_watchers`) takes effect immediately for any `AlertRegistry` deployment that has gating enabled — there is no caching or delay on the consumer side.

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
