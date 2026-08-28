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
- **Admin hijacking via direct call.** `transfer_admin` requires the current admin's auth signature, preventing an attacker from reassigning admin without controlling the current admin key.
- **Replay attacks.** Stellar's sequence number mechanism prevents replaying previously valid transactions.

---

## What the Contract Does NOT Protect Against

- **Compromised admin key.** If the admin keypair is stolen, an attacker can register arbitrary watchers. A time-lock on the most sensitive actions is available but **opt-in** — see [Admin time-lock](#admin-time-lock). With no delay configured (the default), a stolen key gives instant, irreversible control.
- **Malicious behavior by authorized watchers.** Once a watcher is registered, the contract has no visibility into what that node does off-chain (e.g., sending false alerts, ignoring events).
- **Front-running.** Because Stellar transactions are public before finalization, an observer could attempt to front-run an admin action, though the practical impact is low given the permissioned nature of the registry.
- **Social engineering of the admin.** The contract cannot prevent an admin from being tricked into registering a malicious watcher address.
- **Denial of service.** A malicious admin (or compromised key) could remove all watchers, halting the monitoring system. No minimum-watcher-count enforcement exists.

---

## Admin time-lock

`WatcherRegistry` supports an optional delay on the admin actions that can take
the registry over outright: `add_admin`, `transfer_admin`, `clear_all_watchers`,
and lowering the delay itself.

- `set_timelock_delay(caller, delay_ledgers)` configures the delay. It may be
  **raised** directly; lowering or disabling it must go through the time-lock,
  so a stolen key cannot switch the protection off.
- While a delay is set, the sensitive entrypoints return `TimelockRequired`.
  The action must be queued with `propose_admin_action` and run with
  `execute_admin_action` once `ready_at` is reached.
- Any admin may `cancel_admin_action` during the window. This is what turns the
  delay into protection: pair it with a multi-admin set and off-chain alerting
  on the `admin.propose` event.
- The delay defaults to `0`, in which case the direct entrypoints behave exactly
  as before and no protection applies.

The delay bounds the blast radius of a single compromised key; it does not
prevent a compromised admin from registering watchers, which stays immediate.

---

## Attack Scenarios

### 1. Attacker tries to register themselves as a watcher
**Vector:** Call `register_watcher` without admin auth.  
**Outcome:** Rejected by `require_auth()`. No state change.

### 2. Admin key is compromised
**Vector:** Attacker obtains the admin private key and calls `register_watcher` or `transfer_admin`.  
**Outcome:** With no time-lock configured, the attacker gains full control of the registry. With a time-lock configured, `transfer_admin`, `add_admin` and `clear_all_watchers` can only be queued, giving co-admins a window to call `cancel_admin_action`; `register_watcher` is still immediate. **Remaining mitigation outside contract scope** — use hardware wallets, multi-sig accounts, or key rotation procedures.

### 3. Authorized watcher goes rogue
**Vector:** A legitimately registered watcher node starts sending false or malicious alerts.  
**Outcome:** The contract cannot detect this. **Mitigation:** Admin removes the watcher via `remove_watcher`; off-chain monitoring of watcher behavior is required.

### 4. Admin removes all watchers (accidental or malicious)
**Vector:** Admin calls `remove_watcher` for every registered address.  
**Outcome:** No watchers remain; the monitoring system stops functioning. **Mitigation outside contract scope** — operational procedures and alerts on registry changes.

---

## Security Properties Summary

| Property | Enforced by contract |
|---|---|
| Only admin can modify the registry | ✅ |
| Admin transfer requires current admin auth | ✅ |
| Replay protection | ✅ (Stellar protocol) |
| Admin key compromise protection | ⚠️ partial (opt-in time-lock on sensitive actions) |
| Off-chain watcher behavior enforcement | ❌ |
| Multi-sig / time-lock on admin actions | ⚠️ time-lock, opt-in and disabled by default |
