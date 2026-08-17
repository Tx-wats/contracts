# @tx-wat/watcher-registry

TypeScript bindings for the **WatcherRegistry** Soroban smart contract,
part of the [stellar-txwatch-contracts](https://github.com/Tx-wat/stellar-txwatch-contracts)
project.

These bindings are generated from the compiled WASM contract spec using
[`stellar contract bindings typescript`](https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli)
and published automatically on every GitHub release.

## Installation

```bash
npm install @tx-wat/watcher-registry @stellar/stellar-sdk
```

> **Note:** the npm packages are published by CI on the first tagged release. Until then, generate the bindings locally with `make bindings` from the repository root.

## Usage

```typescript
import {
  Client,
  networks,
} from "@tx-wat/watcher-registry";
import { Keypair, Networks } from "@stellar/stellar-sdk";

// Connect to testnet
const client = new Client({
  contractId: networks.testnet.contractId,
  networkPassphrase: Networks.TESTNET,
  rpcUrl: "https://soroban-testnet.stellar.org",
});

// Check if a watcher is authorized (read-only, no signing needed)
const result = await client.is_authorized({
  watcher: "GABC...XYZ",
});
console.log("authorized:", result.result);

// Register a watcher (requires admin keypair)
const adminKeypair = Keypair.fromSecret(process.env.ADMIN_SECRET!);
const tx = await client.register_watcher(
  {
    admin: adminKeypair.publicKey(),
    watcher: "GWATCHER...ADDRESS",
  },
  { signTransaction: (xdr) => adminKeypair.sign(xdr) },
);
await tx.signAndSend();

// Get all authorized watchers
const watchers = await client.get_watchers();
console.log("watchers:", watchers.result);
```

## API

All public contract functions are exposed as async methods on the `Client`
class. The method signatures mirror the Soroban contract interface exactly.

| Method | Auth required | Description |
|---|---|---|
| `initialize(admin)` | `admin` | Initialize the registry (once only) |
| `register_watcher(admin, watcher)` | `admin` | Authorize a watcher address |
| `remove_watcher(admin, watcher)` | `admin` | Revoke a watcher address |
| `is_authorized(watcher)` | — | Check if an address is authorized |
| `get_watchers()` | — | Return all authorized watcher addresses |
| `add_admin(caller, new_admin)` | `caller` (admin) | Add a new admin |
| `remove_admin(caller, target_admin)` | `caller` (admin) | Remove an admin |
| `transfer_admin(admin, new_admin)` | `admin` | Replace the entire admin set |
| `get_admins()` | — | Return all current admin addresses |
| `get_admin()` | — | Return the primary admin address |

## Network addresses

| Network | Contract ID |
|---|---|
| Testnet | See [DEPLOYMENTS.md](https://github.com/Tx-wat/stellar-txwatch-contracts/blob/main/DEPLOYMENTS.md) |
| Mainnet | See [DEPLOYMENTS.md](https://github.com/Tx-wat/stellar-txwatch-contracts/blob/main/DEPLOYMENTS.md) |

## Generating bindings locally

If you need to regenerate the bindings against a specific contract deployment:

```bash
# From the repo root
cargo build --release --target wasm32-unknown-unknown

stellar contract bindings typescript \
  --wasm target/wasm32-unknown-unknown/release/watcher_registry.wasm \
  --contract-id <WATCHER_REGISTRY_CONTRACT_ID> \
  --output-dir bindings/watcher-registry \
  --overwrite

cd bindings/watcher-registry
npm install
npm run build
```

## License

MIT — see [LICENSE](../../LICENSE).
