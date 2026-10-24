# Threat Model: Alert Registry System

## Overview

The `AlertRegistry` contract manages on-chain alert subscriptions for monitored Stellar smart contracts. It stores alert configurations (target contract addresses, rule descriptors, human-readable labels, owner addresses, active flags, and SHA-256 hashed webhook destination endpoints).

The contract supports:
- **Owner-authenticated alert lifecycle management**: Creation, rule/label updates, status toggling, two-phase webhook hash rotation, TTL renewal, and deletion.
- **Admin-controlled system parameters**: Initialization, administrative handover, per-owner active alert limits, administrative alert deletion, and configuration of an optional `WatcherRegistry` contract for read access gating.
- **Optional timelock governance**: A configurable `TimelockDelay` that gates WASM upgrades and admin transfers behind a `propose`/`execute`/`cancel` flow.
- **Read-access queries**: Direct lookup by ID, active status queries, queries by target contract or owner, paginated query variants, and optional cross-contract watcher-gating.

---

## Assets

| Asset | Description |
|---|---|
| **Alert Configurations** | The on-chain `AlertConfig` records (target contracts, rules, labels, active status, and owner associations) stored under `DataKey::Alert(id)`. |
| **Webhook Endpoints & Hashes** | SHA-256 hex digests (`webhook_hash` and staged `pending_webhook_hash`) that off-chain watcher nodes use to authenticate destination webhook URLs. |
| **Owner & Contract Indices** | The lookup indexes mapping owners to alert IDs (`DataKey::OwnerIndex`) and watched contracts to alert IDs (`DataKey::ContractIndex`). |
| **Active Status Index** | Fast-path boolean lookup table stored under `DataKey::AlertActive(id)`. |
| **Monotonic ID Counter** | Global auto-incrementing identifier (`instance_key::NEXT_ID`, instance storage) ensuring unique alert IDs. |
| **Admin Authority** | Privileged control over global operational limits (`LIMIT`), administrative removal (`remove_alert_by_admin`), admin transfer (`transfer_admin`), watcher registry linking (`WATCHREG`), and the timelock configuration (`TimelockDelay`). |
| **Contract WASM / Upgrade Authority** | The deployed contract code, replaceable only through the timelocked `upgrade` path when a timelock is configured. |
| **Pending Timelock Proposals** | Staged `Upgrade` and `TransferAdmin` proposals awaiting execution after the configured delay elapses. |
| **Storage State & TTL** | Persistent storage entries kept alive through automatic write-time renewal, owner-authenticated `renew_alert_ttl`, and permissionless `bump_alert`. |

---

## Trust Assumptions

- **Admin and Owner Keypair Security**: The private keys of the contract admin and alert owners are assumed to be uncompromised and managed securely.
- **Stellar Protocol Integrity**: Cryptographic signature validation and authentication via Soroban's `Address::require_auth()` are enforced correctly by the underlying Stellar network.
- **Atomic Execution & Re-entrancy Model**: Soroban executes contract calls atomically. State changes in `AlertRegistry` are completed synchronously without external re-entrant callbacks during mutation.
- **SHA-256 Cryptographic Preimage Resistance**: SHA-256 is computationally infeasible to invert or collide, preventing attackers from forging raw URLs that produce matching hashes or deducing private URLs from on-chain hashes.
- **WatcherRegistry Trust (when configured)**: If a `WatcherRegistry` address is configured via `set_watcher_registry`, `AlertRegistry` trusts `WatcherRegistryClient::is_watcher_authorized` to accurately report whether a querying address is an authorized watcher node.
- **Off-Chain Watcher Node Fidelity**: Watcher nodes are assumed to faithfully read on-chain alert configs, match event triggers against registered rule descriptors, and dispatch HTTP notifications only to endpoints whose SHA-256 hash matches the on-chain record.
- **Ledger Time Progression**: The timelock relies on the ledger timestamp advancing monotonically; the delay is measured against the ledger time recorded at proposal time.

---

## What the Contract Protects Against

### 1. Unauthorized Modification and Deletion
- **Owner-Exclusive Mutation**: `update_alert`, `update_label`, `update_webhook`, `propose_webhook`, `confirm_webhook`, `renew_alert_ttl`, and `remove_alert` require `caller.require_auth()` and verify that `caller == config.owner`. Non-owners receive `ContractError::Unauthorized`.
- **Admin-Exclusive Operations**: `transfer_admin`, `set_per_owner_alert_limit`, `set_watcher_registry`, and `remove_alert_by_admin` enforce admin authorization via `admin.require_auth()` and `assert_admin()`. Unauthenticated or unauthorized callers receive `ContractError::Unauthorized`.

### 2. Webhook Endpoint Hijacking & Blackholing (Two-Phase Rotation)
- **Staged Transitions (`propose_webhook` / `confirm_webhook`)**: Staging the new webhook hash in `pending_webhook_hash` leaves the active `webhook_hash` undisturbed until explicit confirmation. This prevents temporary or permanent notification blackouts during endpoint migrations.
- **Typo / Misconfiguration Correction**: An owner can overwrite `pending_webhook_hash` with subsequent `propose_webhook` calls prior to `confirm_webhook`, mitigating irreversible fat-finger errors.
- **Fixed-Width Digest Type**: `webhook_hash` and `pending_webhook_hash` are `BytesN<32>`, so every entry point can only receive a 32-byte digest; a truncated, padded or non-hex value cannot be constructed, and no runtime format check is needed (#214).
- **Public Audit Trail**: Rotation lifecycle transitions emit dedicated events (`alert.wh_prop` and `alert.wh_conf`) on-chain.

### 3. Destination Endpoint Privacy
- **Digest-Only On-Chain Storage**: Raw webhook URLs (which may contain internal hostnames, ports, private paths, or token parameters) are never stored or passed on-chain. Only 32-byte SHA-256 digests (`BytesN<32>`) are accepted.

### 4. Denial-of-Service & Resource Exhaustion
- **Per-Owner Active Alert Ceiling**: The admin-configurable `per_owner_alert_limit` restricts the number of active alerts any single address can create, preventing storage spam and index bloating.
- **Rule Count and Length Bounds**: Alert rule lists are capped at a maximum of 50 rules (`ContractError::TooManyRules`), and labels are restricted to a maximum of 128 bytes (`ContractError::LabelTooLong`).
- **Rule Descriptor Validation**: `validate_rules` parses each rule descriptor and rejects unrecognized prefixes, preventing injection of unbounded or malformed rule strings.
- **Rule Uniqueness**: `validate_rules` rejects duplicate descriptors (`ContractError::DuplicateRule`), so a single alert cannot store redundant copies of the same rule that would waste storage and skew rule-count-based limits.
- **Saturating Pagination**: Pagination math in `get_contract_alerts_paginated` and `get_alerts_by_owner_paginated` uses saturating arithmetic to prevent integer overflow denial of service.

### 5. Unauthorized Read Access (Watcher-Gating Attack Surface)
- **Gated Query Access**: When `WatcherRegistry` is configured, read queries (`get_alerts_for_contract`, `get_alerts_by_owner`, `get_active_alerts_for_contract`, `get_alerts_by_ids`, `get_alert`, `get_alert_active`, and paginated variants) cross-call `WatcherRegistry::is_watcher_authorized` with the `querier` address. Unauthorized queriers are rejected with `ContractError::NotAWatcher`.
- **Fail-Closed Verification**: If watcher-gating is enabled and the querying address is not registered, alert data is withheld.
- **Configuration-Time Address Validation**: `set_watcher_registry` probes the candidate address with a read-only `is_watcher_authorized` call *before* persisting it, rejecting a non-contract address or a contract that does not implement the `WatcherRegistry` interface with `InvalidWatcherRegistry`. This prevents a misconfigured address from being stored and later panicking inside `assert_watcher_if_configured` on every gated read — previously the failure surfaced only at query time rather than at configuration time.

### 6. Storage Expiry & Sync Drift
- **Automatic TTL Extension**: Every state-modifying call automatically extends persistent storage TTLs by `DEFAULT_TTL` (~24 hours).
- **Safe Permissionless TTL Bumping**: Anyone may call `bump_alert` to extend an alert's TTL up to `MAX_TTL` (~31 days), with values exceeding the cap safely clamped.
- **Non-Mutating Renewal (`renew_alert_ttl`)**: Owners can renew storage TTL without mutating `updated_at`, preventing false positive triggers in off-chain incremental sync filters (`get_alerts_modified_since`).

### 7. Instant Privileged Takeover (Timelocked Upgrade & Admin Transfer)
- **Optional `TimelockDelay`**: When a non-zero `TimelockDelay` is configured, both WASM replacement (`upgrade`) and admin handover (`TransferAdmin`) are gated behind a two-step `propose`/`execute` flow. A single admin key can no longer instantly replace the contract code or hand off the admin role.
- **Propose / Execute / Cancel Flow**:
  - `propose_*` records a pending proposal (target WASM hash or new admin address) together with the ledger time at which it becomes executable (`proposed_at + TimelockDelay`). Proposing requires admin authorization.
  - `execute_*` may only be called after the delay has elapsed; calling it early fails with `ContractError::TimelockNotElapsed`. Execution requires admin authorization and applies the staged change.
  - `cancel_*` allows the admin to discard a pending proposal before execution, clearing the staged state.
- **Raise-Only Delay Outside the Timelock**: The `TimelockDelay` value itself can only be *raised* through the normal (non-timelocked) admin path. Lowering the delay requires going through the timelock, so a compromised admin key cannot shorten the delay to zero and then instantly execute a pending upgrade or admin transfer. This mirrors the `WatcherRegistry` design.
- **Timelock Applies While Paused**: The timelock gating is enforced independently of the contract's paused state. Even when the contract is paused, `upgrade` and `TransferAdmin` remain subject to the propose/execute delay — pausing does not provide a shortcut around the timelock, and the timelock does not provide a bypass around the pause.
- **Audit Trail**: Proposal, execution, and cancellation transitions emit dedicated events so off-chain monitors can observe pending privileged changes during the delay window.

---

## What the Contract Does NOT Protect Against

- **Compromised Private Keys**: If an alert owner's private key is compromised, the attacker can alter rules, rotate webhook hashes, or delete alerts. If the admin key is compromised, the attacker can modify limits, reassign the watcher registry, or delete arbitrary alerts. A configured `TimelockDelay` limits — but does not eliminate — the blast radius of a compromised admin key for `upgrade` and `TransferAdmin`: the attacker can still propose malicious changes, and those changes will execute once the delay elapses unless the legitimate admin (or governance) cancels them in time.
- **Malicious or Vulnerable External Webhook Servers**: The contract cannot verify whether the server behind a hashed webhook URL is reachable, secure, or responding to HTTP payloads.
- **Off-Chain Watcher Misbehavior or Collusion**: Once an address is registered as an authorized watcher in `WatcherRegistry`, `AlertRegistry` cannot detect if that watcher fails to dispatch alerts, alters event payloads, or leaks subscription data off-chain.
- **Mempool Front-Running & Observation**: Proposed webhook hashes, rule updates, and admin transactions are visible in the public mempool before transaction inclusion.
- **Storage Expiry from Prolonged Inactivity**: If an alert is neither written to, renewed, nor bumped for more than its remaining TTL ledgers, Stellar will archive the persistent storage entry. Archival recovery requires off-chain state restoration.
- **Multi-Sig Governance**: The admin role is a single address. The optional `TimelockDelay` provides a time-based safety window for `upgrade` and `TransferAdmin`, but it is not a multi-signature scheme; external multi-sig accounts must still be used where multiple approvers are required. When no `TimelockDelay` is configured (delay of zero), `upgrade` and `TransferAdmin` execute immediately.

---

## Pending State

`AlertRegistry` holds three kinds of staged state between transactions. Each is authority or routing that can be exercised after the situation it was proposed in has changed, so each is checked against that change.

### Stale proposals

| State | Created by | Expiry | Cleared when |
|---|---|---|---|
| `PendingAdminTransfer` | `propose_admin_transfer` | `ADMIN_TRANSFER_EXPIRY_LEDGERS` (120,960 ledgers, about 7 days) | `accept_admin_transfer`, `cancel_admin_transfer`, or `transfer_admin` |
| `PendingTransfer(alert_id)` | `propose_alert_transfer` | `ALERT_TRANSFER_EXPIRY_LEDGERS` (120,960 ledgers) | `accept_alert_transfer`, `reject_alert_transfer`, or `cancel_alert_transfer` |
| `pending_webhook_hash` on each `AlertConfig` | `propose_webhook` | `WEBHOOK_PROPOSAL_EXPIRY_LEDGERS` (120,960 ledgers) | `confirm_webhook`, `cancel_webhook_proposal`, a re-proposal, or `update_webhook` |

### Attack scenarios

- **Admin transfer accepted after a direct transfer (#365, #410).** A proposal to address B survives an admin rotation to C, and B accepts it. `transfer_admin` clears `PendingAdminTransfer`, so the old proposal cannot be accepted by its target after the handover.
- **Admin transfer accepted after the timelock is enabled (#404).** `AlertRegistry` has no timelock; that case is handled by the `WatcherRegistry` section. Here the equivalent risk is a proposal that is still live during an incident. `accept_admin_transfer` is blocked by `pause`, and `cancel_admin_transfer` remains available while paused, so an admin can revoke the proposal before it is accepted.
- **Stale webhook rotation (#392).** `confirm_webhook` rejects a staged hash once `WEBHOOK_PROPOSAL_EXPIRY_LEDGERS` has passed (`WebhookProposalExpired`, 29), but **the staged hash is not removed**. It stays in `pending_webhook_hash` until the owner cancels or re-proposes. A fresh proposal overwrites it, but an owner who forgets it leaves a dormant routing change on the alert. Note that `propose_webhook` itself does not expire: it can re-stage at any time.
- **Direct update races a staged rotation.** A staged hash must not be promoted over a newer direct `update_webhook`. `update_webhook` clears `pending_webhook_hash`, so a later `confirm_webhook` has nothing to promote. See [ADR 0001](adr/0001-two-phase-webhook-rotation.md).

### Mitigations

- `transfer_admin` clears any pending admin transfer, so a stale admin proposal cannot survive a handover.
- `accept_admin_transfer`, `accept_alert_transfer`, and `confirm_webhook` return errors once the expiry window has passed.
- `pause` blocks `propose_admin_transfer` and `accept_admin_transfer`. `cancel_admin_transfer` remains available while paused, so an admin can revoke a pending proposal during an incident.
- `update_webhook` supersedes any staged rotation.
- Owners can cancel their own proposals with `cancel_webhook_proposal` and `cancel_alert_transfer`.

### Operator checks

```bash
# Pending admin transfer: should be None, or a proposal you made and are tracking
stellar contract invoke --id <ALERT_REGISTRY_CONTRACT_ID> --network <NETWORK> -- get_pending_admin_transfer

# Pending transfer for a specific alert: an expired entry is still returned
stellar contract invoke --id <ALERT_REGISTRY_CONTRACT_ID> --network <NETWORK> -- get_pending_alert_transfer --config_id <CONFIG_ID>

# Staged webhook rotation: pending_webhook_hash should be null unless you are mid-rotation
stellar contract invoke --id <ALERT_REGISTRY_CONTRACT_ID> --network <NETWORK> -- get_alert --querier <QUERIER_ADDRESS> --config_id <CONFIG_ID>
```

- None of the getters return the expiry ledger for a pending webhook rotation. Use the `alert.wh_prop` event ledger, and treat a rotation older than 120,960 ledgers as stale.
- Stale admin proposals are cleared with `cancel_admin_transfer`. Stale alert transfers and webhook rotations are cleared by the owner with `cancel_alert_transfer` and `cancel_webhook_proposal`.
- Audit `("admin", "transfer")`, `("admin", "pause")`, `("alert", "xfer_prop")`, `("alert", "wh_prop")`, and `("alert", "wh_conf")` for the period since the last check.

---

## Attack Scenarios

### 1. Attacker Attempts to Hijack Webhook Destination
- **Vector**: Attacker calls `propose_webhook` or `update_webhook` on a victim's alert ID to redirect alerts to an attacker-controlled endpoint.
- **Outcome**: Call fails at `caller.require_auth()` and `config.owner == caller` check with `ContractError::Unauthorized`.

### 2. Accidental Cutover During Webhook Migration
- **Vector**: Operator rotates webhook endpoint using single-step `update_webhook` before the new server is online.
- **Outcome**: Immediate failure of subsequent notifications.
- **Mitigation (Two-Phase)**: Operator uses `propose_webhook` to stage `pending_webhook_hash`. The live webhook continues processing alerts. Off-chain systems verify the new endpoint and issue `confirm_webhook` only when ready.

### 3. Unauthorized Querier Attempts to Scrape Subscriptions (Watcher-Gated Mode)
- **Vector**: Unregistered third-party address attempts to query `get_alerts_for_contract` or `get_alerts_by_owner` when a `WatcherRegistry` is configured.
- **Outcome**: `AlertRegistry` queries `WatcherRegistryClient::is_watcher_authorized(&querier)`. The call returns `false`, causing `AlertRegistry` to abort with `ContractError::NotAWatcher`.

### 4. Attacker Attempts Storage Spam via Excessive Alert Creation
- **Vector**: Attacker script repeatedly registers hundreds of active alerts under a single owner account.
- **Outcome**: Once the active count reaches `per_owner_alert_limit`, `register_alert` rejects subsequent registrations with `ContractError::OwnerAlertLimitExceeded`.

### 5. Attacker Injects Malformed Rule Descriptors
- **Vector**: Caller supplies strings with arbitrary binary data, invalid prefixes, or excessive rule count.
- **Outcome**: `validate_rules` limits count to <= 50 (`ContractError::TooManyRules`), rejects duplicate descriptors (`ContractError::DuplicateRule`), and `validate_rule` rejects any descriptor not matching recognized prefixes (`ContractError::InvalidRuleDescriptor`).

### 6. Admin Address Handover
- **Vector**: Former admin or attacker attempts to call `transfer_admin` or `set_watcher_registry` after admin role transfer.
- **Outcome**: `assert_admin()` checks stored `ADMIN` address; unauthorized calls return `ContractError::Unauthorized`.

### 7. Compromised Admin Key Attempts Instant Upgrade or Admin Swap
- **Vector**: An attacker who has obtained the admin key attempts to immediately replace the contract WASM via `upgrade` or hand off the admin role via `TransferAdmin` to lock out the legitimate admin.
- **Outcome**: When a `TimelockDelay` is configured, the attacker can only `propose` the change. The change cannot be executed until the delay elapses; premature execution fails with `ContractError::TimelockNotElapsed`. During the delay window, the legitimate admin (or governance) can observe the proposal and `cancel` it. The attacker cannot shorten the delay to bypass this window, because lowering `TimelockDelay` is itself timelocked (raise-only outside the timelock).

### 8. Attempt to Bypass the Timelock While Paused
- **Vector**: Attacker (or a compromised admin) pauses the contract and then attempts to `upgrade` or `TransferAdmin` immediately, hoping the paused state provides a shortcut.
- **Outcome**: The timelock gating is enforced regardless of paused state. The change still requires a prior `propose` and must wait for the delay to elapse; early execution fails with `ContractError::TimelockNotElapsed`.

---

## Security Properties Summary

| Property | Enforcement Mechanism | Status |
|---|---|---|
| **Alert Owner Authorization** | `caller.require_auth()` + `config.owner == caller` | ✅ Enforced |
| **Admin Function Authorization** | `admin.require_auth()` + stored `ADMIN` match | ✅ Enforced |
| **Two-Phase Webhook Staging** | `pending_webhook_hash` separation from `webhook_hash` | ✅ Enforced |
| **Webhook Hash Integrity** | `BytesN<32>` type: only a 32-byte digest can be passed | ✅ Enforced |
| **Webhook URL Privacy** | Only the 32-byte SHA-256 digest is accepted and stored on-chain | ✅ Enforced |
| **Watcher Read Access Gating** | Cross-contract check via `WatcherRegistryClient` | ✅ Enforced (when enabled) |
| **Per-Owner Spam Throttling** | `get_active_alert_count_for_owner` <= `LIMIT` check | ✅ Enforced (when configured) |
| **Rule Descriptor Validation** | Bounded list (<= 50) + recognized prefix check | ✅ Enforced |
| **Safe TTL Extension & Clamping** | `bump_alert` clamped to `MAX_TTL`; write-time `DEFAULT_TTL` | ✅ Enforced |
| **Timelocked Upgrade & Admin Transfer** | Optional `TimelockDelay` with `propose`/`execute`/`cancel`; raise-only delay; enforced even while paused | ✅ Enforced (when configured) |
| **Re-entrancy Protection** | Soroban atomic execution model + synchronous updates | ✅ Protocol Guarantee |
| **Replay Protection** | Stellar transaction sequence numbers | ✅ Protocol Guarantee |
| **Off-Chain Watcher Fidelity** | Off-chain node integrity | ❌ External / Out of Scope |
| **Compromised Private Key Protection**| External key management / Multi-sig | ❌ External / Out of Scope |