# @tx-wats/watcher-registry-bindings

TypeScript bindings for the **WatcherRegistry** Soroban smart contract,
part of the [stellar-txwatch-contracts](https://github.com/Tx-wat/stellar-txwatch-contracts)
project.

These bindings are generated from the compiled WASM contract spec using
[`stellar contract bindings typescript`](https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli)
and published automatically on every GitHub release.

## Installation

```bash
npm install @tx-wats/watcher-registry-bindings @stellar/stellar-sdk
```

> **Note:** the npm packages are published by CI on the first tagged release. Until then, generate the bindings locally with `make bindings-watcher` from the repository root.

## Usage

```typescript
import {
  Client,
  networks,
} from "@tx-wats/watcher-registry-bindings";
import { Keypair } from "@stellar/stellar-sdk";

// Connect to testnet. `networks.testnet` carries the real deployed contract ID,
// network passphrase, and RPC URL (overlaid from DEPLOYMENTS.md at publish time).
const client = new Client({
  contractId: networks.testnet.contractId,
  networkPassphrase: networks.testnet.networkPassphrase,
  rpcUrl: networks.testnet.rpcUrl,
});

// Check if a watcher is authorized (read-only, no signing needed)
const result = await client.is_watcher_authorized({
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
| `initialize(admin)` | `admin` | *(Deprecated, removal in v0.3.0)* Backwards-compatible initializer for pre-`__constructor` deployments; new deployments are already initialized at deploy time |
| `register_watcher(admin, watcher)` | `admin` | Authorize a watcher address |
| `remove_watcher(admin, watcher)` | `admin` | Revoke a watcher address |
| `is_watcher_authorized(watcher)` | — | Check if an address is authorized |
| `is_authorized(watcher)` | — | *(Deprecated, removal in v0.3.0)* Alias for `is_watcher_authorized` |
| `get_watchers()` | — | Return all authorized watcher addresses |
| `add_admin(caller, new_admin)` | `caller` (admin) | Add a new admin |
| `remove_admin(caller, target_admin)` | `caller` (admin) | Remove an admin |
| `transfer_admin(admin, new_admin)` | `admin` | Replace the entire admin set |
| `get_admins()` | — | Return all current admin addresses |
| `get_admin()` | — | Return the primary admin address |
| `set_guardian(caller, guardian)` | `caller` (admin) | Set the low-privilege guardian address (emits `guardian_set`) |
| `get_guardian()` | — | Return the current guardian address, if any |
| `pause(caller)` | `caller` (admin or guardian) | Pause the registry; the guardian may trip this without an admin key |
| `unpause(caller)` | `caller` (admin) | Resume the registry; the guardian cannot unpause |
| `clear_all_watchers(caller, keep)` | `caller` (admin) | Revoke every watcher except those in `keep`; the kept set must satisfy `MIN_WATCHERS` |
| `set_timelock_delay(caller, delay)` | `caller` (admin) | Raise the timelock delay directly; lowering or disabling requires `propose_admin_action` |
| `propose_admin_action(caller, action)` | `caller` (admin) | Queue a timelocked admin action, including `SetTimelockDelay` with a lower or zero delay |
| `execute_admin_action(caller)` | `caller` (admin) | Execute a queued action once the timelock has elapsed |
| `cancel_admin_action(caller)` | `caller` (admin) | Cancel a queued action |
| `get_timelock_delay()` | — | Return the current timelock delay in seconds |
| `get_pending_admin_action()` | — | Return the queued action and its executable-at ledger, if any |

### Guardian and timelock notes

- **Guardian:** mirrors `AlertRegistry`'s guardian role. `set_guardian` emits
  `guardian_set`; pausing as the guardian emits the same `paused` event as an
  admin pause. Guardian attempts to `unpause` fail with `Unauthorized`.
- **Timelock delay:** `set_timelock_delay` is raise-only and rejects lower
  values with `TimelockRequired`. To lower or disable the delay (including
  setting it to `0`), queue `AdminAction::SetTimelockDelay` via
  `propose_admin_action` and call `execute_admin_action` after the current
  delay has elapsed.
- **`clear_all_watchers`:** performs bulk deauthorization while keeping the
  addresses passed in `keep`. The resulting watcher set must still contain at
  least `MIN_WATCHERS` members, otherwise the call fails with
  `BelowMinWatchers`.
| `get_configuration()` | — | Return a one-shot snapshot of the registry configuration (admins, paused state, timelock delay, pending action, pending admin transfer, watcher count) |

## Network addresses

| Network | Contract ID |
|---|---|
| Testnet | `networks.testnet.contractId` — see [DEPLOYMENTS.md](https://github.com/Tx-wat/stellar-txwatch-contracts/blob/main/DEPLOYMENTS.md) |
| Mainnet | Not yet deployed (`networks.mainnet.contractId` is `""`) — see [DEPLOYMENTS.md](../../DEPLOYMENTS.md) |

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
