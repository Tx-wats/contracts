# ADR 0002: Alert Activity Model

## Status

Accepted

## Context

Four independent mechanisms can hide an alert in `AlertRegistry`:

| Mechanism | Storage | Set by | Cleared by |
|-----------|---------|--------|------------|
| Owner flag | `DataKey::AlertActive(id)` | owner via `set_alert_active` / `set_alert_active_checked` | owner |
| Admin suspension | `DataKey::AdminSuspended(id)` | admin via `deactivate_alert_by_admin` | admin only |
| Snooze | `DataKey::SnoozeUntil(id)` | owner via `snooze_alert` | ledger passes `until`, or owner |
| Expiry | `AlertConfig::expires_at_ledger` | owner via `set_alert_expiry` | owner (`None`), or the alert is pruned |

Each getter has combined these differently, so every new PR picked its own
semantics. This ADR fixes one definition.

## Decision

### Definition

An alert is **active** at ledger `L` if and only if all of the following hold:

1. it exists (not removed, not pruned);
2. the owner flag is `true`;
3. it is not admin-suspended;
4. it is not snoozed (`L < SnoozeUntil` is snoozed);
5. it is not expired (`L > expires_at_ledger` is expired, when an expiry is set).

### Precedence

The mechanisms are a logical AND; no mechanism overrides another. The only
ordering is who may clear what:

- **Admin suspension dominates the owner flag.** While suspended, the owner
  cannot set the flag back to `true`; `set_alert_active` fails rather than
  silently storing a value that has no effect.
- **Snooze and expiry are time-based** and never mutate the owner flag. When a
  snooze ends, the alert returns to whatever the flag and suspension say.
- **Expiry is terminal for activity** until the owner extends or clears it, or
  the alert is pruned.

### Counters

- *Live* counters (`OwnerLiveCount`, per-contract counts such as
  `get_active_contract_alert_count`) count **existing** alerts, regardless of
  flag, suspension, snooze or expiry. They change only on register, remove and
  prune. Keeping them time-independent means no ledger tick ever has to update
  storage.
- Any count of *currently active* alerts must apply the full definition above
  at read time and is therefore bounded by a paginated scan, never stored.

### Quotas

Per-owner, per-contract and global limits are charged against **existing**
alerts (the live counters). Deactivating, suspending, snoozing or letting an
alert expire does not free quota; only removal or pruning does. This stops an
owner from bypassing a quota by toggling alerts off and registering more.

### Sync visibility

- `get_alert` and other raw getters return the stored record regardless of
  activity, so off-chain watchers can see why an alert is hidden.
- Active-filtered getters (`get_active_contract_alerts_paginated`,
  `get_active_contract_alerts_paged`, and the deprecated
  `get_active_alerts_for_contract`) apply the full definition, so a watcher
  syncing from them never dispatches for a flagged-off, suspended, snoozed or
  expired alert.
- `get_alert_active` returns only the stored owner flag; callers needing the
  full answer must combine it with `is_alert_suspended`, the snooze state and
  `expires_at_ledger`, or use an active-filtered getter.

## Consequences

- New hiding mechanisms must be added to this definition and to every
  active-filtered getter in the same PR.
- Every active-filtered getter links to this ADR in its rustdoc.
- Counters and quotas stay cheap and deterministic, at the cost of differing
  from the active set; docs must say "live" when they mean "existing".
