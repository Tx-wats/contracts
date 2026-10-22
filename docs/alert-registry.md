# Alert Registry — Function Reference

Contract that stores alert configurations on-chain, keyed by contract address.

---

## Data Types

### `AlertConfig`

| Field | Type | Description |
|---|---|---|
| `label` | `String` | Human-readable name for the alert |
| `webhook_hash` | `BytesN<32>` | SHA-256 digest of the webhook URL, as 32 raw bytes (privacy-preserving; see [Webhook Hash Scheme](#webhook-hash-scheme) below) |
| `rules` | `Vec<String>` | Serialized rule descriptors |
| `owner` | `Address` | Address that owns this config |
| `target_contract` | `Address` | Contract being watched |
| `created_at` | `u64` | Ledger timestamp at creation |
| `updated_at` | `u64` | Ledger timestamp of last update |
| `updated_ledger` | `u32` | Monotonic ledger sequence number of last update |
| `active` | `bool` | Whether the alert is active |
| `pending_webhook_hash` | `Option<BytesN<32>>` | Pending webhook hash proposed via `propose_webhook`, not yet confirmed. `None` when no rotation is in progress. |

> **Expiry:** an alert may also carry an optional expiry ledger, stored separately from `AlertConfig` (see [`get_alert_expiry`](#get_alert_expiry)). Once the current ledger passes it, the alert is treated as inactive by the active getters and sync results even though its stored `active` flag is still `true`.

### `AlertInput`

Input record for [`batch_register_alert`](#batch_register_alert). Mirrors the arguments of `register_alert`.

| Field | Type | Description |
|---|---|---|
| `owner` | `Address` | Address that will own and control this alert |
| `target_contract` | `Address` | Contract address to watch |
| `label` | `String` | Human-readable name for the alert |
| `webhook_hash` | `BytesN<32>` | SHA-256 digest of the destination webhook URL |
| `rules` | `Vec<String>` | Serialized rule descriptors |
| `active_until_ledger` | `Option<u32>` | Optional ledger sequence after which the alert is treated as inactive. `None` means no expiry. |

---

## Webhook Hash Scheme

The `webhook_hash` field stores the **SHA-256 digest** of the destination webhook URL as 32 raw bytes (`BytesN<32>`). Because the type fixes the length, the contract no longer validates it (#214). The raw URL is never written on-chain, which prevents publicly exposing private endpoint addresses.

### Algorithm

| Property | Value |
|---|---|
| Hash function | SHA-256 |
| On-chain type | `BytesN<32>` (the raw digest, not its hex text) |
| Passing it in | `stellar contract invoke` takes the 64-character hex digest printed below; SDKs pass the 32 bytes (e.g. a Node `Buffer`) |
| Input | The raw webhook URL, UTF-8 encoded, no trailing newline |

### Computing the Hash

**Shell (openssl):**
```bash
echo -n "https://example.com/my-webhook" | openssl dgst -sha256
# SHA2-256(stdin)= 6b86b273ff34fce19d6b804eff5a3f5747ada4eaa22f1d49c01e52ddb7875b4b
```

**Shell (sha256sum):**
```bash
printf '%s' 'https://example.com/my-webhook' | sha256sum
# 6b86b273ff34fce19d6b804eff5a3f5747ada4eaa22f1d49c01e52ddb7875b4b  -
```

**JavaScript:**
```js
const hash = await crypto.subtle.digest(
  "SHA-256",
  new TextEncoder().encode("https://example.com/my-webhook"),
);
const hex = Array.from(new Uint8Array(hash))
  .map((b) => b.toString(16).padStart(2, "0"))
  .join("");
```

**Python:**
```python
import hashlib
url = "https://example.com/my-webhook"
hex_digest = hashlib.sha256(url.encode()).hexdigest()
```

**Rust:**
```rust
use sha2::{Digest, Sha256};
let hex_digest = format!("{:x}", Sha256::digest(b"https://example.com/my-webhook"));
```

### Verification

Off-chain watcher nodes store the original webhook URL locally and verify against the on-chain hash before firing a delivery. A mismatch indicates tampering or an out-of-date local config.

### Webhook Rotation

To rotate a webhook URL, two methods are available:
- **Two-phase rotation (recommended)**: Use `propose_webhook` followed by `confirm_webhook` to safely stage and test the new endpoint before activating it without downtime. See [ADR 0001: Two-Phase Webhook Rotation](adr/0001-two-phase-webhook-rotation.md) for the security rationale and threat analysis.
  A staged proposal expires `WEBHOOK_PROPOSAL_EXPIRY_LEDGERS` (120,960 ledgers, about 7 days) after `propose_webhook`; `confirm_webhook` then fails with `WebhookProposalExpired` (29) and the owner must re-propose or cancel.
- **Direct rotation**: Use `update_webhook` with the new SHA-256 digest for immediate cutover.

---

## Rule descriptor format

The `rules` field is a `Vec<String>` containing serialized rule descriptors. Each descriptor is a single string in the format `rule:<prefix>`, where `<prefix>` denotes the event or condition to watch for.

### Valid rule prefixes

| Prefix | Semantics |
|---|---|
| `rule:transfer` | Alert when the target contract emits a transfer-like action. |
| `rule:mint` | Alert when the target contract performs a mint or issuance event. |
| `rule:burn` | Alert when tokens are burned (SEP-41 `burn` / `burn_from`), reducing supply. |
| `rule:approve` | Alert when a holder sets or changes a spending allowance (SEP-41 `approve`). |
| `rule:clawback` | Alert when an issuer claws tokens back from a holder (SEP-41 `clawback`). |

### Parameterized rules

Any descriptor may carry a minimum-amount parameter: `rule:<base>:min=<amount>`, e.g. `rule:transfer:min=10000` ("transfers of at least 10,000 units"). `<amount>` must be a canonical non-negative decimal `i128` (digits only; no sign, no leading zeros except `0`; at most `i128::MAX`). Anything else, including `min=` with no value, other parameter names, or repeated parameters, is rejected with `InvalidRuleDescriptor`. Descriptors are limited to 64 bytes. The registry validates and stores the string verbatim; the off-chain watcher applies the threshold. The grammar is covered by unit tests in `rule_params.rs` and by the `validate_rule` fuzz target.

The alert registry stores these descriptors verbatim and validates that each entry uses a recognized prefix before accepting it. Off-chain watcher logic still interprets prefixes and applies the corresponding alert behavior.

---

## Functions

### `register_alert`

Registers a new alert configuration for a target contract address.

**Requires auth:** `owner`

**Validation:** Rule descriptors are checked against the known prefixes `rule:transfer`, `rule:mint`, `rule:burn`, `rule:approve` and `rule:clawback`. The call returns a `ContractError` instead of registering the alert if validation fails:

| Error | Code | Cause |
|---|---|---|
| `TooManyRules` | 8 | More than `MAX_RULES_PER_ALERT` (`VALID_RULE_DESCRIPTORS.len()`, currently 5) rules were supplied |
| `InvalidRuleDescriptor` | 9 | A rule is not one of the recognized descriptors listed above |
| `DuplicateRule` | 15 | The same rule descriptor appears more than once |

**Parameters**

| Name | Type | Description |
|---|---|---|
| `owner` | `Address` | Owner of the alert config |
| `target_contract` | `Address` | Contract address to watch |
| `label` | `String` | Human-readable label |
| `webhook_hash` | `BytesN<32>` | SHA-256 digest of the webhook URL |
| `rules` | `Vec<String>` | Rule descriptors |
| `active_until_ledger` | `Option<u32>` | Optional ledger sequence after which the alert is treated as inactive. `None` means no expiry. |

**Returns:** `u64` — the new config ID

**Expiry:** when `active_until_ledger` is `Some(ledger)`, the alert is treated as inactive by the active getters and sync results once the current ledger sequence is greater than `ledger`. The stored `active` flag is left untouched, so the record is not archived or removed — it simply stops being reported as active. This is distinct from storage TTL, which archives the record entirely.

---

### `update_alert`

Updates the rules, active status, and optional expiry of an existing alert. Only the original owner may call this.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to update |
| `rules` | `Vec<String>` | New rule descriptors |
| `active` | `bool` | New active status |
| `active_until_ledger` | `Option<u32>` | New expiry ledger, or `None` to clear any expiry |

**Returns:** `Result<(), ContractError>`

**Errors:** `AlertNotFound` if ID does not exist; `Unauthorized` if caller is not the owner; `AlertSuspended` if reactivating an admin-suspended alert; `Paused` while the contract is paused.

---

### `get_alert_expiry`

Returns the optional expiry ledger stored for an alert, or `None` if the alert has no expiry (or does not exist).

**Parameters**

| Name | Type | Description |
|---|---|---|
| `config_id` | `u64` | Alert config ID |

**Returns:** `Option<u32>` — the ledger sequence after which the alert is treated as inactive.

**Expiry semantics:** an alert is considered expired once the current ledger sequence is greater than its `active_until_ledger`. Expired alerts are reported as inactive (`false`) by [`get_alert_active`](#get_alert_active), excluded from [`get_active_alerts_for_contract`](#get_active_alerts_for_contract), and omitted from the sync results of [`get_alerts_modified_since`](#get_alerts_modified_since) and [`get_alerts_modified_since_ledger`](#get_alerts_modified_since_ledger). The underlying record and its `active` flag are not modified, so clearing the expiry (via `update_alert` with `active_until_ledger = None`) makes the alert active again if its stored flag is `true`.

### `set_alert_active`

Updates only the active status of an existing alert. Only the original owner
may call this. The alert's rules are preserved, making this the safe endpoint
for pausing or resuming a single alert.

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to update |
| `active` | `bool` | New active status |

**Returns:** nothing

**Errors:** Returns `ContractError::AlertNotFound` if ID does not exist; `ContractError::Unauthorized` if caller is not the owner.

---
### `__constructor`

Atomic constructor executed during deployment via `stellar contract deploy -- --admin <ADDRESS>`. Sets up the initial admin in the same transaction as deployment, closing the front-running window.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Address to assign as initial admin |

**Returns:** nothing

---
### `initialize`

*(Deprecated as of v0.2.0; scheduled for removal in v0.3.0.)* Initializes an optional admin for the contract. Retained only for contracts deployed before `__constructor` existed. Every contract deployed with `__constructor` is already initialized at deploy time, so on such deployments this call can only ever return `ContractError::AlreadyInitialized`. New deployments should not call this and should not list it as a "getting started" step.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Address to assign as admin |

**Returns:** nothing

---

### `transfer_admin`

Transfers admin authority to a new address. Requires current admin auth.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin address |
| `new_admin` | `Address` | Address to become the new admin |

**Returns:** nothing

---

### `get_admin`

Returns the current admin address.

---

### `set_guardian` / `get_guardian`

`set_guardian(admin, guardian)` designates an optional **pause-only guardian** — an on-call operator or monitoring bot that can trip the circuit-breaker without holding the full admin key (which can also upgrade the contract and change limits). The guardian may call `pause` but can never call `unpause`. Admin only.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin address |
| `guardian` | `Address` | Address to designate as the pause-only guardian |

**Returns:** nothing

**Errors:** `NotInitialized` if the contract has not been initialized; `Unauthorized` if caller is not the admin; `Paused` while the contract is paused.

**Events:** Emits `(Symbol("alert"), Symbol("guardian"))` with data `(admin: Address, guardian: Address)`.

`get_guardian() -> Option<Address>` returns the configured guardian, or `None` if none has been set.

### `get_configuration`

Returns the complete administrative configuration in one call: `admin`,
`paused`, the per-owner, per-contract, and global alert limits, and the
optional `watcher_registry` address, and the optional `pending_admin` address
of an unexpired pending admin transfer. This is the preferred read for
dashboards that display registry configuration.

**Returns:** `Result<Address, ContractError>`

**Errors:** `NotInitialized` if `initialize` has not been called.

---

### `get_pending_admin_transfer`

Returns `Option<Address>`: the address named by an unexpired pending admin
transfer proposal, or `None` if no transfer is pending or the proposal has
expired (see `ADMIN_TRANSFER_EXPIRY_LEDGERS`). Read-only; no auth required.

---

### `set_per_owner_alert_limit`

Sets a global per-owner limit on active alerts. A value of `0` disables the limit.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin address |
| `limit` | `u32` | New per-owner active alert limit |

**Returns:** nothing

---

### `get_per_owner_alert_limit`

Returns the configured per-owner active alert limit, or `0` if no limit is set.

**Returns:** `u32`

---

### `remove_alert_by_admin`

Removes any alert config by ID. Requires admin auth.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin address |
| `config_id` | `u64` | ID of the alert to remove |

**Returns:** nothing

---

### `deactivate_alert_by_admin`

Deactivates any alert by ID without deleting its record. Unlike `remove_alert_by_admin`, the alert config and its owner/contract indexes are left intact — only the `active` flag is cleared — so an admin can moderate a single problematic alert (spam, abuse report) while preserving its history. Admin only.

The alert is also **suspended** (`DataKey::AdminSuspended(id)`): the owner cannot reactivate it with `update_alert(..., active = true)` (`AlertSuspended`) until an admin calls `unlock_alert_by_admin`. Suspending an alert that is already suspended does nothing. The suspension stays with the alert if ownership is transferred and is cleared when the alert is removed (#202).

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin address |
| `config_id` | `u64` | ID of the alert to deactivate |

**Returns:** nothing

**Errors:** Returns `ContractError::AlertNotFound` if ID does not exist; `ContractError::Unauthorized` if caller is not the admin.

**Events:** Emits `(Symbol("alert"), Symbol("admin_off"))` with data `(id: u64, admin: Address)` when a suspension was applied.

---

### `unlock_alert_by_admin` / `is_alert_suspended`

`unlock_alert_by_admin(admin, config_id)` lifts an admin suspension so the owner can reactivate the alert. It does not reactivate the alert itself. Unlocking an alert that is not suspended does nothing. Admin only; blocked while paused.

**Errors:** `AlertNotFound`, `NotInitialized`, `Unauthorized` (not the admin), `Paused`.

**Events:** `(Symbol("alert"), Symbol("admin_on"))` with data `(id: u64, admin: Address)` when a suspension was lifted.

`is_alert_suspended(config_id) -> bool` reports whether an alert is currently suspended. It is public metadata: it is **not** subject to watcher-gating and can be called by anyone for any alert ID.
### `deactivate_all_alerts`

Deactivates every active alert owned by `caller` in one call, leaving the records and indexes in place (same effect as `update_alert(..., active = false)` on each). Expired or already-removed entries are skipped.

**Requires auth:** `caller`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Owner whose alerts are deactivated |

**Returns:** `Result<u32, ContractError>` — the number of alerts deactivated (`0` if none were active)

**Errors:** Returns `ContractError::Paused` while the contract is paused. (Before #204 it returned `0`, which was indistinguishable from an owner with no active alerts.)

**Events:** Emits `(Symbol("alert"), Symbol("bulk_off"))` with data `(caller: Address, count: u32)` when at least one alert was deactivated; no event when the count is `0`.

---
### `remove_alert`

Permanently removes an alert config. Only the original owner may call this.

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to remove |

**Returns:** nothing

**Errors:** Returns `ContractError::AlertNotFound` if ID does not exist; `ContractError::Unauthorized` if caller is not the owner.

**Expired alerts:** if the alert's record has expired (rather than been removed) but its ID is still in the caller's owner index, `remove_alert` removes that dangling index entry and releases the quota slot it held, instead of returning `AlertNotFound`. Any other caller still gets `AlertNotFound`.

---

### `prune_expired_alerts`

Removes IDs from `owner`'s index whose alert record no longer exists (expired instead of being removed), decrements the owner's live counter, and clears leftover `AlertActive` and pending-transfer entries. Callable by anyone with no auth, since it only removes entries that point at nothing. `register_alert` runs the same clean-up automatically when the owner is at the per-owner limit, so expired alerts can no longer lock an owner out.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `owner` | `Address` | Owner whose index is cleaned up |

**Returns:** `u32` — the number of dangling IDs removed in this pass

Each call examines at most `MAX_PRUNE_PER_CALL` (50) index entries, resuming from a stored `PruneCursor`, like `deactivate_all_alerts`. `get_prune_cursor(owner) -> Option<u32>` returns `Some(_)` while a scan is still in progress; call `prune_expired_alerts` again until it returns `None`.

**Events:** Emits `(Symbol("alert"), Symbol("pruned"))` with data `(owner: Address, count: u32, ids: Vec<u64>)` when at least one ID was removed.

**Contract side:** each alert's target is also stored under `AlertTarget(id)`, so pruning repairs the target's `ContractIndex` and `ContractLiveCount` too. Expired alerts therefore release their per-contract quota.

---

### `prune_contract_index`

Drops IDs from `target`'s contract index whose alert record no longer exists (expired) and recounts `ContractLiveCount`. Callable by anyone with no auth. Scans the whole contract index.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `target` | `Address` | Contract whose index is cleaned up |

**Returns:** `u32` — the number of dangling IDs removed

---

### `update_target_contract`

Moves an alert to watch a different contract: updates `target_contract`, migrates the alert ID from the old contract's index to the new one, and clears any pending ownership transfer. Only the owner may call this.

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to retarget |
| `new_target` | `Address` | Contract address to watch instead |

**Returns:** nothing

**Errors:** `AlertNotFound` if the ID does not exist; `Unauthorized` if the caller is not the owner; `ContractAlertLimitExceeded` if `new_target` is already at the per-contract alert limit (retargeting counts against the limit exactly like registering, #199; moving to the alert's current target is never blocked); `Paused` while the contract is paused.

**Events:** Emits `(Symbol("alert"), Symbol("retarget"))` with data `(id: u64, old_target: Address, new_target: Address)`.

---

### Alert ownership transfers

Ownership moves in two steps so nobody can be made the owner of alerts they did not agree to take (which would fill their per-owner quota and add webhooks they do not control to their alert list). The owner proposes, the recipient accepts. A proposal expires after `ALERT_TRANSFER_EXPIRY_LEDGERS` (120,960 ledgers, ≈ 7 days), and is cleared when the alert is removed or retargeted. `transfer_alert_ownership` was replaced by this flow in #201.

| Function | Auth | Effect | Errors |
|---|---|---|---|
| `propose_alert_transfer(caller, config_id, new_owner)` | `caller` (current owner) | Stores a `PendingAlertTransfer { new_owner, expires_at_ledger }`; replaces any earlier proposal. Ownership is unchanged. | `AlertNotFound`, `Unauthorized`, `InvalidTransferRecipient` (new owner is already the owner), `Paused` |
| `accept_alert_transfer(new_owner, config_id)` | `new_owner` (named recipient) | Moves the alert to `new_owner`: updates `owner`, migrates the owner index and live counter, clears the proposal. | `AlertNotFound`, `NoPendingTransfer`, `Unauthorized` (not the named recipient), `TransferExpired`, `OwnerAlertLimitExceeded` (recipient already at the per-owner limit, #200), `Paused` |
| `reject_alert_transfer(new_owner, config_id)` | `new_owner` (named recipient) | Clears the proposal; the alert stays with its owner. | `NoPendingTransfer`, `Unauthorized` |
| `cancel_alert_transfer(caller, config_id)` | `caller` (current owner) | Clears the proposal. | `AlertNotFound`, `Unauthorized`, `NoPendingTransfer` |
| `get_pending_alert_transfer(config_id)` | none | Public metadata, **not** subject to watcher-gating: anyone can read the proposed recipient and expiry for any alert ID. Returns `Option<PendingAlertTransfer>`. An expired proposal is still returned (compare `expires_at_ledger` with the current ledger); it can only be cancelled or replaced. | — |

A transfer can be accepted up to and including ledger `expires_at_ledger`. Accepting counts against the recipient's per-owner limit exactly like registering, so the limit is checked at acceptance (the recipient's count may change after the proposal).

**Events:** `(Symbol("alert"), Symbol("xfer_prop"))` with `(id, owner, new_owner, expires_at_ledger)` on propose; `(Symbol("alert"), Symbol("transfer"))` with `(id, old_owner, new_owner)` on accept; `(Symbol("alert"), Symbol("xfer_rej"))` with `(id, new_owner)` on reject; `(Symbol("alert"), Symbol("xfer_can"))` with `(id, owner)` on cancel.

---

### `batch_register_alert`

Registers multiple alert configs in a single call. Each input is validated and authorized exactly as `register_alert` would, and each successful registration emits the same `alert.register` event. Since Soroban invocations are atomic, if any input fails validation or authorization the entire batch — including any alerts already registered earlier in the same call — is rolled back.

**Requires auth:** the `owner` of each input in `inputs`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `inputs` | `Vec<AlertInput>` | Alert records to register |

**Returns:** `Vec<u64>` — the new alerts' IDs, in the same order as `inputs`

**Errors:** Returns the same errors as `register_alert` for the failing item.

---

### `batch_remove_alert`

Removes multiple alert configs owned by `caller` in a single call. Each ID is validated and authorized exactly as `remove_alert` would, and each successful removal emits the same `alert.remove` event. Since Soroban invocations are atomic, if any ID does not exist or is not owned by `caller`, the entire batch is rolled back.

**Requires auth:** `caller`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must own every alert in `config_ids` |
| `config_ids` | `Vec<u64>` | IDs of the alerts to remove |

**Returns:** nothing

**Errors:** Returns `ContractError::AlertNotFound` if any ID does not exist; `ContractError::Unauthorized` if caller does not own every alert in `config_ids`.

---

### `get_alert`

Retrieves a single alert config by ID.

If a `WatcherRegistry` is configured (via `set_watcher_registry`), `querier` must be a registered watcher or the call returns `ContractError::NotAWatcher`.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `querier` | `Address` | Address performing the query (checked against watcher registry if configured) |
| `config_id` | `u64` | Alert config ID |

**Returns:** `Result<Option<AlertConfig>, ContractError>` — `Ok(Some(config))` if found, `Ok(None)` otherwise.

---

### `get_alerts_by_ids`

Returns alert configurations for a supplied list of IDs in input order,
omitting IDs whose records no longer exist or have expired. Watcher
authorization, when configured, is checked once for the entire batch. Use this
when IDs came from `get_alert_ids_by_owner` or event processing instead of
calling `get_alert` separately for every ID.

---

### `get_alert_active`

Cheap read-only function that returns just the `active` bool for a given alert ID, avoiding the cost of deserializing the full [`AlertConfig`](#alertconfig). The active flag is stored under a dedicated storage key (`DataKey::AlertActive`).

If a `WatcherRegistry` is configured (via `set_watcher_registry`), `querier` must be a registered watcher or the call returns `ContractError::NotAWatcher`.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `querier` | `Address` | Address performing the query (checked against watcher registry if configured) |
| `config_id` | `u64` | Alert config ID |

**Returns:** `Result<Option<bool>, ContractError>` — `Ok(Some(true))` if active, `Ok(Some(false))` if inactive, `Ok(None)` if the alert does not exist.

---

### `get_alerts_for_contract`

Returns all alert configs registered for a given target contract, including both active and inactive entries. Use [`get_active_alerts_for_contract`](#get_active_alerts_for_contract) to filter to active-only.

If a `WatcherRegistry` is configured (via `set_watcher_registry`), `querier` must be a registered watcher or the call returns `ContractError::NotAWatcher`.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `querier` | `Address` | Address performing the query (checked against watcher registry if configured) |
| `target_contract` | `Address` | Contract address to query |

**Returns:** `Result<Vec<(u64, AlertConfig)>, ContractError>` — `Ok(vec)` on success, may be empty.

---

### `get_active_alerts_for_contract`

Returns only the active alert configs (`active == true`) registered for a given target contract. Inactive alerts are excluded.

If a `WatcherRegistry` is configured (via `set_watcher_registry`), `querier` must be a registered watcher or the call returns `ContractError::NotAWatcher`.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `querier` | `Address` | Address performing the query (checked against watcher registry if configured) |
| `target_contract` | `Address` | Contract address to query |

**Returns:** `Result<Vec<(u64, AlertConfig)>, ContractError>` — `Ok(vec)` on success, may be empty.

---

### `get_alerts_by_owner`

Returns all alert configs owned by a given address.

If a `WatcherRegistry` is configured, `querier` must be a registered watcher or the call returns `ContractError::NotAWatcher`.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `querier` | `Address` | Address performing the query |
| `owner` | `Address` | Owner address to query |

**Returns:** `Result<Vec<(u64, AlertConfig)>, ContractError>` — `Ok(vec)` on success, may be empty.

---

### `get_alert_ids_by_owner`

Returns the **live** alert IDs owned by a given address. Use this instead of `get_alerts_by_owner` when only the IDs are needed (e.g. an existence check or count), avoiding the cost of deserializing every full `AlertConfig`.

Only IDs whose record still exists are returned. The underlying `OwnerIndex` keeps dangling IDs until a prune runs, and returning them here would make an existence check or a count lie (see #383). For the number of live alerts, prefer `get_non_removed_alert_count`, which reads a counter instead of filtering this list.

Unlike `get_alerts_by_owner`, this is **not** subject to watcher-gating, since it exposes no alert content.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `owner` | `Address` | Owner address to query |

**Returns:** `Vec<u64>` — may be empty.

---

### `update_label`

Updates only the label of an existing alert, leaving `rules` and `webhook_hash` unchanged. Use this to rename an alert without touching its configuration.

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to update |
| `label` | `String` | New human-readable label (max 128 bytes) |

**Returns:** `Result<(), ContractError>`

**Errors:** `AlertNotFound` if ID does not exist; `Unauthorized` if caller is not the owner.

**Panics:** if `label` exceeds 128 bytes.

---

### `update_webhook`

Updates the webhook hash for an existing alert. Use this to rotate webhook URLs without re-registering. Only the original owner may call this.

The update takes effect immediately and **discards any rotation staged by `propose_webhook`** (`pending_webhook_hash` is reset to `None`), so a later `confirm_webhook` returns `NoPendingWebhook` instead of reverting this update.

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to update |
| `webhook_hash` | `BytesN<32>` | SHA-256 digest of the new webhook URL |

**Returns:** `Result<(), ContractError>`

**Errors:** `AlertNotFound` if ID does not exist; `Unauthorized` if caller is not the owner.

---

### `propose_webhook`

Step 1 of the two-step webhook rotation flow (see [ADR 0001](adr/0001-two-phase-webhook-rotation.md)). Stores the new hash in `pending_webhook_hash` without replacing the live `webhook_hash`. The old webhook remains active until the owner calls `confirm_webhook`, eliminating the window where the old webhook is deactivated before the new one is confirmed.

Calling `propose_webhook` again before confirming overwrites the previous pending hash, allowing the owner to correct a mistake.

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to update |
| `new_webhook_hash` | `BytesN<32>` | SHA-256 digest of the new webhook URL |

**Returns:** `Result<(), ContractError>`

**Errors:** `AlertNotFound` if ID does not exist; `Unauthorized` if caller is not the owner.

**Events:** Emits `(Symbol("alert"), Symbol("wh_prop"))` with data `(id: u64, caller: Address)`.

---

### `confirm_webhook`

Step 2 of the two-step webhook rotation flow. Promotes `pending_webhook_hash` to `webhook_hash` and clears the pending field. Returns `NoPendingWebhook` if no rotation is in progress.

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to confirm rotation for |

**Returns:** `Result<(), ContractError>`

**Errors:** `AlertNotFound` if ID does not exist; `Unauthorized` if caller is not the owner; `NoPendingWebhook` if no pending hash exists.

**Events:** Emits `(Symbol("alert"), Symbol("wh_conf"))` with data `(id: u64, caller: Address)`.

---

### `renew_alert_ttl`

Extends the TTL of an alert and its index entries without modifying any data. This is the recommended way to keep an alert alive without changing `updated_at` or `updated_ledger` (which would cause it to appear in incremental-sync results via `get_alerts_modified_since` or `get_alerts_modified_since_ledger`).

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to renew |

**Returns:** `Result<(), ContractError>`

**Errors:** `AlertNotFound` if ID does not exist; `Unauthorized` if caller is not the owner.

`renew_alert_ttl` always extends to `DEFAULT_TTL`. To renew for longer, use `renew_alert_ttl_for(caller, config_id, ttl)`: same auth and effects, but it extends to `ttl` ledgers, which must be between `DEFAULT_TTL` and `MAX_TTL` (otherwise `InvalidTtl`).

---

### `get_contract_alerts_paginated`

Returns a page of alert configs registered for a given target contract.

If a `WatcherRegistry` is configured, `querier` must be a registered watcher or the call returns `ContractError::NotAWatcher`.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `querier` | `Address` | Address performing the query |
| `target_contract` | `Address` | Contract address to query |
| `offset` | `u32` | Number of results to skip |
| `limit` | `u32` | Maximum number of results to return |

**Returns:** `Result<Vec<(u64, AlertConfig)>, ContractError>` — may be empty.

---

### `get_active_contract_alerts_paged`

Returns a page of **active** alert configs registered for a given target contract — the bounded replacement for `get_active_alerts_for_contract` (#384). Only the `[offset, offset + limit)` window of the contract index is walked, so the cost does not grow with the whole registry.

`offset` and `limit` count index slots rather than matches, so paging stays deterministic even when most slots are inactive: advance `offset` by `limit` until the returned vec is shorter than `limit`. `limit` is clamped to the maximum page size.

If a `WatcherRegistry` is configured, `querier` must be a registered watcher or the call returns `ContractError::NotAWatcher`.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `querier` | `Address` | Address performing the query |
| `target_contract` | `Address` | Contract address to query |
| `offset` | `u32` | Number of index slots to skip |
| `limit` | `u32` | Maximum number of index slots to scan (clamped) |

**Returns:** `Result<Vec<(u64, AlertConfig)>, ContractError>` — may be empty.

---

### `get_alerts_by_owner_paginated`

Returns a page of alert configs owned by a given address.

If a `WatcherRegistry` is configured, `querier` must be a registered watcher or the call returns `ContractError::NotAWatcher`.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `querier` | `Address` | Address performing the query |
| `owner` | `Address` | Owner address to query |
| `offset` | `u32` | Number of results to skip |
| `limit` | `u32` | Maximum number of results to return |

**Returns:** `Result<Vec<(u64, AlertConfig)>, ContractError>` — may be empty.

---

### `get_alerts_modified_since`

Returns all alert configs whose `updated_at` timestamp is greater than or equal to `since`.

Enables incremental sync for watcher nodes by passing the ledger timestamp of the last sync.

> **Note:** Because multiple ledgers can close within the same timestamp second, timestamp-based synchronization may produce duplicates (with `since = T`) or miss changes across ledgers closed in the same second (with `since = T + 1`). For strictly monotonic, unambiguous sync, use [`get_alerts_modified_since_ledger`](#get_alerts_modified_since_ledger).

**Parameters**

| Name | Type | Description |
|---|---|---|
| `since` | `u64` | Ledger timestamp (inclusive lower bound, pass `0` for all) |
| `offset` | `u32` | Number of alert IDs to skip from the start of ID space |
| `limit` | `u32` | Maximum number of IDs to scan |

**Returns:** `Vec<(u64, AlertConfig)>` — live alerts and inactive removal tombstones
matching `updated_at >= since` among the IDs in `[offset, offset + limit)`.
Continue paging by increasing `offset` by `limit` until it reaches
`get_alert_count()`; a short or empty page does not indicate that later IDs
cannot match. A tombstone has `removed == true` (and `active == false`) and
identifies an alert that must be purged from the watcher's local state. An
entry with `active == false` and `removed == false` is a paused alert: keep it
and mark it inactive. Always branch on `removed`, never on `active` alone.
All paginated alert queries cap `limit` at `MAX_PAGE_SIZE` (100 IDs), so
passing a larger value is safe and does not create an unbounded RPC simulation.

**Returns:** `Vec<(u64, AlertConfig)>` — live alerts matching `updated_at >= since`.

---

### `get_min_sync_ledger`

Returns the oldest ledger sequence from which incremental sync is guaranteed to include every removal tombstone (`current_ledger - TOMBSTONE_TTL`, saturating at 0). If a client's `since_ledger` cursor is below this value it must fall back to a full resync.

**Returns:** `u32`

---

### `get_alerts_modified_since_ledger`

Returns all alert configs whose `updated_ledger` sequence number is greater than or equal to `since_ledger`.

Provides unambiguous **incremental sync** for watcher nodes keyed on monotonic ledger sequence numbers rather than timestamps. Because several ledgers can share the same close-time second, sequence numbers eliminate duplicate delivery and missed updates.

#### Recommended Sync Loop

1. Initialize `cursor_ledger = 0` (or the last-synced ledger sequence).
2. On each polling cycle:
   - Call `get_alerts_modified_since_ledger(since_ledger = cursor_ledger, offset, limit)`.
   - Paginate by advancing `offset += limit` until `offset` reaches `get_alert_count()`; empty or short pages can occur before the end of the ID space.
   - For each returned alert, update local state and record the highest ledger sequence seen: `max_ledger = max(max_ledger, alert.updated_ledger)`.
   - After finishing the scan, update the cursor for the next cycle: `cursor_ledger = max_ledger + 1` (or `current_ledger + 1` if no alerts were returned).

**Parameters**

| Name | Type | Description |
|---|---|---|
| `since_ledger` | `u32` | Ledger sequence number (inclusive lower bound, pass `0` for all) |
| `offset` | `u32` | Number of alert IDs to skip from the start of ID space |
| `limit` | `u32` | Maximum number of IDs to scan |

**Returns:** `Vec<(u64, AlertConfig)>` — live alerts matching `updated_ledger >= since_ledger`.

---

### `get_alert_count`

Returns the total number of alerts ever registered.

> **Monotonic counter:** This value only ever increases. Removing an alert (via `remove_alert` or `remove_alert_by_admin`) does not decrement the counter. It reflects the cumulative count of all registrations since contract initialization, not the number of currently active alerts.

**Parameters:** none

**Returns:** `u64`

---

### `set_watcher_registry`

Configures the `WatcherRegistry` contract address used for optional watcher-gating on read queries. Once set, `get_alerts_for_contract`, `get_alerts_by_owner`, and their paginated variants will cross-call `WatcherRegistry::is_watcher_authorized` before returning data. Any address configured here — including a zero/default `Address` — is treated as a real registry and will be cross-called; use `clear_watcher_registry` to disable gating. Admin only.

`watcher_registry` is probed with a read-only `is_watcher_authorized` call before being persisted. A misconfigured address (not a contract, or a contract that doesn't implement the `WatcherRegistry` interface) is rejected here with `InvalidWatcherRegistry` instead of surfacing later as a panic the next time a gated query runs.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin address |
| `watcher_registry` | `Address` | Address of the deployed `WatcherRegistry` contract |

**Returns:** nothing

**Errors:** `InvalidWatcherRegistry` if `watcher_registry` does not respond to the `WatcherRegistry` interface.

---

### `clear_watcher_registry`

Clears the configured `WatcherRegistry` contract address, disabling watcher-gating on the read queries. After this call, `get_alerts_for_contract`, `get_alerts_by_owner`, and their paginated variants no longer cross-call `WatcherRegistry` and behave as if gating had never been configured. Call `set_watcher_registry` again to re-enable gating. Admin only.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin address |

**Returns:** nothing

**Errors:** Returns `ContractError::NotInitialized` if the contract has not been initialized; `ContractError::Unauthorized` if caller is not the admin.

---

### `get_watcher_registry`

Returns the configured `WatcherRegistry` contract address, or `None` if watcher-gating has not been enabled.

**Returns:** `Option<Address>`

---

### `is_watcher_gating_enabled`

Convenience boolean getter returning `true` if watcher-gating is currently active (`WATCHREG` is configured), or `false` otherwise.

**Parameters:** none

**Returns:** `bool`

---

## Pause

`pause(caller)` freezes the registry during an incident: **every state-mutating entry point returns `ContractError::Paused`** until `unpause(admin)`. Reads keep working.

**Authorization:** `pause` may be called by the admin **or** by the configured guardian (see [`set_guardian`](#set_guardian--get_guardian)). `unpause` may only be called by the admin — the guardian can trip the circuit-breaker but never release it. Calling `pause` when already paused returns `ContractError::Paused`.

The only exemptions are deliberate (#203):

| Exempt entry point | Why |
|---|---|
| `pause`, `unpause` | The circuit-breaker itself must stay operable. |
| `initialize` | One-time admin bootstrap. |
| `upgrade` | Lets a hot-fix be deployed while paused. |

This includes admin moderation (`deactivate_alert_by_admin`, `unlock_alert_by_admin`, `remove_alert_by_admin`), limit and watcher-registry configuration, alert transfers (propose, accept, reject, cancel), `batch_remove_alert` and `prune_expired_alerts`. `deactivate_all_alerts` currently returns `0` instead of an error while paused (tracked separately in #204).

---

## Errors

| Variant | Code | Description |
|---|---|---|
| `Unauthorized` | 1 | Caller is not the owner or admin |
| `AlertNotFound` | 2 | No alert exists for the given ID |
| `AlreadyInitialized` | 3 | `initialize` was called more than once |
| `NotInitialized` | 4 | Admin function called before `initialize` |
| `NotAWatcher` | 5 | Watcher-gating is enabled and `querier` is not a registered watcher |
| `InvalidWebhookHash` | 6 | `register_alert`, `update_webhook` or `propose_webhook` called with the all-zero webhook hash (never the SHA-256 of a URL). Wrong lengths cannot occur: hashes are `BytesN<32>`. |
| `LabelTooLong` | 7 | `label` exceeds 128 bytes |
| `TooManyRules` | 8 | `rules` exceeds `MAX_RULES_PER_ALERT` (`VALID_RULE_DESCRIPTORS.len()`, currently 2) |
| `InvalidRuleDescriptor` | 9 | A rule is not a recognised descriptor (`rule:transfer`, `rule:mint`) |
| `OwnerAlertLimitExceeded` | 10 | Owner is at the configured per-owner active alert limit |
| `DuplicateAlertId` | 11 | Internal invariant violation — an ID was already present in an index |
| `NoPendingWebhook` | 12 | `confirm_webhook` called but no rotation is in progress |
| `GlobalAlertLimitExceeded` | 13 | `register_alert` called after the global alert-count ceiling has been reached |
| `ContractAlertLimitExceeded` | 14 | `register_alert` called when the target contract is already at its per-contract alert limit |
| `DuplicateRule` | 15 | The same rule descriptor appears more than once in `rules` |
| `NoPendingTransfer` | 16 | `accept_alert_transfer`, `reject_alert_transfer`, `cancel_alert_transfer`, `accept_admin_transfer`, or `cancel_admin_transfer` called with no transfer pending |
| `InvalidWatcherRegistry` | 17 | `set_watcher_registry` given an address that doesn't implement the `WatcherRegistry` interface |
| `Paused` | 18 | A state-mutating call was made while the contract is paused |
| `TransferExpired` | 19 | `accept_alert_transfer` called after the proposal's `expires_at_ledger` |
| `InvalidTransferRecipient` | 20 | `propose_alert_transfer` named the current owner as the recipient |
| `AlertSuspended` | 21 | `update_alert` tried to reactivate an alert suspended by `deactivate_alert_by_admin` |
| `EmptyLabel` | 22 | `label` is empty |
| `EmptyRules` | 25 | `register_alert`, `update_alert` or `batch_register_alert` called with an empty `rules` list |
| `NoopWebhookRotation` | 23 | `propose_webhook` called with a hash that is already live or already pending |
| `InvalidTtl` | 26 | `bump_alert` called with a TTL below `DEFAULT_TTL`, or `renew_alert_ttl_for` called with a TTL outside `DEFAULT_TTL..=MAX_TTL` |
| `AdminTransferExpired` | 24 | `accept_admin_transfer` called after the proposal's `ADMIN_TRANSFER_EXPIRY_LEDGERS` (120,960 ledgers) have elapsed |

---

## Storage

- Alert configs are stored in **persistent storage** under `DataKey::Alert(id)`.
- Owner and contract indexes are stored in **persistent storage** under `DataKey::OwnerIndex` and `DataKey::ContractIndex`.
- The auto-incrementing ID counter is stored in **instance storage** under `NEXT_ID`.
- The optional admin address is stored in **instance storage** under `ADMIN`.
- The optional per-owner alert limit is stored in **instance storage** under `LIMIT`.
- The optional `WatcherRegistry` contract address is stored in **instance storage** under `WATCHREG`.
- The optional pause-only guardian address is stored in **instance storage** under `GUARDIAN`.
- Optional alert expiry ledgers are stored in **persistent storage** under a dedicated `DataKey::AlertExpiry(id)` key, so `AlertConfig` does not need to be re-encoded when the feature is unused.

---

## Re-entrancy and cross-contract safety

This contract is safe to call from other Soroban contracts. Soroban executes contract calls atomically and does not allow classic callback-style re-entrancy into the same contract within the same transaction.

All state-mutating functions in `AlertRegistry` first enforce authorization with `require_auth()` and then perform local storage updates. There are no external callbacks or indirect contract calls during state mutation, so cross-contract invocation cannot introduce re-entrancy vulnerabilities.
