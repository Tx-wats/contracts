## Build

```bash
cargo build --release --target wasm32-unknown-unknown
```

## Test

```bash
cargo test
```

The test suite covers happy paths, unauthorized rejections, idempotency, multi-admin operations, and event emission.

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

## License
MIT
