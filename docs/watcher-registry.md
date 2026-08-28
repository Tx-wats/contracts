# Watcher Registry — Function Reference

Contract that stores authorized watcher node addresses on-chain. Only registered watchers (trusted instances of `stellar-txwatch-core`) may interact with the alert registry.

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

Initializes the registry with an admin address. Can only be called once.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Initial admin of the registry |

**Returns:** `Result<(), ContractError>`

**Errors:** returns `ContractError::AlreadyInitialized` if called more than once. Through a raw client call this surfaces as a panic with `Error(Contract, #1)`; via `try_initialize` it is `Ok(Err(ContractError::AlreadyInitialized))`.

---

### `register_watcher`

Adds an address to the authorized watcher set. Idempotent — registering an already-registered watcher is a no-op.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin |
| `watcher` | `Address` | Watcher address to authorize |

**Returns:** `Result<(), ContractError>`

**Errors:**

- `ContractError::NotInitialized` (`#3`) if the contract has not been initialized.
- `ContractError::Unauthorized` (`#2`) if `admin` is not in the admin set.

A raw client call panics with `Error(Contract, #N)`; `try_register_watcher` returns `Ok(Err(ContractError::…))`.

---

### `remove_watcher`

Removes an address from the authorized watcher set.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin |
| `watcher` | `Address` | Watcher address to remove |

**Returns:** `Result<(), ContractError>`

**Errors:**

- `ContractError::NotInitialized` (`#3`) if the contract has not been initialized.
- `ContractError::Unauthorized` (`#2`) if `admin` is not in the admin set.

Removing an address that is not registered is a no-op — it returns `Ok(())` and emits no event. A raw client call panics with `Error(Contract, #N)`; `try_remove_watcher` returns `Ok(Err(ContractError::…))`.

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

**Returns:** `Result<(), ContractError>`

**Errors:**

- `ContractError::NotInitialized` (`#3`) if the contract has not been initialized.
- `ContractError::Unauthorized` (`#2`) if `admin` is not in the admin set.

Replaces the **entire** admin set with `new_admin`. A raw client call panics with `Error(Contract, #N)`; `try_transfer_admin` returns `Ok(Err(ContractError::…))`.

---

### `get_admin`

Returns the current admin address.

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
