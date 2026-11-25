# Keeper Operations Guide

This guide covers running a keeper for the `alert-registry` and
`watcher-registry` contracts: what to call, how often, what it costs, how to
monitor it, and how to restore state that has already been archived. For the
TTL values and why they matter, read [docs/ttl.md](ttl.md) first.

The reference script is [`scripts/keeper.sh`](../scripts/keeper.sh). It is a
starting point, not a production service.

## What a Keeper Does

Every keeper entrypoint below is permissionless: it needs no auth and changes
no alert content. A keeper identity therefore only needs XLM to pay fees. It
does not need to be an admin, owner or watcher, and it should not be one.

## Cadence

Two rules set the cadence:

1. **Instance bumps only act inside the threshold window.** `bump_instance_ttl`
   extends an instance entry only once its remaining TTL is at or below
   `INSTANCE_BUMP_THRESHOLD` (17 280 ledgers, ~24 hours). Calls made earlier
   are no-ops. A keeper must therefore call at least once per 24-hour window,
   and calling more often than that is what gives margin.
2. **`bump_alert` resets the full alert set, but owner writes reset it to 24
   hours.** `bump_alert(id, MAX_TTL)` sets an alert's entries to ~31 days.
   Mutators such as `update_alert` and `register_alert` reset them to
   `DEFAULT_TTL` (~24 hours), so an alert is only ~24 hours from archival after
   an owner write until the next bump.

| Target | Entrypoint | Recommended cadence | Notes |
|---|---|---|---|
| WatcherRegistry instance | `bump_instance_ttl` | Every 12 hours | No-op until the 24-hour threshold window |
| AlertRegistry instance | `bump_instance_ttl` | Every 12 hours | Also extended by every write; this covers quiet periods |
| Each live alert | `bump_alert(id, 535680)` | Every 12 hours | Covers owner writes that reset alerts to ~24 hours |
| Owner index cleanup | `prune_expired_alerts(owner)` | On demand, or weekly | Only needed to free per-owner quota; see the restore caveat below |
| Contract index cleanup | `prune_contract_index(target)` | On demand, or weekly | Frees per-contract quota; `keeper.sh prune-contracts` |
| Owner renewal | `renew_alert_ttl` | Not for keepers | Owner-only, and resets the alert to `DEFAULT_TTL` (~24 hours) |

A 12-hour cadence is 8 640 ledgers, so each target gets at least two chances
inside every 24-hour window. A daily cadence leaves no margin: a call can land
exactly at the edge of the window.

`bump_alert` fails while the contract is paused (`assert_not_paused`). The
pause flag lives in instance storage, so an archived AlertRegistry instance
also makes every bump fail. Alerts keep counting down during a pause, so monitor
the pause flag (see [Monitoring](#monitoring)).

## Batching Alert Bumps

There is no batch bump entrypoint. `batch_remove_alert` exists, but bumps are
single-ID calls. Soroban allows one contract invocation per transaction, so
"batching" in practice means:

- Keep the set of live alert IDs in a file, one per line.
- Submit one `bump_alert` per ID, as `scripts/keeper.sh alerts` does.
- Run the submissions in small parallel groups if the number of alerts is
  large. Use a separate identity per group if the sequence numbers collide.
- Record failures and retry them on the next run.

To build the ID list, read `get_alerts_by_owner` for each owner, or keep your
own index from the `alert.register` and `alert.renew` events (see
[docs/events.md](events.md)). Reads do not extend TTL, so the list may include
alerts that have already been archived. Those fail the bump and need a restore
first.

```bash
# Every 12 hours: instance entries, then every live alert.
export NETWORK=mainnet MAINNET_RPC_URL=<rpc-url> STELLAR_IDENTITY=keeper
export ALERT_REGISTRY_ID=<alert-registry-id> WATCHER_REGISTRY_ID=<watcher-registry-id>

./scripts/keeper.sh instance
./scripts/keeper.sh alerts --ids-file /etc/keeper/alert-ids.txt --ttl 535680
```

Schedule it with cron or a systemd timer, for example
`0 */12 * * *`. Do not run two keeper instances against the same
identity at the same time, because their sequence numbers will collide.

## Cost

Fees are set per transaction by the network after simulation, and depend on
the entries touched and the TTL extension. This guide does not quote a fee
figure, because none has been measured for this build. To estimate:

1. Simulate one `bump_alert` and one `bump_instance_ttl` on testnet, and record
   the resource and inclusion fees.
2. Multiply by the number of calls per day: `2 × (live alerts + 2)` at a
   12-hour cadence.
3. Add a margin for failed submissions, which still cost fees if they reach
   the ledger.

The cost of a bump grows with the number of alerts, not with the number of
owners. Re-measure after changing `MAX_TTL` or `DEFAULT_TTL`.

## Pruning

`prune_expired_alerts(owner)` removes IDs from an owner's index whose alert
record no longer exists. It is permissionless and emits `alert.pruned`
when it removes anything. `register_alert` calls the same cleanup when an
owner is at the per-owner limit.

Each call scans a bounded window of the owner index and records its position
in `get_prune_cursor`. The script repeats the call for each owner until the
cursor is `None`, so large owners are fully pruned:

```bash
./scripts/keeper.sh prune --owners-file /etc/keeper/owners.txt
```

`prune_contract_index(target)` drops dangling IDs from a target contract's
index and releases the per-contract quota they held. Run it for each target
listed (one address per line, `#` comments allowed):

```bash
./scripts/keeper.sh prune-contracts --targets-file /etc/keeper/targets.txt
```

**Caveat:** the contract cannot tell an expired alert from an archived one,
because a missing persistent entry looks the same either way. Pruning an owner
whose alerts are archived but restorable removes their index entries. The
alerts can still be restored by key, but they are no longer reachable through
`get_alerts_by_owner` and the owner's quota slots are released. Restore archived
alerts before pruning their owner.

## Restoring Archived Entries

Once a persistent entry's TTL reaches zero, the network archives it. Archived
entries are not deleted. They are restored with a `RestoreFootprintOp`, which
costs a fee and cannot be triggered by the contract. The Stellar CLI exposes
this as `stellar contract restore`.

> **Verify before use:** the CLI's flags for this command have changed across
> versions, and the exact `--key` encoding for a `DataKey` variant depends on
> the CLI version. Run `stellar contract restore --help` against your installed
> CLI before using the commands below, and confirm the key encoding on
> testnet first.

Restore order matters:

1. **Instance first.** If the AlertRegistry or WatcherRegistry instance is
   archived, every alert is unreachable, and `bump_alert` and
   `bump_instance_ttl` cannot run. Restore the instance entry, then bump it.
2. **Alerts second.** Restore each archived `Alert(id)`, `AlertActive(id)`,
   `OwnerIndex(owner)` and `ContractIndex(target)` entry, then bump the alert so
   the full set returns to `MAX_TTL`.
3. **Prune last, and only after restoring.** See the caveat above.

Symptoms of archival in a keeper log: a simulation or RPC error mentioning an
archived or expired ledger entry, or an `AlertNotFound` you do not expect for
an alert you have just registered. Check the entry's `liveUntilLedgerSeq` with
Soroban RPC `getLedgerEntries` before assuming the alert was removed.

## Monitoring

Monitor the keeper itself and the TTL state it is responsible for:

- **Keeper freshness.** Record the timestamp of the last successful
  `instance` run and the last successful `alerts` run. Page if the instance
  run is more than 24 hours old, or the alerts run is more than 24 hours old
  with any alert failing.
- **Failed bumps.** Treat any `FAILED alert` line as an alert, not a warning.
  Repeated failures on the same ID usually mean that alert is archived.
- **Remaining TTL.** Compare each entry's `liveUntilLedgerSeq` from Soroban RPC
  `getLedgerEntries` with the current ledger. Alert when any instance entry is
  within 48 hours of expiry, which is far earlier than the 24-hour window the
  keeper relies on.
- **Events.** `alert.bump` events confirm bumps landed on-chain. A drop in
  their count against the number of live alerts means the keeper is failing
  silently. `alert.pruned` counts show how often owners hit dangling IDs. See
  [docs/events.md](events.md).
- **Pause flag.** A paused AlertRegistry rejects bumps and renewals. Alert
  when the contract is paused for longer than a few hours.
- **Keeper balance.** A keeper with no XLM fails every call. Alert on a low
  balance before it reaches zero.

## Related

- [docs/ttl.md](ttl.md): TTL values, the threshold rules and the per-function
  table of which entries each call extends.
- [docs/storage.md](storage.md): which keys share the instance entry.
- [docs/events.md](events.md): the events to index.
- [docs/incident-response.md](incident-response.md): what to do when a
  registry must be paused or rotated.
