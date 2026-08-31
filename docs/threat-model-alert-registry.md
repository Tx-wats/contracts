# Threat Model: Alert Registry System

## Overview

The `AlertRegistry` contract manages on-chain alert subscriptions for monitored Stellar smart contracts. It stores alert configurations (target contract addresses, rule descriptors, human-readable labels, owner addresses, active flags, and SHA-256 hashed webhook destination endpoints).

The contract supports:
- **Owner-authenticated alert lifecycle management**: Creation, rule/label updates, status toggling, two-phase webhook hash rotation, TTL renewal, and deletion.
- **Admin-controlled system parameters**: Initialization, administrative handover, per-owner active alert limits, administrative alert deletion, and configuration of an optional `WatcherRegistry` contract for read access gating.
- **Read-access queries**: Direct lookup by ID, active status queries, queries by target contract or owner, paginated query variants, and optional cross-contract watcher-gating.

---

## Assets

| Asset | Description |
|---|---|
| **Alert Configurations** | The on-chain `AlertConfig` records (target contracts, rules, labels, active status, and owner associations) stored under `DataKey::Alert(id)`. |
| **Webhook Endpoints & Hashes** | SHA-256 hex digests (`webhook_hash` and staged `pending_webhook_hash`) that off-chain watcher nodes use to authenticate destination webhook URLs. |
| **Owner & Contract Indices** | The lookup indexes mapping owners to alert IDs (`DataKey::OwnerIndex`) and watched contracts to alert IDs (`DataKey::ContractIndex`). |
| **Active Status Index** | Fast-path boolean lookup table stored under `DataKey::AlertActive(id)`. |
| **Monotonic ID Counter** | Global auto-incrementing identifier (`DataKey::NextId`) ensuring unique alert IDs. |
| **Admin Authority** | Privileged control over global operational limits (`LIMIT`), administrative removal (`remove_alert_by_admin`), admin transfer (`transfer_admin`), and watcher registry linking (`WATCHREG`). |
| **Storage State & TTL** | Persistent storage entries kept alive through automatic write-time renewal, owner-authenticated `renew_alert_ttl`, and permissionless `bump_alert`. |

---

## Trust Assumptions

- **Admin and Owner Keypair Security**: The private keys of the contract admin and alert owners are assumed to be uncompromised and managed securely.
- **Stellar Protocol Integrity**: Cryptographic signature validation and authentication via Soroban's `Address::require_auth()` are enforced correctly by the underlying Stellar network.
- **Atomic Execution & Re-entrancy Model**: Soroban executes contract calls atomically. State changes in `AlertRegistry` are completed synchronously without external re-entrant callbacks during mutation.
- **SHA-256 Cryptographic Preimage Resistance**: SHA-256 is computationally infeasible to invert or collide, preventing attackers from forging raw URLs that produce matching hashes or deducing private URLs from on-chain hashes.
- **WatcherRegistry Trust (when configured)**: If a `WatcherRegistry` address is configured via `set_watcher_registry`, `AlertRegistry` trusts `WatcherRegistryClient::is_watcher_authorized` to accurately report whether a querying address is an authorized watcher node.
- **Off-Chain Watcher Node Fidelity**: Watcher nodes are assumed to faithfully read on-chain alert configs, match event triggers against registered rule descriptors, and dispatch HTTP notifications only to endpoints whose SHA-256 hash matches the on-chain record.

---

## What the Contract Protects Against

### 1. Unauthorized Modification and Deletion
- **Owner-Exclusive Mutation**: `update_alert`, `update_label`, `update_webhook`, `propose_webhook`, `confirm_webhook`, `renew_alert_ttl`, and `remove_alert` require `caller.require_auth()` and verify that `caller == config.owner`. Non-owners receive `ContractError::Unauthorized`.
- **Admin-Exclusive Operations**: `transfer_admin`, `set_per_owner_alert_limit`, `set_watcher_registry`, and `remove_alert_by_admin` enforce admin authorization via `admin.require_auth()` and `assert_admin()`. Unauthenticated or unauthorized callers receive `ContractError::Unauthorized`.

### 2. Webhook Endpoint Hijacking & Blackholing (Two-Phase Rotation)
- **Staged Transitions (`propose_webhook` / `confirm_webhook`)**: Staging the new webhook hash in `pending_webhook_hash` leaves the active `webhook_hash` undisturbed until explicit confirmation. This prevents temporary or permanent notification blackouts during endpoint migrations.
- **Typo / Misconfiguration Correction**: An owner can overwrite `pending_webhook_hash` with subsequent `propose_webhook` calls prior to `confirm_webhook`, mitigating irreversible fat-finger errors.
- **Strict Format Validation**: Both single-step and two-phase rotation enforce exact 64-character hex length requirements (`validate_webhook_hash`), rejecting malformed digests with `ContractError::InvalidWebhookHash`.
- **Public Audit Trail**: Rotation lifecycle transitions emit dedicated events (`alert.wh_prop` and `alert.wh_conf`) on-chain.

### 3. Destination Endpoint Privacy
- **Digest-Only On-Chain Storage**: Raw webhook URLs (which may contain internal hostnames, ports, private paths, or token parameters) are never stored or passed on-chain. Only 64-character SHA-256 hex digests are accepted.

### 4. Denial-of-Service & Resource Exhaustion
- **Per-Owner Active Alert Ceiling**: The admin-configurable `per_owner_alert_limit` restricts the number of active alerts any single address can create, preventing storage spam and index bloating.
- **Rule Count and Length Bounds**: Alert rule lists are capped at a maximum of 50 rules (`ContractError::TooManyRules`), and labels are restricted to a maximum of 128 bytes (`ContractError::LabelTooLong`).
- **Rule Descriptor Validation**: `validate_rules` parses each rule descriptor and rejects unrecognized prefixes, preventing injection of unbounded or malformed rule strings.
- **Saturating Pagination**: Pagination math in `get_contract_alerts_paginated` and `get_alerts_by_owner_paginated` uses saturating arithmetic to prevent integer overflow denial of service.

### 5. Unauthorized Read Access (Watcher-Gating Attack Surface)
- **Gated Query Access**: When `WatcherRegistry` is configured, read queries (`get_alerts_for_contract`, `get_alerts_by_owner`, `get_active_alerts_for_contract`, `get_alert`, `get_alert_active`, and paginated variants) cross-call `WatcherRegistry::is_watcher_authorized` with the `querier` address. Unauthorized queriers are rejected with `ContractError::NotAWatcher`.
- **Fail-Closed Verification**: If watcher-gating is enabled and the querying address is not registered, alert data is withheld.
- **Configuration-Time Address Validation**: `set_watcher_registry` probes the candidate address with a read-only `is_watcher_authorized` call *before* persisting it, rejecting a non-contract address or a contract that does not implement the `WatcherRegistry` interface with `InvalidWatcherRegistry`. This prevents a misconfigured address from being stored and later panicking inside `assert_watcher_if_configured` on every gated read — previously the failure surfaced only at query time rather than at configuration time.

### 6. Storage Expiry & Sync Drift
- **Automatic TTL Extension**: Every state-modifying call automatically extends persistent storage TTLs by `DEFAULT_TTL` (~24 hours).
- **Safe Permissionless TTL Bumping**: Anyone may call `bump_alert` to extend an alert's TTL up to `MAX_TTL` (~31 days), with values exceeding the cap safely clamped.
- **Non-Mutating Renewal (`renew_alert_ttl`)**: Owners can renew storage TTL without mutating `updated_at`, preventing false positive triggers in off-chain incremental sync filters (`get_alerts_modified_since`).

---

## What the Contract Does NOT Protect Against

- **Compromised Private Keys**: If an alert owner's private key is compromised, the attacker can alter rules, rotate webhook hashes, or delete alerts. If the admin key is compromised, the attacker can modify limits, reassign the watcher registry, or delete arbitrary alerts.
- **Malicious or Vulnerable External Webhook Servers**: The contract cannot verify whether the server behind a hashed webhook URL is reachable, secure, or responding to HTTP payloads.
- **Off-Chain Watcher Misbehavior or Collusion**: Once an address is registered as an authorized watcher in `WatcherRegistry`, `AlertRegistry` cannot detect if that watcher fails to dispatch alerts, alters event payloads, or leaks subscription data off-chain.
- **Mempool Front-Running & Observation**: Proposed webhook hashes, rule updates, and admin transactions are visible in the public mempool before transaction inclusion.
- **Storage Expiry from Prolonged Inactivity**: If an alert is neither written to, renewed, nor bumped for more than its remaining TTL ledgers, Stellar will archive the persistent storage entry. Archival recovery requires off-chain state restoration.
- **Multi-Sig / Timelock Governance**: The admin role is a single address without built-in multi-signature or timelock requirements at the contract level (external multi-sig accounts must be used).

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
- **Outcome**: `validate_rules` limits count to <= 50 (`ContractError::TooManyRules`) and `validate_rule` rejects any descriptor not matching recognized prefixes (`ContractError::InvalidRuleDescriptor`).

### 6. Admin Address Handover
- **Vector**: Former admin or attacker attempts to call `transfer_admin` or `set_watcher_registry` after admin role transfer.
- **Outcome**: `assert_admin()` checks stored `ADMIN` address; unauthorized calls return `ContractError::Unauthorized`.

---

## Security Properties Summary

| Property | Enforcement Mechanism | Status |
|---|---|---|
| **Alert Owner Authorization** | `caller.require_auth()` + `config.owner == caller` | ✅ Enforced |
| **Admin Function Authorization** | `admin.require_auth()` + stored `ADMIN` match | ✅ Enforced |
| **Two-Phase Webhook Staging** | `pending_webhook_hash` separation from `webhook_hash` | ✅ Enforced |
| **Webhook Hash Integrity** | 64-char lowercase hex check (`validate_webhook_hash`) | ✅ Enforced |
| **Webhook URL Privacy** | Only SHA-256 hex digest accepted and stored on-chain | ✅ Enforced |
| **Watcher Read Access Gating** | Cross-contract check via `WatcherRegistryClient` | ✅ Enforced (when enabled) |
| **Per-Owner Spam Throttling** | `get_active_alert_count_for_owner` <= `LIMIT` check | ✅ Enforced (when configured) |
| **Rule Descriptor Validation** | Bounded list (<= 50) + recognized prefix check | ✅ Enforced |
| **Safe TTL Extension & Clamping** | `bump_alert` clamped to `MAX_TTL`; write-time `DEFAULT_TTL` | ✅ Enforced |
| **Re-entrancy Protection** | Soroban atomic execution model + synchronous updates | ✅ Protocol Guarantee |
| **Replay Protection** | Stellar transaction sequence numbers | ✅ Protocol Guarantee |
| **Off-Chain Watcher Fidelity** | Off-chain node integrity | ❌ External / Out of Scope |
| **Compromised Private Key Protection**| External key management / Multi-sig | ❌ External / Out of Scope |
