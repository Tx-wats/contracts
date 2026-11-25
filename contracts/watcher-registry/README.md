## Build

```bash
cargo build --release --target wasm32-unknown-unknown
```

## Test

```bash
cargo test
```

The test suite covers happy paths, unauthorized rejections, idempotency, multi-admin operations, and event emission.

## Governance notes

- **Guardian role.** `set_guardian` / `get_guardian` manage a low-privilege
  responder key that may call `pause` (but not `unpause`). This mirrors
  `AlertRegistry`'s guardian: on-call responders can halt the registry without
  holding an upgrade-capable admin key. Guardian changes emit
  `guardian_set` / `guardian_removed` events and reuse the same error
  discriminants as `AlertRegistry` (`NotGuardian`, `GuardianAlreadySet`, ...).
- **Timelock delay.** The direct `set_timelock_delay` setter is raise-only, but
  a lower delay (including `0`, which disables the timelock) can be scheduled
  through `propose_admin_action(AdminAction::SetTimelockDelay)` and applied once
  the current timelock elapses. Lowering the delay is therefore possible, just
  subject to the delay it is replacing.
- **Clearing watchers.** `clear_all_watchers` / `AdminAction::ClearAllWatchers`
  deauthorize every watcher *except* a caller-supplied kept set, so the
  `MIN_WATCHERS` invariant is preserved. Passing an empty kept set is rejected
  with `BelowMinWatchers`.

## Contract spec golden file

Every public function and error discriminant exposed by this contract is pinned in
`contracts/watcher-registry/spec.golden.json`. That file is extracted from the
built WASM with `stellar contract info interface --output json` and verified in CI
so that renaming or removing an entrypoint (which would silently break every
generated binding) fails a PR before it lands.

### Regenerate

Only regenerate after rebuilding and re-optimizing the release WASM:

```bash
cargo build --release --target wasm32-unknown-unknown --locked -p alert-registry -p watcher-registry
stellar contract optimize --wasm target/wasm32-unknown-unknown/release/alert_registry.wasm
stellar contract optimize --wasm target/wasm32-unknown-unknown/release/watcher_registry.wasm
bash scripts/update-spec-golden.sh
```

Then commit the new `contracts/alert-registry/spec.golden.json` (and/or
`contracts/watcher-registry/spec.golden.json`) alongside the change that required it.

> **Note:** the guardian entrypoints (`set_guardian`, `get_guardian`) and the
> revised `clear_all_watchers` signature change the watcher-registry interface,
> so `contracts/watcher-registry/spec.golden.json` and the generated bindings
> under `bindings/watcher-registry` must be regenerated in the same PR.

### Same check in CI

```bash
bash scripts/spec-golden.sh
```

or `make spec-check`. The CI job `spec-golden` runs this check on every PR and push
to `main`.

## Deploy (testnet)

```bash
bash ../../scripts/deploy.sh --network testnet
```

## Invoke via Stellar CLI

```bash
# The contract is already initialized at deploy time via `__constructor`
# (see `deploy.sh` above). `initialize` is a deprecated, backwards-compatible
# entry point for pre-`__constructor` deployments only — new deployments
# should not call it; it would just return `AlreadyInitialized`.

# Register a watcher
stellar contract invoke \
  --id <CONTRACT_ID> --source <ADMIN_IDENTITY> --network testnet \
  -- register_watcher --admin <ADMIN_ADDRESS> --watcher <WATCHER_ADDRESS>

# Check authorization
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- is_watcher_authorized --watcher <WATCHER_ADDRESS>
```

## TypeScript bindings

```bash
npm install @tx-wats/watcher-registry-bindings @stellar/stellar-sdk
```

> **Note:** the npm packages are published by CI on the first tagged release. Until then, generate the bindings locally with `make bindings` from the repository root.

See [bindings/watcher-registry](../../bindings/watcher-registry/README.md).

## Function reference

See [docs/watcher-registry.md](../../docs/watcher-registry.md).

### `get_configuration`

Returns the full registry configuration in a single call, so dashboards do not
need to invoke `get_admins`, `is_paused`, `get_timelock_delay`,
`get_pending_action`, `get_pending_admin_transfer` and `get_watcher_count`
separately:

```rust
pub struct WatcherRegistryConfiguration {
    pub admins: Vec<Address>,
    pub paused: bool,
    pub timelock_delay: u64,
    pub pending_action: Option<PendingAction>,
    pub pending_admin_transfer: Option<Address>,
    pub watcher_count: u32,
}
```

```bash
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- get_configuration
```

### Watcher metadata and scopes

`set_watcher_info` and `set_watcher_scope` store per-watcher data in
**persistent** storage with TTL extension, so metadata does not bloat instance
storage or the cost of every other call. Both setters:

- require the contract to be unpaused (they fail with `ContractPaused` otherwise);
- reject addresses that are not registered watchers (`NotAuthorized`);
- cap `label` and `operator` at `MAX_WATCHER_STRING_LEN` bytes and the scope
  vector at `MAX_WATCHER_SCOPE_LEN` entries (`InvalidInput` when exceeded).
=======

## License
MIT
