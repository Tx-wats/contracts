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
- **Admin hijacking via direct call.** `transfer_admin` requires the current admin's auth signature, preventing an attacker from reassigning admin without controlling the current admin key.
- **A single admin unilaterally stripping co-admins.** In a multi-admin set, `transfer_admin` only ever replaces the *caller's own* slot in the admin set — it cannot touch other admins' entries. An admin wanting to remove another admin must use `remove_admin`, which is a separate, individually-authorized call per target and still refuses to remove the last remaining admin. This means no single admin (even a compromised one) can use `transfer_admin` to seize sole control of a multi-admin registry.
- **Total loss of watcher monitoring via admin action.** `remove_watcher` and `clear_all_watchers` both refuse to drop the registered watcher count below `MIN_WATCHERS` (currently `1`). A malicious or compromised admin can still remove watchers down to that floor, but cannot fully halt the monitoring system through these entrypoints.
- **Replay attacks.** Stellar's sequence number mechanism prevents replaying previously valid transactions.
- **Unbounded registry growth.** `register_watcher` and `add_admin` enforce `MAX_WATCHERS`/`MAX_ADMINS` caps, bounding the worst-case cost of operations (like `clear_all_watchers`) that iterate the full list.

---

## What the Contract Does NOT Protect Against

- **Compromised admin key.** If the admin keypair is stolen, an attacker can register arbitrary watchers. A time-lock on the most sensitive actions is available but **opt-in** — see [Admin time-lock](#admin-time-lock). With no delay configured (the default), a stolen key gives instant, irreversible control.
- **Compromised admin key.** If the admin keypair is stolen, an attacker can register arbitrary watchers or transfer their own admin slot to themselves under a new key. No multi-sig or time-lock is enforced at the contract level. As a stopgap, any admin can call `pause` to freeze all state-mutating entrypoints while the incident is investigated — see [Pause / circuit-breaker](#pause--circuit-breaker) below.
- **Malicious behavior by authorized watchers.** Once a watcher is registered, the contract has no visibility into what that node does off-chain (e.g., sending false alerts, ignoring events).
- **Front-running.** Because Stellar transactions are public before finalization, an observer could attempt to front-run an admin action, though the practical impact is low given the permissioned nature of the registry.
- **Social engineering of the admin.** The contract cannot prevent an admin from being tricked into registering a malicious watcher address.
- **Partial denial of service down to the minimum.** A malicious admin (or compromised key) can still remove watchers down to `MIN_WATCHERS`, degrading monitoring coverage even though the system cannot be fully halted via `remove_watcher`/`clear_all_watchers`.

---

## Pause / circuit-breaker

Both `WatcherRegistry` and `AlertRegistry` expose an admin-gated `pause` / `unpause` pair and a `paused` instance flag. While paused, every state-mutating entrypoint — including admin-management calls like `add_admin`/`remove_admin`/`transfer_admin` — returns `ContractError::Paused`; only `pause`/`unpause` themselves and read-only queries remain callable. This is intended purely as an emergency freeze — e.g. to stop further damage the moment a compromise is suspected — not as a routine operational control. Resolving the underlying compromise (rotating or removing the affected admin) still requires unpausing first.

---

## Admin time-lock

`WatcherRegistry` supports an optional delay on the admin actions that can take
the registry over outright: `add_admin`, `transfer_admin`, `clear_all_watchers`,
and `upgrade`.

- `set_timelock_delay(caller, delay_ledgers)` configures the delay. It may be
  **raised** directly.
- **Lock-in (#64):** once the delay is above `0`, it cannot be lowered or
  disabled by any path. `set_timelock_delay` rejects a lower value with
  `TimelockRequired`, and `propose_admin_action(SetTimelockDelay(lower))` is
  refused at proposal time with the same error, so a lower value is never
  queued. The only remaining exit is a contract upgrade, which is itself
  time-locked. Treat enabling the delay as permanent.
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

Because the delay cannot be lowered (#64), an operator who sets it to a long
value and later needs it shorter must upgrade the contract to change it. Pick
the value deliberately before enabling it.

---

## Watcher scopes and metadata

Each watcher has two optional, admin-written records, both keyed by watcher
address and stored in instance storage:

- **Scope** (`set_watcher_scope`, `DataKey::WatcherScope`): a list of target
  contracts. An empty or absent scope means unrestricted. Only
  `is_watcher_authorized_for` reads it.
- **Metadata** (`set_watcher_info`, `DataKey::WatcherInfo`): free-text `label`
  and `operator`. Informational only; it carries no authorization weight.

### Attack surface

- **Instance bloat.** Neither setter caps the size of the scope list or the
  label/operator strings, and neither checks that the address is registered.
  `remove_watcher` and `clear_all_watchers` do not delete the scope or metadata
  entries, so records for removed watchers stay behind. `MAX_WATCHERS` bounds the
  watcher list only, not these entries. All of this lives in one instance entry
  that is read on every call, so growth raises the cost of every entrypoint and
  can eventually exceed the network's ledger-entry size limit. Both setters are
  also not blocked by `pause`, unlike `register_watcher`.
- **Scope widening on replace (#61 context).** `replace_watcher` moves the
  watcher slot only. Scope and metadata are keyed by address and do not follow
  the replacement. A watcher scoped to `[A]` that is replaced by an address with
  no scope becomes unrestricted under `is_watcher_authorized_for`. A stale scope
  left on a previously removed address also re-applies if that address is
  registered again.
- **Scope not enforced by AlertRegistry (#61).** `AlertRegistry` gates reads
  with `is_watcher_authorized`, not `is_watcher_authorized_for`. A scoped watcher
  can read alerts for any target. Scope is only a guarantee for consumers that
  call `is_watcher_authorized_for` themselves. See
  [threat-model-alert-registry.md](threat-model-alert-registry.md).

### Operator guidance

- After `replace_watcher`, call `set_watcher_scope` on the new address in the
  same operation, and clear the old address's scope with an empty vector.
- After `remove_watcher`, clear the scope and metadata explicitly
  (`set_watcher_scope` with an empty vector, `set_watcher_info` with `None`).
- Do not rely on scope for isolation until #61 is fixed.

---

## Pending State

Two-step and queued admin operations keep authority in storage between transactions: a `PendingAdminTransfer` (from `propose_admin_transfer`) and a `PendingAction` (from `propose_admin_action`). Anything left there can be exercised later, after the admin set has changed, so the contract clears it on the transitions below. Operators must check for what the contract cannot clear.

### Stale proposals and queued actions

| State | Created by | Expiry | Cleared when |
|---|---|---|---|
| `PendingAdminTransfer` | `propose_admin_transfer` | `ADMIN_TRANSFER_EXPIRY_LEDGERS` (120,960 ledgers, about 7 days) | `accept_admin_transfer`, `cancel_admin_transfer`, a direct `transfer_admin`, enabling a timelock delay, or removal of the proposer via `remove_admin` |
| `PendingAction` | `propose_admin_action` | **None.** Executable any time after `ready_at` | `execute_admin_action`, `cancel_admin_action`, or removal of the proposer via `remove_admin` |

### Attack scenarios

- **Proposal accepted after a direct transfer (#365, #410).** An admin proposes a transfer to address B, then rotates admin directly to C. If the old proposal survived, B could accept it and take control. `do_transfer_admin` clears `PendingAdminTransfer` on every direct handover.
- **Proposal accepted after the timelock is enabled (#404).** A proposal made while the timelock was off stays live once `set_timelock_delay` moves the delay above zero. Enabling a delay clears `PendingAdminTransfer`, so the proposal is never accepted under the new regime. Admins must re-propose through the timelock.
- **Proposal or queued action outlives its proposer (#405, #406).** A removed admin's proposal would otherwise remain acceptable, or their queued action remain executable by a colluding admin. `remove_admin` clears both when the proposer is removed. `accept_admin_transfer` also re-checks that the proposer is still an admin, so a proposal that slipped past this would still be rejected.
- **Expired proposal left in storage.** After 120,960 ledgers `accept_admin_transfer` returns `AdminTransferExpired`, but the record is **not** deleted. `get_pending_admin_transfer` still returns the address. An expired record is harmless to accept but is clutter that can mislead an operator.
- **Forgotten queued action.** A `PendingAction` has no expiry. A queued `SetTimelockDelay` or `Upgrade` that nobody cancels stays executable indefinitely once its `ready_at` has passed.

### Mitigations

- The contract clears pending state on every transition listed in the table above.
- `accept_admin_transfer` is rejected once the expiry ledger has passed, and when the proposer is no longer an admin.
- Any admin can call `cancel_admin_transfer` or `cancel_admin_action` at any time, including during an incident.
- `pause` blocks `accept_admin_transfer` on `WatcherRegistry` (it returns `ContractError::Paused`), matching `AlertRegistry`, so a pending takeover cannot be completed while the registry is paused. `cancel_admin_transfer` remains callable while paused, so an admin can still clear a suspicious proposal during an incident.

### Operator checks

Run these after any admin rotation, timelock change, or suspected compromise:

```bash
# Pending admin transfer: should be None, or a proposal you made and are tracking
stellar contract invoke --id <WATCHER_REGISTRY_CONTRACT_ID> --network <NETWORK> -- get_pending_admin_transfer

# Queued action: should be None, or an action you proposed with a known ready_at
stellar contract invoke --id <WATCHER_REGISTRY_CONTRACT_ID> --network <NETWORK> -- get_pending_action

# Timelock delay: should match the value you intended
stellar contract invoke --id <WATCHER_REGISTRY_CONTRACT_ID> --network <NETWORK> -- get_timelock_delay
```

- `get_pending_admin_transfer` does not return the expiry ledger. Record the ledger at which each proposal was made (from the `admin.propose` event) and treat anything older than 120,960 ledgers as stale.
- Clear stale state with `cancel_admin_transfer` (admin) or `cancel_admin_action` (admin).
- Audit the `("admin", "propose")`, `("admin", "cancel")`, and `("admin", "transfer")` events for the period since the last check.

---

## Attack Scenarios

### 1. Attacker tries to register themselves as a watcher
**Vector:** Call `register_watcher` without admin auth.  
**Outcome:** Rejected by `require_auth()`. No state change.

### 2. Admin key is compromised
**Vector:** Attacker obtains the admin private key and calls `register_watcher` or `transfer_admin`.  
**Outcome:** With no time-lock configured, the attacker gains full control of the registry. With a time-lock configured, `transfer_admin`, `add_admin` and `clear_all_watchers` can only be queued, giving co-admins a window to call `cancel_admin_action`; `register_watcher` is still immediate. **Remaining mitigation outside contract scope** — use hardware wallets, multi-sig accounts, or key rotation procedures.
**Vector:** Attacker obtains an admin private key and calls `register_watcher`, `add_admin`, or `transfer_admin`.  
**Outcome:** In a single-admin registry, the attacker gains full control. In a multi-admin registry, `transfer_admin` only replaces the *compromised admin's own* slot — the attacker cannot use it to strip other admins, though they can still add new admins via `add_admin` or perform any other privileged action available to a single admin. **Mitigation:** Any other admin can call `pause` immediately to freeze all mutations (including further `add_admin`/`transfer_admin` calls) while the compromised key is rotated out via `remove_admin`; hardware wallets, multi-sig accounts, or key rotation procedures remain the first line of defense outside the contract.

### 3. Authorized watcher goes rogue
**Vector:** A legitimately registered watcher node starts sending false or malicious alerts.  
**Outcome:** The contract cannot detect this. **Mitigation:** Admin removes the watcher via `remove_watcher`; off-chain monitoring of watcher behavior is required.

### 4. Admin removes all watchers (accidental or malicious)
**Vector:** Admin calls `remove_watcher` repeatedly, or `clear_all_watchers`, attempting to deauthorize every registered address.  
**Outcome:** Both entrypoints refuse to drop the watcher count below `MIN_WATCHERS` (`1` by default) — `clear_all_watchers` is rejected outright while any watcher remains, and the final `remove_watcher` call below the floor returns `ContractError::BelowMinWatchers`. The last watcher cannot be removed through these calls, so monitoring can be degraded but not fully halted this way.

### 5. Incident response while a compromise is suspected
**Vector:** An admin observes suspicious registry activity and wants to stop further damage before finishing the investigation.  
**Outcome:** Any admin can call `pause`, which immediately rejects all state-mutating calls (on both `WatcherRegistry` and `AlertRegistry`) with `ContractError::Paused` while leaving all reads available. Once the compromised admin is identified and removed, an admin calls `unpause` to resume normal operation.

### 5b. Timelock enabled, then needed shorter
**Vector:** An admin enables a long timelock delay, then wants to shorten it.
**Outcome:** Both `set_timelock_delay` and `propose_admin_action(SetTimelockDelay)` return `TimelockRequired`. The delay stays locked in until a contract upgrade. See [Admin time-lock](#admin-time-lock).

### 5c. Watcher replaced into a wider scope
**Vector:** A watcher scoped to `[A]` is swapped for a new address via `replace_watcher`, and the new address was never scoped.
**Outcome:** `is_watcher_authorized_for(new, B)` returns `true`. The scope did not move with the watcher. See [Watcher scopes and metadata](#watcher-scopes-and-metadata).

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
| Admin transfer requires current admin auth | ✅ |
| `transfer_admin` cannot strip other admins in a multi-admin set | ✅ |
| Minimum watcher count enforced on removal | ✅ (`MIN_WATCHERS`) |
| Emergency pause / circuit-breaker on mutations | ✅ |
| Pause blocks `accept_admin_transfer` (cancel still allowed) | ✅ |
| Replay protection | ✅ (Stellar protocol) |
| Admin key compromise protection | ⚠️ partial (opt-in time-lock on sensitive actions) |
| Admin key compromise protection | ❌ (mitigated via `pause` + `remove_admin`, not prevented) |
| Off-chain watcher behavior enforcement | ❌ |
| Multi-sig / time-lock on admin actions | ❌ |
| Bounded watcher/admin set size | ✅ (`MAX_WATCHERS`/`MAX_ADMINS`) |
| Multi-sig / time-lock on admin actions | ⚠️ time-lock, opt-in and disabled by default |
| Timelock can be shortened or disabled once set | ❌ never, lock-in until upgrade (#64) |
| Watcher scope enforced by `AlertRegistry` | ❌ (#61) |
| Watcher scope follows the watcher on `replace_watcher` | ❌ |
| Scope and metadata bounded and cleared on removal | ❌ |
