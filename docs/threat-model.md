# Threat Model: Watcher Authorization System

## Overview

The `WatcherRegistry` contract stores a set of authorized watcher node addresses on-chain. Only addresses registered by an admin may act as watchers in the Tx-wat system.

---

## Assets

| Asset | Description |
|---|---|
| Watcher registry | The on-chain list of authorized watcher addresses |
| Admin authority | The ability to add/remove watchers and transfer admin |

---

## Trust Assumptions

- **Admin keypair is secure.** The admin Stellar account is assumed to be controlled by a trusted operator. Compromise of the admin key is out of scope for the contract itself.
- **Stellar protocol integrity.** The contract relies on `require_auth()` from the Soroban SDK. It trusts that the Stellar network correctly enforces signature verification.
- **Soroban re-entrancy model.** Soroban executes contract calls atomically and does not support classic callback-based re-entrancy into the same stateful contract within a single transaction. The registry contracts do not invoke other contracts during state mutation, so cross-contract callers cannot cause re-entrant state changes.
- **Watcher nodes are honest once authorized.** The contract only controls *who* may be a watcher, not *what* an authorized watcher does off-chain.

---

## What the Contract Protects Against

- **Unauthorized watcher registration.** Only the current admin can call `register_watcher` or `remove_watcher`. Any unsigned or incorrectly signed call is rejected at the protocol level.
- **Admin hijacking via direct call.** `propose_admin_transfer` requires the current admin's auth signature, preventing an attacker from reassigning admin without controlling the current admin key.
- **Locking the contract via a typo'd or unowned new admin.** Admin transfer is a two-step `propose_admin_transfer` / `accept_admin_transfer` flow — the proposed `new_admin` must sign `accept_admin_transfer` themselves before the admin set is replaced, so a typo'd or unowned address cannot silently take over (or permanently lock) the contract. A pending proposal can be cancelled by any admin via `cancel_admin_transfer`.
- **Replay attacks.** Stellar's sequence number mechanism prevents replaying previously valid transactions.

---

## What the Contract Does NOT Protect Against

- **Compromised admin key.** If the admin keypair is stolen, an attacker can register arbitrary watchers or transfer admin to themselves. No multi-sig or time-lock is enforced at the contract level.
- **Malicious behavior by authorized watchers.** Once a watcher is registered, the contract has no visibility into what that node does off-chain (e.g., sending false alerts, ignoring events).
- **Front-running.** Because Stellar transactions are public before finalization, an observer could attempt to front-run an admin action, though the practical impact is low given the permissioned nature of the registry.
- **Social engineering of the admin.** The contract cannot prevent an admin from being tricked into registering a malicious watcher address.
- **Denial of service.** A malicious admin (or compromised key) could remove all watchers, halting the monitoring system. No minimum-watcher-count enforcement exists.

---

## Attack Scenarios

### 1. Attacker tries to register themselves as a watcher
**Vector:** Call `register_watcher` without admin auth.  
**Outcome:** Rejected by `require_auth()`. No state change.

### 2. Admin key is compromised
**Vector:** Attacker obtains the admin private key and calls `register_watcher` or `transfer_admin`.  
**Outcome:** Attacker gains full control of the registry. **Mitigation outside contract scope** — use hardware wallets, multi-sig accounts, or key rotation procedures.

### 3. Authorized watcher goes rogue
**Vector:** A legitimately registered watcher node starts sending false or malicious alerts.  
**Outcome:** The contract cannot detect this. **Mitigation:** Admin removes the watcher via `remove_watcher`; off-chain monitoring of watcher behavior is required.

### 4. Admin removes all watchers (accidental or malicious)
**Vector:** Admin calls `remove_watcher` for every registered address.  
**Outcome:** No watchers remain; the monitoring system stops functioning. **Mitigation outside contract scope** — operational procedures and alerts on registry changes.

### 5. Admin transfers to a typo'd or unowned address
**Vector:** Admin calls `propose_admin_transfer` with an address they mistyped or don't control.  
**Outcome:** No state change to the admin set — the proposal only takes effect once the named `new_admin` signs `accept_admin_transfer`. An unowned or unaccepted proposal can be cancelled by any existing admin via `cancel_admin_transfer`, so the contract is never locked out.

---

## Security Properties Summary

| Property | Enforced by contract |
|---|---|
| Only admin can modify the registry | ✅ |
| Admin transfer requires current admin auth to propose | ✅ |
| Admin transfer requires new admin's own signature to complete | ✅ |
| Replay protection | ✅ (Stellar protocol) |
| Admin key compromise protection | ❌ |
| Off-chain watcher behavior enforcement | ❌ |
| Multi-sig / time-lock on admin actions | ❌ |
