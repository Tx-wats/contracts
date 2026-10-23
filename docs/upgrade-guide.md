# Upgrade Guide

Both contracts can replace their own WASM in place via Soroban's
`update_current_contract_wasm`, so a deployed contract ID — and all the state
stored under it — survives a code change.

| Contract | Entrypoint | Authorized by |
|---|---|---|
| `AlertRegistry` | `upgrade(admin, new_wasm_hash)` | the stored admin |
| `WatcherRegistry` | `upgrade(admin, new_wasm_hash)` | any address in the admin set |

`new_wasm_hash` is the 32-byte hash of a WASM binary that is **already
installed** on the network (`stellar contract install` prints it).

## Before you upgrade

An upgrade swaps code but never touches storage: every existing entry stays
exactly as it was, and the new build reads it back with its own type
definitions. The host cannot check that those still match, so the checks below
are the operator's responsibility.

- **Storage keys must be preserved.** Keep the `DataKey` variants — and their
  declaration order, which determines their encoding — as they are. Add new
  variants at the **end**; never reorder, rename or remove existing ones.
- **Stored types must stay compatible.** `AlertConfig`, `PendingAction` and any
  other `#[contracttype]` struct must keep its existing fields with the same
  names and types. Adding a field changes the encoding of every already-stored
  value, so a migration entrypoint is needed rather than a plain upgrade.
- **Counters must be preserved.** `AlertRegistry`'s `NEXT_ID` counter
  (`instance_key::NEXT_ID`, an instance-storage key) and
  `WatcherRegistry`'s `W_CNT` are read as-is after the upgrade; a build that
  interprets them differently will hand out duplicate alert IDs or a wrong
  watcher count.
- **Keep the interface additive.** Removing or renaming an entrypoint, or
  changing its signature, breaks every existing caller — including the
  published TypeScript bindings and any contract that calls
  `is_watcher_authorized` cross-contract.
- **Rehearse on testnet first,** against a contract holding representative
  state, and read the state back after the upgrade.

## Upgrading

`scripts/upgrade.sh` builds, installs the new WASM and invokes `upgrade`:

```bash
./scripts/upgrade.sh \
  --contract watcher-registry \
  --contract-id <CONTRACT_ID> \
  --network testnet
```

Or by hand:

```bash
NEW_WASM_HASH=$(stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/watcher_registry.wasm \
  --source deployer --network testnet)

stellar contract invoke --id <CONTRACT_ID> --source deployer --network testnet \
  -- upgrade --admin <ADMIN_ADDRESS> --new_wasm_hash "$NEW_WASM_HASH"
```

Bump the `Version` value in the contract's `contractmeta!` before building, so
the deployed metadata identifies which build is live.

## Upgrading a timelocked `WatcherRegistry`

An upgrade can rewrite every rule the contract enforces, so it is treated as a
sensitive admin action: while a timelock delay is configured, `upgrade` returns
`TimelockRequired` and must go through the queue.

### Using `scripts/upgrade.sh`

The upgrade script supports two-phase timelocked upgrades directly:

```bash
# Step 1: Upload new WASM and propose upgrade behind timelock
./scripts/upgrade.sh \
  --contract watcher-registry \
  --contract-id <CONTRACT_ID> \
  --network testnet \
  --propose

# Step 2: Once the required timelock delay in ledgers has passed, execute:
./scripts/upgrade.sh \
  --contract watcher-registry \
  --contract-id <CONTRACT_ID> \
  --network testnet \
  --execute
```

If `scripts/upgrade.sh` is invoked without `--propose` on a `watcher-registry` contract with a non-zero timelock delay, it will automatically detect the delay and propose the action.

### Manual Invocations via `stellar-cli`

```bash
# Queue it — records ready_at and emits admin.propose
stellar contract invoke --id <CONTRACT_ID> ... -- propose_admin_action \
  --caller <ADMIN_ADDRESS> --action '{"Upgrade":"<NEW_WASM_HASH>"}'

# ...once ready_at is reached
stellar contract invoke --id <CONTRACT_ID> ... -- execute_admin_action \
  --caller <ADMIN_ADDRESS>
```

Any admin can call `cancel_admin_action` during the window. See
[threat-model.md](threat-model.md#admin-time-lock).

## After the upgrade

- Read a few entries back (`get_admins`, `get_watchers`, `get_alert`) and
  confirm they decode as expected.
- Regenerate the TypeScript bindings (`make bindings-all`) if the interface changed.
- Record the new WASM hash in [DEPLOYMENTS.md](../DEPLOYMENTS.md).

## Rollback

Rollback is just another upgrade: invoke `upgrade` with the previous WASM hash,
which stays installed on the network. This only works while the old build can
still read the current state — an upgrade that migrated stored data is not
reversible this way.

## Storage schema version and migrations (`AlertRegistry`)

Before this guide's checks above were codified, layout changes were migrated
lazily and ad hoc — for example `legacy_owner_active_count_key` in
`contracts/alert-registry/src/lib.rs`, which rewrites an entry under its old
key the first time it is read under the new one. That approach has no way to
tell a freshly deployed contract what layout an existing one is on, so a new
build cannot decide whether a lazy-migration path is even needed.

`AlertRegistry` now also stores an explicit schema version in instance storage
under `instance_key::SCHEMAV`, set to `CURRENT_SCHEMA_VERSION` by both
`__constructor` and `initialize`. Read it with:

```bash
stellar contract invoke --id <CONTRACT_ID> ... -- get_schema_version
```

When a future upgrade changes the on-chain layout (a new field, a changed
key, a format change to an existing value), bump `CURRENT_SCHEMA_VERSION` and
add the field rewrite inside `AlertRegistry::migrate`:

```bash
stellar contract invoke --id <CONTRACT_ID> --source deployer --network testnet \
  -- migrate --caller <ADMIN_ADDRESS> --from <OLD_VERSION> --to <NEW_VERSION> --cursor 0
```

`migrate` is admin-only and resumable: it returns the cursor to pass into the
next call, mirroring the cursor pattern already used by
`deactivate_all_alerts` and `prune_expired_alerts`. Call it repeatedly with the
returned cursor until the call that sets the cursor to the (then-current)
alert count also advances `SCHEMAV` to `to` and emits
`(Symbol("schema"), Symbol("migrate"))`. It rejects with
`SchemaVersionMismatch` if `from` does not match the version currently
stored, and with `InvalidSchemaVersion` if `to` is not strictly greater than
`from`, so a stale or out-of-order migration call cannot silently skip steps.

Recommended sequence for a layout-changing upgrade:

1. Deploy the new WASM via `upgrade` as above — the new build must still be
   able to read the **old** layout, since `migrate` has not run yet.
2. Call `migrate` repeatedly (new cursor each time) until it reports the new
   version via `get_schema_version`.
3. Only after migration completes should new code paths that assume the new
   layout be exercised (e.g. by a second, separate upgrade, or a feature flag
   inside the same build).
