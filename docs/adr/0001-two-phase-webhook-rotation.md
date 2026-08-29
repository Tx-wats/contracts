# ADR 0001: Two-Phase Webhook Rotation

## Status

Accepted (Implemented)

## Context

The `AlertRegistry` contract stores notification configurations on-chain. To preserve endpoint privacy and prevent exposing internal webhook destination URLs on a public ledger, destinations are stored as 64-character lowercase SHA-256 hex digests (`webhook_hash`). Off-chain watcher nodes store the raw URL locally, hash it, and verify that it matches the on-chain hash before dispatching webhook alerts.

Previously, rotating an alert's webhook URL relied exclusively on a direct, single-step update function (`update_webhook`). Direct mutation introduces several operational and security hazards:

1. **Immediate Cutover with Zero Staging**: Direct mutation instantly overwrites the active webhook hash. If the operator submits the transaction before the new receiving endpoint is provisioned, healthy, or registered with off-chain watchers, all incoming alerts during that transition window are dropped.
2. **Accidental Blackholing (Fat-Finger Risk)**: A typo in a 64-character hexadecimal digest is virtually impossible to detect by visual inspection. In a single-step model, committing an erroneous hash immediately breaks notification delivery without any intermediate safety check.
3. **Lack of Canary / Off-Chain Verification Window**: Infrastructure migrations (e.g. moving webhooks to a new server or domain) require a verification period where the new endpoint can be health-checked while the old endpoint continues to receive production traffic.
4. **Audit and Synchronization Gaps**: Off-chain indexers and watcher daemons need clear signals separating a proposed reconfiguration from an activated one.

## Decision

We introduce a **two-phase webhook rotation mechanism** via two distinct entry points in `AlertRegistry`:

1. **`propose_webhook(caller, config_id, new_webhook_hash)` (Phase 1: Stage)**
   - Requires `caller` auth matching `config.owner`.
   - Validates that `new_webhook_hash` is a valid 64-character ASCII hex string.
   - Stores the new hash in `AlertConfig::pending_webhook_hash: Option<String>`.
   - **Crucially leaves the active `webhook_hash` unchanged.** The existing webhook remains fully functional.
   - Emits an `(Symbol("alert"), Symbol("wh_prop"))` event containing `(config_id, caller)`.
   - Idempotent / Re-proposable: An owner can call `propose_webhook` again to overwrite a mistaken proposal before confirmation.

2. **`confirm_webhook(caller, config_id)` (Phase 2: Promote)**
   - Requires `caller` auth matching `config.owner`.
   - Validates that `pending_webhook_hash` contains `Some(hash)`. If `None`, rejects with `ContractError::NoPendingWebhook`.
   - Promotes `pending_webhook_hash` to `webhook_hash`.
   - Resets `pending_webhook_hash` to `None`.
   - Updates `updated_at` ledger timestamp and refreshes persistent storage TTL.
   - Emits an `(Symbol("alert"), Symbol("wh_conf"))` event containing `(config_id, caller)`.

3. **Legacy `update_webhook` Retention**
   - The direct `update_webhook` function is retained for backwards compatibility and emergency updates where single-step atomic cutover is explicitly required.

## Threat Analysis & Mitigations

| Threat / Risk Scenario | Single-Step `update_webhook` | Two-Phase Rotation (`propose` + `confirm`) |
|---|---|---|
| **Accidental Blackhole / Typo**: Operator submits incorrect SHA-256 hash. | Active endpoint immediately replaced; alerts fail silently. | Active endpoint remains intact; operator can inspect `pending_webhook_hash` or re-propose before confirming. |
| **Premature Switchover**: New server not yet ready to process webhook traffic. | Traffic immediately routes to unready server; alerts lost. | Operator stages hash, verifies endpoint health / watcher readiness, and confirms only when verified. |
| **Watcher Out-of-Sync**: Off-chain watcher nodes take time to pick up new destination mapping. | Watcher fails hash comparison immediately on next event. | Watchers observe `wh_prop` event, preload new URL mapping, and seamlessly switch upon `wh_conf`. |
| **Unauthorized Modification**: Malicious actor attempts to hijack alert destination. | Protected by `caller.require_auth()` and owner check. | Protected by `caller.require_auth()` and owner check on both `propose` and `confirm` steps. |

## Consequences

### Positive
- **Zero-Downtime Rotations**: Webhook endpoints can be migrated smoothly without missing mission-critical alert notifications.
- **Resilience to Operational Errors**: Fat-finger mistakes can be detected and corrected before activating the new hash.
- **Clear Observability**: Distinct `alert.wh_prop` and `alert.wh_conf` events provide a transparent audit log on-chain for monitoring systems and watcher nodes.
- **Backwards Compatibility**: Existing clients can continue using `update_webhook` if they do not wish to adopt the two-phase flow.

### Tradeoffs
- **Transaction Overhead**: Completing a two-phase rotation requires two on-chain transactions instead of one.
- **Storage Overhead**: Adds an `Option<String>` field (`pending_webhook_hash`) to `AlertConfig` in persistent storage.
