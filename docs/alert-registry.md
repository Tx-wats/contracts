# Alert Registry — Function Reference

Contract that stores alert configurations on-chain, keyed by contract address.

---

## Data Types

### `AlertConfig`

| Field | Type | Description |
|---|---|---|
| `label` | `String` | Human-readable name for the alert |
| `webhook_hash` | `String` | SHA-256 hex digest of the webhook URL (privacy-preserving; see [Webhook Hash Scheme](#webhook-hash-scheme) below) |
| `rules` | `Vec<String>` | Serialized rule descriptors |
| `owner` | `Address` | Address that owns this config |
| `target_contract` | `Address` | Contract being watched |
| `created_at` | `u64` | Ledger timestamp at creation |
| `updated_at` | `u64` | Ledger timestamp of last update |
| `active` | `bool` | Whether the alert is active |
| `pending_webhook_hash` | `Option<String>` | Pending webhook hash proposed via `propose_webhook`, not yet confirmed. `None` when no rotation is in progress. |

### `AlertInput`

Input record for [`batch_register_alert`](#batch_register_alert). Mirrors the arguments of `register_alert`.

| Field | Type | Description |
|---|---|---|
| `owner` | `Address` | Address that will own and control this alert |
| `target_contract` | `Address` | Contract address to watch |
| `label` | `String` | Human-readable name for the alert |
| `webhook_hash` | `String` | SHA-256 hex digest of the destination webhook URL |
| `rules` | `Vec<String>` | Serialized rule descriptors |

---

## Webhook Hash Scheme

The `webhook_hash` field stores a **SHA-256 hex digest** of the destination webhook URL. The raw URL is never written on-chain, which prevents publicly exposing private endpoint addresses.

### Algorithm

| Property | Value |
|---|---|
| Hash function | SHA-256 |
| Encoding | Lowercase hex string (64 characters) |
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

To rotate a webhook URL, use `update_webhook` with the new SHA-256 hex digest and update your local watcher config to match.

---

## Rule descriptor format

The `rules` field is a `Vec<String>` containing serialized rule descriptors. Each descriptor is a single string in the format `rule:<prefix>`, where `<prefix>` denotes the event or condition to watch for.

### Valid rule prefixes

| Prefix | Semantics |
|---|---|
| `rule:transfer` | Alert when the target contract emits a transfer-like action. |
| `rule:mint` | Alert when the target contract performs a mint or issuance event. |

The alert registry stores these descriptors verbatim and validates that each entry uses a recognized prefix before accepting it. Off-chain watcher logic still interprets prefixes and applies the corresponding alert behavior.

---

## Functions

### `register_alert`

Registers a new alert configuration for a target contract address.

**Requires auth:** `owner`

**Validation:** Rule descriptors are checked against the known prefixes `rule:transfer` and `rule:mint`, and the contract panics if any rule is not recognized.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `owner` | `Address` | Owner of the alert config |
| `target_contract` | `Address` | Contract address to watch |
| `label` | `String` | Human-readable label |
| `webhook_hash` | `String` | SHA-256 hex digest of the webhook URL |
| `rules` | `Vec<String>` | Rule descriptors |

**Returns:** `u64` — the new config ID

---

### `update_alert`

Updates the rules and active status of an existing alert. Only the original owner may call this.

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to update |
| `rules` | `Vec<String>` | New rule descriptors |
| `active` | `bool` | New active status |

**Returns:** nothing

**Errors:** Returns `ContractError::AlertNotFound` if ID does not exist; `ContractError::Unauthorized` if caller is not the owner.

---
### `initialize`

Initializes an optional admin for the contract. Can only be called once.

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

**Returns:** `Address`

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

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin address |
| `config_id` | `u64` | ID of the alert to deactivate |

**Returns:** nothing

**Errors:** Returns `ContractError::AlertNotFound` if ID does not exist; `ContractError::Unauthorized` if caller is not the admin.

**Events:** Emits `(Symbol("alert"), Symbol("admin_off"))` with data `(id: u64, admin: Address)`.

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

---

### `transfer_alert_ownership`

Transfers ownership of an alert to a new address. Updates the `owner` field of the alert config and migrates the alert ID from the old owner's `OwnerIndex` to the new owner's. Only the current owner may call this.

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the current alert owner |
| `config_id` | `u64` | ID of the alert to transfer |
| `new_owner` | `Address` | Address to become the new owner |

**Returns:** nothing

**Errors:** Returns `ContractError::AlertNotFound` if ID does not exist; `ContractError::Unauthorized` if caller is not the owner.

**Events:** Emits `(Symbol("alert"), Symbol("transfer"))` with data `(id: u64, old_owner: Address, new_owner: Address)`.

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

**Parameters**

| Name | Type | Description |
|---|---|---|
| `config_id` | `u64` | Alert config ID |

**Returns:** `Option<AlertConfig>` — `Some(config)` if found, `None` otherwise.

---

### `get_alert_active`

Cheap read-only function that returns just the `active` bool for a given alert ID, avoiding the cost of deserializing the full [`AlertConfig`](#alertconfig). The active flag is stored under a dedicated storage key (`DataKey::AlertActive`).

**Parameters**

| Name | Type | Description |
|---|---|---|
| `config_id` | `u64` | Alert config ID |

**Returns:** `Option<bool>` — `Some(true)` if active, `Some(false)` if inactive, `None` if the alert does not exist.

---

### `get_alerts_for_contract`

Returns all alert configs registered for a given target contract, including both active and inactive entries. Use [`get_active_alerts_for_contract`](#get_active_alerts_for_contract) to filter to active-only.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `target_contract` | `Address` | Contract address to query |

**Returns:** `Vec<AlertConfig>` — may be empty.

---

### `get_active_alerts_for_contract`

Returns only the active alert configs (`active == true`) registered for a given target contract. Inactive alerts are excluded.

If a `WatcherRegistry` is configured (via `set_watcher_registry`), `querier` must be a registered watcher or the call returns `ContractError::NotAWatcher`.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `querier` | `Address` | Address performing the query (checked against watcher registry if configured) |
| `target_contract` | `Address` | Contract address to query |

**Returns:** `Result<Vec<AlertConfig>, ContractError>` — `Ok(vec)` on success, may be empty.

---

### `get_alerts_by_owner`

Returns all alert configs owned by a given address.

If a `WatcherRegistry` is configured, `querier` must be a registered watcher or the call returns `ContractError::NotAWatcher`.

**Parameters**

| Name | Type | Description |
|---|---|---|
| `querier` | `Address` | Address performing the query |
| `owner` | `Address` | Owner address to query |

**Returns:** `Result<Vec<AlertConfig>, ContractError>` — `Ok(vec)` on success, may be empty.

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

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to update |
| `webhook_hash` | `String` | New hashed webhook URL |

**Returns:** `Result<(), ContractError>`

**Errors:** `AlertNotFound` if ID does not exist; `Unauthorized` if caller is not the owner.

---

### `propose_webhook`

Step 1 of the two-step webhook rotation flow. Stores the new hash in `pending_webhook_hash` without replacing the live `webhook_hash`. The old webhook remains active until the owner calls `confirm_webhook`, eliminating the window where the old webhook is deactivated before the new one is confirmed.

Calling `propose_webhook` again before confirming overwrites the previous pending hash, allowing the owner to correct a mistake.

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to update |
| `new_webhook_hash` | `String` | SHA-256 hex digest of the new webhook URL |

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

Extends the TTL of an alert and its index entries without modifying any data. This is the recommended way to keep an alert alive without changing `updated_at` (which would cause it to appear in incremental-sync results via `get_alerts_modified_since`).

**Requires auth:** `caller` (must match `owner` of the config)

**Parameters**

| Name | Type | Description |
|---|---|---|
| `caller` | `Address` | Must be the alert owner |
| `config_id` | `u64` | ID of the alert to renew |

**Returns:** `Result<(), ContractError>`

**Errors:** `AlertNotFound` if ID does not exist; `Unauthorized` if caller is not the owner.

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

**Returns:** `Result<Vec<AlertConfig>, ContractError>` — may be empty.

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

**Returns:** `Result<Vec<AlertConfig>, ContractError>` — may be empty.

---

### `get_alert_count`

Returns the total number of alerts ever registered.

> **Monotonic counter:** This value only ever increases. Removing an alert (via `remove_alert` or `remove_alert_by_admin`) does not decrement the counter. It reflects the cumulative count of all registrations since contract initialization, not the number of currently active alerts.

**Parameters:** none

**Returns:** `u64`

---

### `set_watcher_registry`

Configures the `WatcherRegistry` contract address used for optional watcher-gating on read queries. Once set, `get_alerts_for_contract`, `get_alerts_by_owner`, and their paginated variants will cross-call `WatcherRegistry::is_watcher_authorized` before returning data. Any address configured here — including a zero/default `Address` — is treated as a real registry and will be cross-called; use `clear_watcher_registry` to disable gating. Admin only.

**Requires auth:** `admin`

**Parameters**

| Name | Type | Description |
|---|---|---|
| `admin` | `Address` | Current admin address |
| `watcher_registry` | `Address` | Address of the deployed `WatcherRegistry` contract |

**Returns:** nothing

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

## Errors

| Variant | Code | Description |
|---|---|---|
| `Unauthorized` | 1 | Caller is not the owner or admin |
| `AlertNotFound` | 2 | No alert exists for the given ID |
| `AlreadyInitialized` | 3 | `initialize` was called more than once |
| `NotInitialized` | 4 | Admin function called before `initialize` |
| `NoPendingWebhook` | 5 | `confirm_webhook` called but no rotation is in progress |

---

## Storage

- Alert configs are stored in **persistent storage** under `DataKey::Alert(id)`.
- Owner and contract indexes are stored in **persistent storage** under `DataKey::OwnerIndex` and `DataKey::ContractIndex`.
- The auto-incrementing ID counter is stored in **instance storage** under `NEXT_ID`.
- The optional admin address is stored in **instance storage** under `ADMIN`.
- The optional per-owner alert limit is stored in **instance storage** under `LIMIT`.
- The optional `WatcherRegistry` contract address is stored in **instance storage** under `WATCHREG`.

---

## Re-entrancy and cross-contract safety

This contract is safe to call from other Soroban contracts. Soroban executes contract calls atomically and does not allow classic callback-style re-entrancy into the same contract within the same transaction.

All state-mutating functions in `AlertRegistry` first enforce authorization with `require_auth()` and then perform local storage updates. There are no external callbacks or indirect contract calls during state mutation, so cross-contract invocation cannot introduce re-entrancy vulnerabilities.
