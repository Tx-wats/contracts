# AlertRegistry

> Part of [stellar-txwatch-contracts](https://github.com/Tx-wat/stellar-txwatch-contracts)

On-chain registry for alert configurations. Each alert specifies a target
contract address to watch, a set of rule descriptors, and a SHA-256 hash of
the destination webhook URL. Off-chain watcher nodes read these configs to
decide when and where to fire notifications.

This contract demonstrates several intermediate-to-advanced Soroban patterns
and is suitable for use as a reference implementation.

## Key patterns demonstrated

- **Persistent storage with TTL extension** — alert configs and their indexes
  are stored in persistent storage and have their TTL extended on every write,
  keeping entries alive as long as they are actively used.
- **Dual-index lookups** — alerts are indexed by both owner address
  (`OwnerIndex`) and target contract address (`ContractIndex`), enabling
  efficient queries in both dimensions without a full scan.
- **Paginated query variants** — `get_contract_alerts_paginated` and
  `get_alerts_by_owner_paginated` accept `offset` and `limit` parameters,
  preventing instruction-limit overruns for owners with many alerts.
- **Monotonic ID counter** — a `u64` counter in instance storage generates
  unique, sequential alert IDs without requiring external coordination.
- **Privacy-preserving webhook storage** — only the 32-byte SHA-256 digest
  (`BytesN<32>`) of the webhook URL is stored on-chain; the raw URL never appears in contract state.
- **Rule descriptor validation** — the contract validates each rule string
  against a known allowlist (`rule:transfer`, `rule:mint`) before accepting it,
  preventing garbage data from entering the registry.
- **Per-owner rate limiting** — an optional admin-configurable limit caps the
  number of active alerts per owner, enforced at registration time.
- **Admin role with transfer** — an optional admin address can remove any
  alert and set owner limits; the role can be transferred via `transfer_admin`.

## Build

```bash
cargo build --release --target wasm32-unknown-unknown
```

## Test

```bash
cargo test
```

## Deploy (testnet)

```bash
bash ../../scripts/deploy.sh --network testnet
```

## Invoke via Stellar CLI

```bash
# Register an alert. webhook_hash is BytesN<32>: pass the 64-char hex
# SHA-256 digest of the URL, e.g. from `printf %s "$URL" | sha256sum`.
stellar contract invoke \
  --id <CONTRACT_ID> --source <OWNER_IDENTITY> --network testnet \
  -- register_alert \
  --owner <OWNER_ADDRESS> \
  --target_contract <WATCHED_CONTRACT> \
  --label "My Alert" \
  --webhook_hash "<sha256-hex>" \
  --rules '["rule:transfer"]'

# Query alerts for a contract (paginated)
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- get_contract_alerts_paginated \
  --target_contract <WATCHED_CONTRACT> \
  --offset 0 \
  --limit 20
```

## Function reference

See [docs/alert-registry.md](../../docs/alert-registry.md).

## License

MIT
