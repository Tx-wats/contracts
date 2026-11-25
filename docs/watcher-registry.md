# Watcher Registry — Function Reference

Contract that stores authorized watcher node addresses on-chain. Only registered watchers (trusted instances of `stellar-txwatch-core`) may interact with the alert registry.

Watcher scopes and how `AlertRegistry` enforces them are decided in [ADR 0003](adr/0003-watcher-scopes.md).

The watcher set is capped at `MAX_WATCHERS` (100) and the admin set at `MAX_ADMINS` (10). `register_watcher`/`add_admin` return a typed error (`MaxWatchersReached`/`MaxAdminsReached`) once the cap is reached.
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

### `__constructor`

Atomic constructor executed during deployment via `stellar contract deploy -- --admin <ADDRESS>`. Sets up the bootstrap admin in the same transaction as deployment, closing the front-running window where an attacker could invoke `initialize` before the deployer.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Initial (bootstrap) admin of the registry |

**Events:** `("admin", "init")` with data `(admin: Address)`

---

### `initialize`

*(Deprecated as of v0.2.0; scheduled for removal in v0.3.0.)* Initializes the registry with a single bootstrap admin. Retained only for contracts deployed before `__constructor` existed. Every contract deployed with `__constructor` is already initialized at deploy time, so on such deployments this call can only ever return `ContractError::AlreadyInitialized`. New deployments should not call this and should not list it as a "getting started" step — use `__constructor` at deploy time instead (see above).

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

### Admin transfer: `propose_admin_transfer` / `accept_admin_transfer` / `cancel_admin_transfer`

Hands the admin role to a new address in two steps, replacing the **entire** admin set with that single address once the new admin accepts. The acceptance step requires the new admin's own signature, so a typo'd or unowned address can never lock the contract. To rotate one member of a multi-admin set without dropping the others, use `add_admin` + `remove_admin` instead.

> These semantics follow the two-step "replace the whole set" design (matching `AdminAction::TransferAdmin`). The contract source still contains a spliced-in "hand off only my own slot" variant; see #187.

**Flow**

1. An existing admin calls `propose_admin_transfer(admin, new_admin)`. The proposal is stored under `DataKey::PendingAdminTransfer`; the admin set is unchanged. A new proposal overwrites any earlier one.
2. `new_admin` calls `accept_admin_transfer(new_admin)`. The admin set becomes `[new_admin]` and the pending proposal is cleared.
3. Until it is accepted, any existing admin may call `cancel_admin_transfer(admin)` to discard the proposal.

#### `propose_admin_transfer`

**Requires auth:** `admin` (must be an existing admin)

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Existing admin proposing the transfer |
| `new_admin` | `Address` | Address proposed to become the sole admin |

**Returns:** `Result<(), ContractError>`

**Events:** `("admin", "propose")` with data `(admin: Address, new_admin: Address)`

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.
- `TimelockRequired` if a timelock delay is configured. Queue `AdminAction::TransferAdmin(new_admin)` via `propose_admin_action` instead.

#### `accept_admin_transfer`

**Requires auth:** `new_admin`

| Name | Type | Description |
|---|---|---|
| `new_admin` | `Address` | The proposed address, accepting the transfer |

**Returns:** `Result<(), ContractError>`

**Events:** `("admin", "transfer")` with data `(old_admin: Address, new_admin: Address)`

**Errors:**

- `NoPendingTransfer` if no transfer is pending, or the pending proposal names a different address.
- `AdminTransferExpired` if more than `ADMIN_TRANSFER_EXPIRY_LEDGERS` (120,960) ledgers have passed since the proposal.
- `Paused` if the contract is paused. Pausing during a suspected compromise therefore stops a pending takeover from completing; `cancel_admin_transfer` remains callable while paused so admins can discard the proposal.

#### `cancel_admin_transfer`

**Requires auth:** `admin` (must be an existing admin)

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Existing admin cancelling the proposal |

**Returns:** `Result<(), ContractError>`

**Events:** `("admin", "cancel")` with data `(admin: Address)`

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.
- `NoPendingTransfer` if no transfer is pending.

Unlike `accept_admin_transfer`, this entrypoint is **not** blocked by the pause flag, so a pending takeover can always be cancelled while the registry is paused.

#### `get_pending_admin_transfer`

Returns the address named in the current pending admin transfer, if any, so
co-admins can check for an in-flight takeover before deciding whether to
call `cancel_admin_transfer`.

**Parameters:** none

**Returns:** `Option<Address>` — `None` if no transfer is pending.

**Errors:** none (read-only call; never panics on uninitialized storage).

#### Direct `transfer_admin`

`transfer_admin(admin, new_admin)` is the single-step form: it replaces the admin set with `[new_admin]` immediately, skipping the acceptance step, so prefer the two-step flow above. It is timelock-gated: while a timelock delay is configured it returns `TimelockRequired`, and the transfer must be queued as `AdminAction::TransferAdmin` through `propose_admin_action` / `execute_admin_action`. A raw client call panics with `Error(Contract, #N)`; `try_transfer_admin` returns `Ok(Err(ContractError::…))`.

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

Adds an address to the authorized watcher set. Idempotent — registering an already-registered watcher is a no-op. Returns `MaxWatchersReached` once the registry holds `MAX_WATCHERS` (100) watchers — see [Capacity limits](#capacity-limits).
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

**Metadata and scope:** the rotation carries the old watcher's `WatcherInfo` (metadata) and `WatcherScope` over to `new_watcher`, so a scoped watcher stays scoped to the same targets after rotation and does not silently widen to "authorized for every target". Any metadata/scope stored for `new_watcher` before the call is overwritten by the old watcher's values.

**Events:** `(Symbol("watcher"), Symbol("remove"))` with data `(old_watcher: Address)`, then `(Symbol("watcher"), Symbol("replace"))` with data `(old_watcher: Address, new_watcher: Address, scope: Option<WatcherScope>)` — the carried-over scope is included so indexers can track the new address's effective scope without a follow-up read.
=======

---

### `clear_all_watchers`

Deauthorizes every registered watcher **except** the addresses in `keep`, in one admin call. Because `MIN_WATCHERS` (1) must always hold, the call requires at least one address to be kept: `keep` must be non-empty and every address in it must already be a registered watcher. The watcher counter is updated to the size of the kept set.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Any current admin |
| `keep` | `Vec<Address>` | Watcher addresses to retain; must be non-empty and all currently registered |

**Returns:** `Result<(), ContractError>` — `NotInitialized` / `Unauthorized` / `BelowMinWatchers` (empty `keep`) / `WatcherNotFound` (a kept address is not registered).

**Events:** one `(Symbol("watcher"), Symbol("remove"))` with data `(watcher: Address)` per removed address, so dependent systems can revoke trust for each. No event is emitted for kept addresses.

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.
- `BelowMinWatchers` if `keep` is empty (the registry would drop below `MIN_WATCHERS`).
- `WatcherNotFound` if an address in `keep` is not currently registered.
- `Paused` if the contract is paused.
- `TimelockRequired` if a timelock delay is configured; queue `AdminAction::ClearAllWatchers(keep)` via `propose_admin_action` instead.

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

**Metadata and scope:** the old watcher's `WatcherInfo` and `WatcherScope` are moved to `new_watcher`; the new address never starts unscoped or without metadata when the old one had them.

**Events:** `("watcher", "remove")` with data `(old_watcher: Address)`, then `("watcher", "replace")` with data `(old_watcher: Address, new_watcher: Address, scope: Option<WatcherScope>)`.

**Errors:**

- `WatcherNotFound` if `old_watcher` is not currently registered.
- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.

---

### `clear_all_watchers`

Removes every registered watcher except those listed in `keep`, in a single admin call. Any admin may call this. Since `MIN_WATCHERS` is 1, `keep` must contain at least one currently registered watcher; the watcher count is set to `keep.len()`.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | An existing admin |
| `keep` | `Vec<Address>` | Watcher addresses to retain (non-empty, all currently registered) |

**Returns:** `Result<(), ContractError>` — `NotInitialized` / `Unauthorized` / `BelowMinWatchers` / `WatcherNotFound`.

**Events:** one `("watcher", "remove")` per removed watcher, each with data `(watcher: Address)`. No events for kept addresses.

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.
- `BelowMinWatchers` if `keep` is empty.
- `WatcherNotFound` if a kept address is not registered.

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

> **Deprecated:** Deprecated as of v0.2.0; scheduled for removal in v0.3.0. Use [`is_watcher_authorized`](#is_watcher_authorized) instead.

Backwards-compatible alias for `is_watcher_authorized`. Identical behavior and return value; retained so existing callers do not break during the migration window.

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

### `set_watcher_info`

Sets or clears the metadata (`WatcherInfo`) attached to a registered watcher.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Any current admin |
| `watcher` | `Address` | Registered watcher whose metadata is being set |
| `info` | `Option<WatcherInfo>` | New metadata, or `None` to clear it |

**Returns:** `Result<(), ContractError>` — `WatcherNotFound` if `watcher` is not registered, plus `NotInitialized` / `Unauthorized`.

**Events:** `("watcher", "info")` is emitted for **both** shapes: with data `(watcher: Address, info: Some(WatcherInfo))` when metadata is set, and with data `(watcher: Address, info: None)` when it is cleared. Indexers must treat the `None` payload as "metadata removed" and drop any cached metadata for that watcher.

---

### `get_watchers`

Returns all currently authorized watcher addresses.

**Parameters:** none

**Returns:** `Vec<Address>` — may be empty.

---

### `get_admins`

Returns every address in the current admin set.

**Parameters:** none

**Returns:** `Result<Vec<Address>, ContractError>` — `NotInitialized` if the contract has not been initialized.

---

### `get_watchers_paginated`

Returns a page of authorized watcher addresses. `offset` and `limit` are saturating — an `offset` beyond the end of the list returns an empty page rather than erroring.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `offset` | `u32` | Index of the first watcher to return |
| `limit` | `u32` | Maximum number of watchers to return |

**Returns:** `Vec<Address>` — may be empty.

---

### `clear_all_watchers`

Removes all registered watchers except the addresses in `keep`, emitting one `("watcher", "remove")` event per removed watcher. `keep` must be non-empty and contain only currently registered watchers (`BelowMinWatchers` / `WatcherNotFound` otherwise).

**Limitation:** with a large enough watcher set, this can exceed per-transaction resource/event limits and become unusable. Prefer `clear_watchers_batch` for large watcher sets.

---

### `get_watcher_count`

Returns the number of registered watchers as a cheap integer read, avoiding the cost of fetching and deserializing the full watcher list.

**Parameters:** none

**Returns:** `u32` — the number of authorized watchers.

---

### `propose_admin_action`

Queues a timelocked admin action (`AdminAction::Upgrade(hash)` or `AdminAction::TransferAdmin(addr)`) for later execution via `execute_admin_action`.
### `get_configuration`

One-shot read of the registry's configuration, so dashboards and monitoring
tools do not need to call `get_admins`, `is_paused`, `get_timelock_delay`,
`get_pending_action`, `get_pending_admin_transfer` and `get_watcher_count`
separately.

**Parameters:** none

**Returns:** `WatcherRegistryConfiguration` — a struct with the following fields:

| Field | Type | Description |
|---|---|---|
| `admins` | `Vec<Address>` | Current admin set |
| `paused` | `bool` | Pause flag |
| `timelock_delay` | `u32` | Timelock delay in ledgers (`0` = disabled) |
| `pending_action` | `Option<PendingAction>` | Queued timelocked admin action, if any |
| `pending_admin_transfer` | `Option<Address>` | Address named in the pending admin transfer, if any |
| `watcher_count` | `u32` | Number of registered watchers |

**Errors:** none (read-only call; never panics on uninitialized storage — an
uninitialized registry reports empty `admins` and zeroed counters).

---

### `set_watcher_info`

Stores descriptive metadata for a watcher address.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Any current admin |
| `action` | `AdminAction` | Action to queue |

**Returns:** `Result<(), ContractError>` — `ActionAlreadyPending` if an action is already queued, `DelayTooLarge` if the configured delay exceeds `MAX_TIMELOCK_DELAY`, plus `NotInitialized` / `Unauthorized`.

**Events:** `("admin", "queue")` with data `(caller: Address, ready_at: u32, action: AdminAction)` — the queued action is included so co-admins can alert on an `Upgrade(hash)` or `TransferAdmin(addr)` during the timelock window without calling `get_pending_action`. The distinct `admin.queue` topic avoids clashing with the `admin.propose` transfer-proposal event.

---

### `are_watchers_authorized`

Batch form of `is_watcher_authorized`: checks several addresses in one call.
| `watcher` | `Address` | A **registered** watcher address |
| `info` | `WatcherInfo` | Metadata record (see below) |

`WatcherInfo` fields are length-capped: `label` and `operator` may each be at
most `MAX_WATCHER_LABEL_LEN` / `MAX_WATCHER_OPERATOR_LEN` bytes. Longer values
are rejected with `ContractError::StringTooLong`.

**Returns:** `Result<(), ContractError>`

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.
- `Paused` if the contract is paused.
- `WatcherNotFound` if `watcher` is not currently registered.
- `StringTooLong` if `label` or `operator` exceeds its cap.

**Events:** `("watcher", "info")` with data `(watcher: Address)`.

---

### `set_watcher_scope`

Stores the scope (e.g. the set of contracts or networks a watcher covers) for a
watcher address.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `watchers` | `Vec<Address>` | Addresses to check |

**Returns:** `Result<Vec<bool>, ContractError>` — one flag per input, in the same order.

**Errors:** `ContractError::TooManyWatchers` (or the equivalent batch-size error) if `watchers.len() > MAX_PAGE_SIZE`. Oversized batches are rejected rather than silently truncated, so the returned `Vec<bool>` always has exactly `watchers.len()` entries and can be zipped with the input safely.
| `admin` | `Address` | Any current admin |
| `watcher` | `Address` | A **registered** watcher address |
| `scope` | `WatcherScope` | Scope record (see below) |

The scope vector is capped at `MAX_WATCHER_SCOPE_LEN` entries; longer vectors
are rejected with `ContractError::ScopeTooLong`.

**Returns:** `Result<(), ContractError>`

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.
- `Paused` if the contract is paused.
- `WatcherNotFound` if `watcher` is not currently registered.
- `ScopeTooLong` if the scope vector exceeds its cap.

**Events:** `("watcher", "scope")` with data `(watcher: Address)`.

---

### `get_watcher_info` / `get_watcher_scope`

Read back the metadata stored by the setters above.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `watcher` | `Address` | Watcher address to look up |

**Returns:** `Option<WatcherInfo>` / `Option<WatcherScope>` — `None` when no
metadata has been stored for that address.

---

### `clear_watchers_batch`

Removes up to `max_count` registered watchers in a single call. Batched fallback for `clear_all_watchers` — call repeatedly until `get_watcher_count` returns the size of the set you intend to keep, without exceeding per-transaction resource/event limits. The call never drops the watcher count below `MIN_WATCHERS`.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin |
| `max_count` | `u32` | Maximum number of watchers to remove in this call |

**Returns:** nothing

**Errors:** `Unauthorized` if `admin` is not an existing admin; `BelowMinWatchers` if removing `max_count` watchers would drop the count below `MIN_WATCHERS`.

---

### `set_guardian`

Sets the low-privilege guardian address. The guardian may `pause` the registry but may **not** `unpause` it, so on-call responders can halt mutations without holding an upgrade-capable admin key. Mirrors `AlertRegistry::set_guardian`.

**Requires auth:** `admin` (must be an existing admin)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | An existing admin authorizing the change |
| `guardian` | `Address` | Address to designate as guardian |

**Returns:** `Result<(), ContractError>` — `NotInitialized` / `Unauthorized`.

**Events:** `(Symbol("admin"), Symbol("guardian_set"))` with data `(guardian: Address)`.

---

### `get_guardian`

Returns the current guardian address, if one has been set.

**Parameters:** none

**Returns:** `Option<Address>` — `None` if no guardian is configured.

**Errors:** none (read-only call; never panics on uninitialized storage).

---

### `pause` / `unpause`

`pause` may be called by any admin **or** the configured guardian; `unpause` may only be called by an admin. This mirrors `AlertRegistry`'s guardian semantics (#390).

**Requires auth:** `caller` (admin, or guardian for `pause`)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | An admin, or the guardian when calling `pause` |

**Returns:** `Result<(), ContractError>` — `NotInitialized` / `Unauthorized`.

**Events:** `(Symbol("admin"), Symbol("pause"))` / `(Symbol("admin"), Symbol("unpause"))` with data `(caller: Address)`.

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `caller` is neither an admin nor (for `pause`) the guardian.

---

### `set_watcher_scope`

Sets the list of contract addresses a watcher is scoped to monitor. The scope is stored as provided, after validation:

- **Duplicates are removed.** Repeated entries are collapsed so the stored scope contains each address at most once.
- **The registry's own address is rejected** — a watcher cannot be scoped to the registry itself.
- **The watcher's own address is rejected** — a watcher cannot be scoped to itself.
### `register_watchers`

Registers several watcher addresses in one call. Rewrites the watcher list once for the whole batch. Already-registered addresses are skipped, so the call is idempotent.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Any current admin |
| `watcher` | `Address` | Registered watcher whose scope is being set |
| `scope` | `Vec<Address>` | Contract addresses the watcher should monitor |

**Returns:** `Result<(), ContractError>`

| `admin` | `Address` | An existing admin |
| `watchers` | `Vec<Address>` | Watcher addresses to authorize |

**Returns:** `Result<(), ContractError>`

**Events:** one `("watcher", "register")` with data `(watcher: Address)` per newly added address. None for addresses that were already present.

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.
- `WatcherNotFound` if `watcher` is not currently registered.
- `InvalidScopeTarget` if `scope` contains the registry's own address or the watcher's own address.
- `Paused` if the contract is paused.

**Events:** `("watcher", "scope")` with data `(watcher: Address, scope: Vec<Address>)`, carrying the deduplicated scope.

---

### `get_schema_version`

Returns the storage schema version the contract was initialized with (`SCHEMAV`). Use it to decide whether `migrate` needs to run after an upgrade.

**Parameters:** none

**Returns:** `u32` — the current storage schema version.

**Errors:** `NotInitialized` if the contract has not been initialized.

---

### `migrate`

Migrates instance storage from an older schema version to the current one. The call is **resumable**: it advances the stored `SCHEMAV` one step at a time, so a migration that runs out of resources can simply be re-invoked until `get_schema_version` reports the current version. Calling it when storage is already at the current version is a no-op.

**Caveat:** unlike `register_watcher`, this call does **not** enforce `MAX_WATCHERS` and does **not** check the pause flag. It can grow the watcher set past the cap, and it still runs while the registry is paused. See [Capacity limits](#capacity-limits) and [Pause behavior](#pause-behavior).

---

### `set_watcher_scope`

Sets the list of target contracts a watcher is restricted to. Admin-only. An empty `Vec` clears the restriction, which makes the watcher unrestricted again. See [Watcher scope](#watcher-scope).

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | An existing admin |
| `watcher` | `Address` | Watcher address to scope |
| `scope` | `Vec<Address>` | Target contracts the watcher may act for. Empty means unrestricted. |

**Returns:** `Result<(), ContractError>`

**Events:** `("watcher", "scope")` with data `(watcher: Address, scope: Vec<Address>)`

**Errors:** `NotInitialized` if the contract has not been initialized. `Unauthorized` if `admin` is not in the admin set.

The `watcher` does not need to be registered. Operators can configure scope before calling `register_watcher`. This call is not blocked by the pause flag.

---

### `get_watcher_scope`

Returns the scope list stored for a watcher.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `watcher` | `Address` | Watcher address to look up |

**Returns:** `Vec<Address>`. An empty vec means no scope is set, which is the same as unrestricted.

**Errors:** none (read-only).

---

### `is_watcher_authorized_for`

Checks whether `watcher` is authorized for one specific `target` contract. This is the scoped form of `is_watcher_authorized`.

The check runs in this order:

1. If `watcher` is not registered, the result is `false`.
2. If the watcher's scope is empty or unset, the result is `true` for every target.
3. Otherwise the result is `true` only when `target` appears in the scope list.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `watcher` | `Address` | Address to check |
| `target` | `Address` | Target contract the watcher wants to act for |

**Returns:** `bool`

**Errors:** none (read-only).

---

### `set_watcher_info`

Sets or clears optional, informational metadata for a watcher. The metadata has no authorization weight. Pass `None` to clear it.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Any current admin |

**Returns:** `Result<(), ContractError>`

**Errors:**

- `NotInitialized` if the contract has not been initialized.
- `Unauthorized` if `admin` is not in the admin set.
- `Paused` if the contract is paused.

**Events:** `("admin", "migrate")` with data `(from: u32, to: u32)`.
| `admin` | `Address` | An existing admin |
| `watcher` | `Address` | Watcher address to annotate |
| `info` | `Option<WatcherInfo>` | `WatcherInfo { label: String, operator: String }`, or `None` to clear |

**Returns:** `Result<(), ContractError>`

**Events:** when `info` is `Some`, `("watcher", "info")` with data `(watcher: Address, label: String, operator: String)`. Clearing with `None` emits no event.

**Errors:** `NotInitialized` if the contract has not been initialized. `Unauthorized` if `admin` is not in the admin set.

The `watcher` does not need to be registered.

---

### `get_watcher_info`

Returns the metadata stored for a watcher.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `watcher` | `Address` | Watcher address to look up |

**Returns:** `Option<WatcherInfo>`. `None` if no metadata is set.

**Errors:** none (read-only).

---

### `are_watchers_authorized`

Batch form of `is_watcher_authorized`. Returns one `bool` per input address, in the same order.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `watchers` | `Vec<Address>` | Addresses to check. Only the first `MAX_PAGE_SIZE` (100) are checked. Entries beyond that are dropped from the result, not reported `false`. |

**Returns:** `Vec<bool>`, with one entry per checked address.

**Errors:** none (read-only).

---

### `is_admin`

Returns whether `address` is in the admin set. This is a cheap check that avoids fetching the full list with `get_admins`.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `address` | `Address` | Address to check |

**Returns:** `bool`. Returns `false` on an uninitialized registry.

**Errors:** none (read-only).

---

### `is_paused`

Returns whether the registry is paused. While paused, mutating entrypoints return `Paused`. Read-only entrypoints keep working.

**Parameters:** none

**Returns:** `bool`. `false` when the pause flag has never been set.

**Errors:** none (read-only).

---

### `get_timelock_delay`

Returns the configured timelock delay in ledgers.

**Parameters:** none

**Returns:** `u32`. `0` means the timelock is disabled.

**Errors:** none (read-only).

---

### `get_pending_action`

Returns the queued timelocked admin action, if any. Only one action can be queued at a time. Actions are queued with `propose_admin_action` and run with `execute_admin_action`.

**Parameters:** none

**Returns:** `Option<PendingAction>`, where `PendingAction` is:

| Field | Type | Description |
|---|---|---|
| `action` | `AdminAction` | The queued action |
| `proposer` | `Address` | The admin that queued it |
| `ready_at` | `u32` | Ledger sequence at or after which it can be executed |

**Errors:** none (read-only).

---

### `bump_instance_ttl`

Extends the TTL of the contract instance entry. It is permissionless and needs no auth. It never reads or changes registry state. If the remaining TTL is below `INSTANCE_BUMP_THRESHOLD` (17,280 ledgers, about 24 hours), it extends the TTL to `INSTANCE_BUMP_AMOUNT` (535,680 ledgers, about 31 days). Otherwise it does nothing.

Keepers should call this periodically on low-traffic deployments. See [docs/ttl.md](ttl.md#keeping-the-watcher-registry-alive).

**Parameters:** none

**Returns:** nothing

**Errors:** none.

---

## Errors

The contract defines a single error enum ([`lib.rs`](../contracts/watcher-registry/src/lib.rs)). Each variant is returned as `Err(ContractError::…)` from the relevant entrypoint.

| Variant | Discriminant | Returned by | Meaning |
|---|---|---|---|
| `AlreadyInitialized` | `1` | `initialize` | `initialize` was called after the registry was already set up. |
| `Unauthorized` | `2` | every admin-gated entrypoint | The caller is not a member of the admin set. |
| `NotInitialized` | `3` | every admin-gated entrypoint, `get_admins`, `get_admin` | A privileged call or admin read happened before `initialize`. |
| `LastAdmin` | `4` | `remove_admin` | Removing this admin would leave the registry with no admins, permanently locking it. |
| `WatcherNotFound` | `5` | `replace_watcher` | `old_watcher` is not currently registered, so there is nothing to replace. |
| `DelayTooLarge` | `12` | `set_timelock_delay`, `propose_admin_action` | Configured or proposed timelock delay exceeds `MAX_TIMELOCK_DELAY` (518,400 ledgers ≈ 30 days). |
| `NoPendingTransfer` | `6` | `accept_admin_transfer`, `cancel_admin_transfer` | No admin transfer is pending, or the pending proposal names a different address. |
| `AdminTransferExpired` | `16` | `accept_admin_transfer` | The pending admin transfer was proposed more than `ADMIN_TRANSFER_EXPIRY_LEDGERS` ledgers ago. |
| `Paused` | `14` | admin and watcher mutations (`add_admin`, `remove_admin`, `accept_admin_transfer`, `register_watcher`, `remove_watcher`, `replace_watcher`, `clear_all_watchers`, …) | The contract is paused; an admin must call `unpause` first. |
| `MaxWatchersReached` | `7` | `register_watcher` | Registering would exceed `MAX_WATCHERS`. |
| `BelowMinWatchers` | `15` | `remove_watcher`, `clear_all_watchers`, `clear_watchers_batch` | The operation would drop the watcher count below `MIN_WATCHERS` (e.g. an empty `keep` set). |
| `MaxAdminsReached` | `8` | `add_admin`, `execute_admin_action` | Adding would exceed `MAX_ADMINS`. |
| `TimelockRequired` | `13` | `add_admin`, `transfer_admin`, `propose_admin_transfer`, `clear_all_watchers`, `upgrade`, `set_timelock_delay` | A timelock delay is configured, so the action must go through `propose_admin_action` / `execute_admin_action`. From `set_timelock_delay`: the new delay is lower than the current one (the direct setter is raise-only; lower it via `propose_admin_action`). |
| `ActionAlreadyPending` | `9` | `propose_admin_action` | Another timelocked action is already queued. |
| `NoPendingAction` | `10` | `execute_admin_action`, `cancel_admin_action` | No timelocked action is queued. |
| `TimelockNotExpired` | `11` | `execute_admin_action` | The queued action's delay has not elapsed yet. |
| `StringTooLong` | `17` | `set_watcher_info` | A `WatcherInfo` string field exceeds its length cap. |
| `ScopeTooLong` | `18` | `set_watcher_scope` | The `WatcherScope` vector exceeds `MAX_WATCHER_SCOPE_LEN`. |

See [Error handling: raw vs `try_` calls](#error-handling-raw-vs-try_-calls) for how these surface through the generated SDK client.

---

## Role policy

Address roles in this registry are **not mutually exclusive and are not restricted to external accounts**:

- **An address may be both an admin and a registered watcher.** This is permitted and unvalidated by design. The watcher role grants no privileges beyond "is an authorized watcher", so an admin that is also a watcher gains nothing it could not already do.
- **The guardian is a low-privilege role.** A guardian may only `pause`; it cannot `unpause`, mutate admins or watchers, or perform any other privileged operation. An address may be both an admin and the guardian.
- **Contract addresses are accepted** anywhere an `Address` is taken (admins and watchers alike). Soroban treats account and contract addresses uniformly, and the registry does not distinguish them.

Callers that need external-account-only or disjoint-role guarantees must enforce that off-chain before calling `initialize`, `add_admin`, or `register_watcher`.

---

## Timelock delay

The timelock delay can be **raised** directly with `set_timelock_delay(admin, delay)`, which is raise-only: it returns `TimelockRequired` for any value below the current delay. To **lower** the delay — including disabling it with `0` — queue `AdminAction::SetTimelockDelay(new_delay)` via `propose_admin_action` and let it mature before `execute_admin_action`. `propose_admin_action` accepts lower and zero delays (subject to `MAX_TIMELOCK_DELAY`); the timelock itself protects the reduction, so a compromised admin cannot silently shorten the delay.

---

## Pause behavior

The contract can be paused by an admin **or the guardian**, which prevents all state-mutating operations (`add_admin`, `remove_admin`, `register_watcher`, `remove_watcher`, `replace_watcher`, `clear_all_watchers`, `accept_admin_transfer`, etc.). Only an admin may `unpause`. Pausing only affects mutations — read-only queries like `is_watcher_authorized`, `get_watchers`, and `get_admins` continue to work.
The contract can be paused by an admin, which prevents all state-mutating operations (`add_admin`, `remove_admin`, `register_watcher`, `remove_watcher`, `replace_watcher`, `clear_all_watchers`, `accept_admin_transfer`, `set_watcher_scope`, `migrate`, etc.). Pausing only affects mutations — read-only queries like `is_watcher_authorized`, `get_watchers`, `get_admins`, and `get_schema_version` continue to work.

`accept_admin_transfer` is blocked while paused, so pausing during a suspected compromise stops a pending admin takeover from completing. `cancel_admin_transfer` is deliberately **not** blocked by the pause flag, so admins can always discard a pending proposal while the registry is paused.
The contract can be paused by an admin, which prevents all state-mutating operations (`add_admin`, `remove_admin`, `register_watcher`, `remove_watcher`, `replace_watcher`, `clear_all_watchers`, `accept_admin_transfer`, `set_watcher_info`, `set_watcher_scope`, etc.). Pausing only affects mutations — read-only queries like `is_watcher_authorized`, `get_watchers`, `get_admins`, and `get_configuration` continue to work.

When `AlertRegistry` has gating enabled (configured to use `WatcherRegistry` as its authorization source), pausing `WatcherRegistry` does **not** prevent gated reads from `AlertRegistry`. The gating check calls the read-only `is_watcher_authorized` method, which is unaffected by the pause flag. This ensures that legitimate watchers can continue querying alert data even when the watcher registry is paused, since the pause only blocks mutations, not authorization checks.

Conversely, pausing `AlertRegistry` does not prevent mutations in `WatcherRegistry`. The two contracts are independent — pausing one does not affect the other.

---

## Watcher scope

Each watcher can carry an optional **scope**: a list of target contract addresses it is allowed to act for.

- **Unscoped (default).** A watcher with no scope, or an empty scope, is authorized for every target. This keeps existing deployments unchanged.
- **Scoped.** A watcher with a non-empty scope is authorized only for the listed targets. Use `is_watcher_authorized_for(watcher, target)` to check this.
- **Set by admins only** through `set_watcher_scope`. Scope can be set before the watcher is registered. Scope does not grant registration: a watcher must still be registered for any check to return `true`.
- **Stored separately** under `DataKey::WatcherScope(Address)` and not in the `Watchers` vector, so changing scope does not rewrite the hot watcher list.

**How AlertRegistry uses it today:** it does **not** use scope. `AlertRegistry`'s gate (see [Consumers](#consumers)) calls the global `is_watcher_authorized(querier)` and never calls `is_watcher_authorized_for`. A watcher with a scope set here still passes `AlertRegistry`'s gate for every target. Per-target scope is therefore informational until `AlertRegistry` (or another consumer) calls `is_watcher_authorized_for`. Consumers that need per-target isolation must make that call themselves.

---

## Consumers

### `AlertRegistry`

`AlertRegistry` can be configured to gate its own read queries on this registry. When an admin calls `AlertRegistry::set_watcher_registry` with this contract's address, `AlertRegistry`'s `get_alerts_for_contract`, `get_alerts_by_owner`, and their paginated variants perform a cross-contract call to `is_watcher_authorized` before returning data, rejecting callers that are not registered watchers with `ContractError::NotAWatcher`.

Gating is optional and off by default — if no watcher registry address is configured on `AlertRegistry`, these reads are unrestricted. See `docs/alert-registry.md`'s [`set_watcher_registry`](./alert-registry.md#set_watcher_registry) for the consumer-side configuration.

Because gating is a live cross-contract read, removing a watcher here (via `remove_watcher`, `replace_watcher`, or `clear_all_watchers` with a `keep` set that excludes it) takes effect immediately for any `AlertRegistry` deployment that has gating enabled — there is no caching or delay on the consumer side.

---

## Storage

All state is stored in **instance storage**, addressed by the `DataKey` enum plus one short-symbol counter key:

| Key | Value | Description |
|---|---|---|
| `DataKey::Admins` | `Vec<Address>` | Current admin set |
| `DataKey::Watchers` | `Vec<Address>` | Authorized watcher addresses |
| `DataKey::PendingAdminTransfer` | `(Address, u32)` | `(new_admin, expires_at_ledger)` awaiting `accept_admin_transfer` |
| `DataKey::TimelockDelay` | `u32` | Timelock delay in ledgers (`0`/absent = disabled) |
| `DataKey::PendingAction` | `PendingAction` | Queued timelocked admin action |
| `DataKey::Paused` | `bool` | Pause flag |
| `DataKey::Guardian` | `Address` | Optional guardian allowed to `pause` (not `unpause`) |
| `DataKey::SchemaVersion` | `u32` | Storage schema version (`SCHEMAV`), set by the constructor and advanced by `migrate` |
| `"W_CNT"` | `u32` | Cached watcher count |

Per-address watcher metadata lives in **persistent storage** so it does not
inflate the instance entry that every invocation loads:

| Key | Value | Description |
|---|---|---|
| `DataKey::WatcherInfo(Address)` | `WatcherInfo` | Per-watcher metadata written by `set_watcher_info` |
| `DataKey::WatcherScope(Address)` | `WatcherScope` | Per-watcher scope written by `set_watcher_scope` |

Both persistent entries are extended on write and on read using the contract's
persistent TTL bump, so they survive as long as they are touched within the TTL
window. Entries are only written for **registered** watchers, and both setters
are pause-gated. String fields and scope vectors are bounded (see
[Capacity limits](#capacity-limits)) so a single entry can never approach the
ledger entry size limit.

See [storage.md](storage.md#watcherregistry) for TTL behaviour.

---

## Capacity limits

Both sets live under a single instance-storage key, and every mutation loads,
scans and rewrites the whole `Vec<Address>`. To keep those O(n) operations
within a transaction's resource budget the sets are bounded:

| Constant | Value | Enforced by |
|---|---|---|
| `MAX_WATCHERS` | 100 | `register_watcher` → `MaxWatchersReached` |
| `MAX_ADMINS` | 10 | `add_admin` → `MaxAdminsReached` |
| `MAX_WATCHER_LABEL_LEN` | 64 | `set_watcher_info` → `StringTooLong` |
| `MAX_WATCHER_OPERATOR_LEN` | 64 | `set_watcher_info` → `StringTooLong` |
| `MAX_WATCHER_SCOPE_LEN` | 32 | `set_watcher_scope` → `ScopeTooLong` |

Registering an address that is already present is still a no-op at the cap, so
idempotent re-registration never fails. `replace_watcher` also stays available
at the cap, since it removes one address for each one it adds.

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
