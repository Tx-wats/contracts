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
- **Counters must be preserved.** `AlertRegistry`'s `NextId` and
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
- Regenerate the TypeScript bindings (`make bindings`) if the interface changed.
- Record the new WASM hash in [DEPLOYMENTS.md](../DEPLOYMENTS.md).

## Rollback

Rollback is just another upgrade: invoke `upgrade` with the previous WASM hash,
which stays installed on the network. This only works while the old build can
still read the current state — an upgrade that migrated stored data is not
reversible this way.
