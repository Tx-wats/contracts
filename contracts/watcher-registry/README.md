# WatcherRegistry

> Part of [stellar-txwatch-contracts](https://github.com/Tx-wat/stellar-txwatch-contracts)

On-chain registry for authorized watcher node addresses. An admin (or set of
admins) controls which off-chain watcher nodes are permitted to interact with
the `AlertRegistry` contract. Any caller can query whether a given address is
authorized.

This contract is a clean, self-contained example of common Soroban patterns
and is suitable for use as a reference implementation.

## Key patterns demonstrated

- **`require_auth()` for admin-gated mutations** — every state-changing
  function calls `admin.require_auth()` as its first line, delegating
  signature verification entirely to the Stellar protocol.
- **Multi-admin set** — the admin role is held by a `Vec<Address>` rather
  than a single address, eliminating the single-point-of-failure of a sole
  admin while keeping the authorization model simple and auditable.
- **Idempotent registration** — `register_watcher` and `add_admin` are
  no-ops when the address is already present, making them safe to call
  multiple times without side effects.
- **On-chain audit events** — every privileged mutation emits a
  `soroban_sdk::events` entry so changes are visible on-chain and
  indexable off-chain.
- **`#[contracterror]` enum** — typed error codes (`AlreadyInitialized`,
  `Unauthorized`, `NotInitialized`, `LastAdmin`) instead of raw panics,
  giving callers structured error handling.
- **Instance storage** — all state lives in instance storage, which is
  appropriate for a small, frequently-accessed registry.
- **Last-admin guard** — `remove_admin` refuses to remove the final admin,
  preventing the contract from becoming permanently unmanageable.

## Build

```bash
cargo build --release --target wasm32-unknown-unknown
```

## Test

```bash
cargo test
```

The test suite covers happy paths, unauthorized rejections, idempotency,
multi-admin operations, and event emission.

## Deploy (testnet)

```bash
bash ../../scripts/deploy.sh --network testnet
```

## Invoke via Stellar CLI

```bash
# Initialize
stellar contract invoke \
  --id <CONTRACT_ID> --source <ADMIN_IDENTITY> --network testnet \
  -- initialize --admin <ADMIN_ADDRESS>

# Register a watcher
stellar contract invoke \
  --id <CONTRACT_ID> --source <ADMIN_IDENTITY> --network testnet \
  -- register_watcher --admin <ADMIN_ADDRESS> --watcher <WATCHER_ADDRESS>

# Check authorization
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- is_authorized --watcher <WATCHER_ADDRESS>
```

## TypeScript bindings

```bash
npm install @tx-wat/watcher-registry @stellar/stellar-sdk
```

> **Note:** the npm packages are published by CI on the first tagged release. Until then, generate the bindings locally with `make bindings` from the repository root.

See [bindings/watcher-registry](../../bindings/watcher-registry/README.md).

## Function reference

See [docs/watcher-registry.md](../../docs/watcher-registry.md).

## License

MIT
