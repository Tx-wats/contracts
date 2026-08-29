# tx-watch-contracts

Soroban smart contracts for on-chain alert configuration storage and watcher registry.  
Part of the [Tx-wats](https://github.com/Tx-wats) organization.

## Contracts

| Contract | Description |
|---|---|
| [Alert Registry](contracts/alert-registry/src/lib.rs) | Stores alert configs on-chain keyed by contract address |
| [Watcher Registry](contracts/watcher-registry/src/lib.rs) | Stores authorized watcher node addresses |

## Quick Start

```bash
# Install Rust + Soroban target
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown

# Build
cargo build --release --target wasm32-unknown-unknown

# Test
cargo test

# Generate TypeScript bindings
make bindings
```

## TypeScript Bindings

TypeScript bindings for the AlertRegistry contract live in
[bindings/alert-registry/](bindings/alert-registry/) — see its
[README](bindings/alert-registry/README.md) for usage examples.

Bindings are published to npm as `@tx-wat/alert-registry-bindings` by the
[publish-bindings workflow](.github/workflows/publish-bindings.yml) when a GitHub release is
tagged. Until the first tagged release, generate them locally:

```bash
make bindings
```

## Architecture

```mermaid
flowchart TD
    subgraph Stellar["Stellar Network (on-chain)"]
        AR["AlertRegistry\n─────────────\nstores alert configs\nkeyed by contract address"]
        WR["WatcherRegistry\n─────────────\nstores authorized\nwatcher addresses"]
    end

    subgraph OffChain["Off-chain (tx-watch-core)"]
        W["Watcher Node\n─────────────\npolls Horizon\nmatches rules\nfires webhooks"]
    end

    Owner["Owner"] -->|"register_alert / update_alert"| AR
    Admin["Admin"] -->|"register_watcher / remove_watcher"| WR

    W -->|"is_authorized(watcher)"| WR
    W -->|"get_alerts_for_contract(target)"| AR
    Horizon["Horizon API"] -->|"GET /accounts/{id}/transactions"| W
    W -->|"POST webhook URL"| Endpoint["Downstream\nIntegration"]
```

**Data flow:**

1. An owner registers an alert in `AlertRegistry` — specifying the target contract, rules, and a hashed webhook URL.
2. Authorized watcher nodes are recorded in `WatcherRegistry` by an admin.
3. A watcher node polls Horizon for transaction activity, fetches matching alert configs from `AlertRegistry`, and checks whether any rule matches.
4. On a match the watcher fires the configured webhook so downstream integrations can react.

---

## How it works

The system is centered around three data-flow steps:

1. An owner registers an alert in `AlertRegistry` with the contract address, labels, webhook hash, and rules.
2. Authorized watcher nodes poll Horizon for transaction activity, then check the stored alert definitions in `AlertRegistry` to determine whether a watched contract event matches.
3. When a match is found, the watcher fires the configured webhook so downstream integrations can react.

This keeps alert configuration on-chain while letting watcher nodes perform off-chain polling and delivery.

## Stellar Integration

### Network Configuration

```toml
# Testnet
rpc_url       = "https://soroban-testnet.stellar.org"
passphrase    = "Test SDF Network ; September 2015"
horizon_url   = "https://horizon-testnet.stellar.org"

# Mainnet
rpc_url       = "https://mainnet.stellar.validationcloud.io/v1/<API_KEY>"
passphrase    = "Public Global Stellar Network ; September 2015"
horizon_url   = "https://horizon.stellar.org"
```

### Invoking Contracts (Stellar CLI)

Below is a consolidated cheatsheet with copy-pasteable `stellar contract invoke` examples for every public function across both contracts, grouped to mirror the reference documentation in [docs/alert-registry.md](docs/alert-registry.md) and [docs/watcher-registry.md](docs/watcher-registry.md).

#### Alert Registry (`ALERT_REGISTRY_CONTRACT_ID`)

**Admin & Configuration:**

```bash
# Initialize contract admin (one-time)
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- initialize \
  --admin <ADMIN_ADDRESS>

# Transfer admin role
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- transfer_admin \
  --admin <ADMIN_ADDRESS> \
  --new_admin <NEW_ADMIN_ADDRESS>

# Get current admin
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_admin

# Set per-owner alert limit (0 = unlimited)
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- set_per_owner_alert_limit \
  --admin <ADMIN_ADDRESS> \
  --limit 10

# Get configured per-owner alert limit
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_per_owner_alert_limit

# Configure Watcher Registry address for read gating
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- set_watcher_registry \
  --admin <ADMIN_ADDRESS> \
  --watcher_registry <WATCHER_REGISTRY_CONTRACT_ID>

# Get configured Watcher Registry address
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_watcher_registry

# Check if watcher gating is enabled
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- is_watcher_gating_enabled
```

**Alert Mutations:**

```bash
# Register a new alert config
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <OWNER_IDENTITY> \
  --network testnet \
  -- register_alert \
  --owner <OWNER_ADDRESS> \
  --target_contract <WATCHED_CONTRACT_ADDRESS> \
  --label "My Alert" \
  --webhook_hash "<sha256-of-webhook-url>" \
  --rules '["rule:transfer","rule:mint"]'

# Update alert rules and active flag
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <OWNER_IDENTITY> \
  --network testnet \
  -- update_alert \
  --caller <OWNER_ADDRESS> \
  --config_id 1 \
  --rules '["rule:transfer"]' \
  --active true

# Update alert label
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <OWNER_IDENTITY> \
  --network testnet \
  -- update_label \
  --caller <OWNER_ADDRESS> \
  --config_id 1 \
  --label "Updated Alert Label"

# Update webhook hash directly
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <OWNER_IDENTITY> \
  --network testnet \
  -- update_webhook \
  --caller <OWNER_ADDRESS> \
  --config_id 1 \
  --webhook_hash "<new-sha256-hash>"

# Propose new webhook hash (step 1 of 2-step rotation)
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <OWNER_IDENTITY> \
  --network testnet \
  -- propose_webhook \
  --caller <OWNER_ADDRESS> \
  --config_id 1 \
  --webhook_hash "<staged-sha256-hash>"

# Confirm new webhook hash (step 2 of 2-step rotation)
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <OWNER_IDENTITY> \
  --network testnet \
  -- confirm_webhook \
  --caller <OWNER_ADDRESS> \
  --config_id 1

# Renew alert TTL (owner-authenticated, preserves updated_at)
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <OWNER_IDENTITY> \
  --network testnet \
  -- renew_alert_ttl \
  --caller <OWNER_ADDRESS> \
  --config_id 1

# Bump alert TTL (unauthenticated / keeper service)
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <CALLER_IDENTITY> \
  --network testnet \
  -- bump_alert \
  --config_id 1 \
  --ttl 535680

# Update target contract for an alert
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <OWNER_IDENTITY> \
  --network testnet \
  -- update_target_contract \
  --caller <OWNER_ADDRESS> \
  --config_id 1 \
  --new_target <NEW_WATCHED_CONTRACT_ADDRESS>

# Deactivate all alerts owned by caller
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <OWNER_IDENTITY> \
  --network testnet \
  -- deactivate_all_alerts \
  --caller <OWNER_ADDRESS>

# Remove an alert (owner only)
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <OWNER_IDENTITY> \
  --network testnet \
  -- remove_alert \
  --caller <OWNER_ADDRESS> \
  --config_id 1

# Remove an alert by admin (admin only)
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- remove_alert_by_admin \
  --admin <ADMIN_ADDRESS> \
  --config_id 1
```

**Alert Queries & Inspection:**

```bash
# Retrieve a single alert config by ID
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_alert \
  --config_id 1

# Check if an alert is active (lightweight read)
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_alert_active \
  --config_id 1

# Query all alerts for a contract
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_alerts_for_contract \
  --querier <QUERIER_ADDRESS> \
  --target_contract <WATCHED_CONTRACT_ADDRESS>

# Query active alerts for a contract
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_active_alerts_for_contract \
  --target_contract <WATCHED_CONTRACT_ADDRESS>

# Query all alerts owned by an address
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_alerts_by_owner \
  --querier <QUERIER_ADDRESS> \
  --owner <OWNER_ADDRESS>

# Query alerts for a contract with pagination
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_contract_alerts_paginated \
  --querier <QUERIER_ADDRESS> \
  --target_contract <WATCHED_CONTRACT_ADDRESS> \
  --offset 0 \
  --limit 10

# Query alerts by owner with pagination
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_alerts_by_owner_paginated \
  --querier <QUERIER_ADDRESS> \
  --owner <OWNER_ADDRESS> \
  --offset 0 \
  --limit 10

# Query alerts modified since ledger timestamp (incremental sync)
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_alerts_modified_since \
  --since 1700000000

# Get total cumulative alert count
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_alert_count

# Get active alert count for an owner
stellar contract invoke \
  --id <ALERT_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_active_alert_count \
  --owner <OWNER_ADDRESS>
```

#### Watcher Registry (`WATCHER_REGISTRY_CONTRACT_ID`)

**Admin & Governance:**

```bash
# Initialize registry with initial admin (one-time)
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- initialize \
  --admin <ADMIN_ADDRESS>

# Add a co-admin
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- add_admin \
  --caller <ADMIN_ADDRESS> \
  --new_admin <NEW_ADMIN_ADDRESS>

# Remove an admin
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- remove_admin \
  --caller <ADMIN_ADDRESS> \
  --admin_to_remove <ADMIN_TO_REMOVE_ADDRESS>

# Transfer admin role (replaces admin set)
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- transfer_admin \
  --caller <ADMIN_ADDRESS> \
  --new_admin <NEW_ADMIN_ADDRESS>

# Get primary admin address
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_admin

# Get all current admin addresses
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_admins
```

**Watcher Management:**

```bash
# Register an authorized watcher
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- register_watcher \
  --admin <ADMIN_ADDRESS> \
  --watcher <WATCHER_ADDRESS>

# Remove an authorized watcher
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- remove_watcher \
  --admin <ADMIN_ADDRESS> \
  --watcher <WATCHER_ADDRESS>

# Replace an existing watcher with a new address
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- replace_watcher \
  --admin <ADMIN_ADDRESS> \
  --old_watcher <OLD_WATCHER_ADDRESS> \
  --new_watcher <NEW_WATCHER_ADDRESS>

# Clear all authorized watchers (bulk deauthorization)
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --source <ADMIN_IDENTITY> \
  --network testnet \
  -- clear_all_watchers \
  --admin <ADMIN_ADDRESS>
```

**Watcher Queries:**

```bash
# Check if an address is an authorized watcher
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- is_watcher_authorized \
  --watcher <WATCHER_ADDRESS>

# Check authorization (backward-compatible alias)
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- is_authorized \
  --watcher <WATCHER_ADDRESS>

# Get all authorized watcher addresses
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_watchers

# Get total count of authorized watchers
stellar contract invoke \
  --id <WATCHER_REGISTRY_CONTRACT_ID> \
  --network testnet \
  -- get_watcher_count
```

### Invoking Contracts (JavaScript SDK)

```js
import {
  Contract,
  SorobanRpc,
  TransactionBuilder,
  Networks,
  BASE_FEE,
  nativeToScVal,
  Address,
} from "@stellar/stellar-sdk";

const server = new SorobanRpc.Server("https://soroban-testnet.stellar.org");
const contract = new Contract("<ALERT_REGISTRY_CONTRACT_ID>");

// Build a register_alert transaction
const account = await server.getAccount(ownerKeypair.publicKey());
const tx = new TransactionBuilder(account, {
  fee: BASE_FEE,
  networkPassphrase: Networks.TESTNET,
})
  .addOperation(
    contract.call(
      "register_alert",
      new Address(ownerKeypair.publicKey()).toScVal(),          // owner
      new Address("<WATCHED_CONTRACT_ADDRESS>").toScVal(),      // target_contract
      nativeToScVal("My Alert", { type: "string" }),            // label
      nativeToScVal("<sha256-of-webhook-url>", { type: "string" }), // webhook_hash
      nativeToScVal(["rule:transfer", "rule:mint"], { type: "array", element: { type: "string" } }), // rules
    )
  )
  .setTimeout(30)
  .build();

const preparedTx = await server.prepareTransaction(tx);
preparedTx.sign(ownerKeypair);
const result = await server.sendTransaction(preparedTx);
console.log("Transaction hash:", result.hash);
```

```js
import {
  Contract,
  SorobanRpc,
  TransactionBuilder,
  Networks,
  BASE_FEE,
  Address,
} from "@stellar/stellar-sdk";

const server = new SorobanRpc.Server("https://soroban-testnet.stellar.org");
const contract = new Contract("<WATCHER_REGISTRY_CONTRACT_ID>");

// Initialize the registry (one-time, admin only)
const account = await server.getAccount(adminKeypair.publicKey());
const initTx = new TransactionBuilder(account, {
  fee: BASE_FEE,
  networkPassphrase: Networks.TESTNET,
})
  .addOperation(
    contract.call(
      "initialize",
      new Address(adminKeypair.publicKey()).toScVal(), // admin
    )
  )
  .setTimeout(30)
  .build();

const preparedInit = await server.prepareTransaction(initTx);
preparedInit.sign(adminKeypair);
await server.sendTransaction(preparedInit);

// Register a watcher node (admin only)
const account2 = await server.getAccount(adminKeypair.publicKey());
const registerTx = new TransactionBuilder(account2, {
  fee: BASE_FEE,
  networkPassphrase: Networks.TESTNET,
})
  .addOperation(
    contract.call(
      "register_watcher",
      new Address(adminKeypair.publicKey()).toScVal(),   // admin
      new Address("<WATCHER_NODE_ADDRESS>").toScVal(),   // watcher
    )
  )
  .setTimeout(30)
  .build();

const preparedRegister = await server.prepareTransaction(registerTx);
preparedRegister.sign(adminKeypair);
await server.sendTransaction(preparedRegister);

// Check if an address is an authorized watcher (read-only, no signature needed)
const account3 = await server.getAccount(adminKeypair.publicKey());
const checkTx = new TransactionBuilder(account3, {
  fee: BASE_FEE,
  networkPassphrase: Networks.TESTNET,
})
  .addOperation(
    contract.call(
      "is_authorized",
      new Address("<WATCHER_NODE_ADDRESS>").toScVal(), // watcher
    )
  )
  .setTimeout(30)
  .build();

const result = await server.simulateTransaction(checkTx);
console.log("Is authorized:", result.result?.retval); // SCV_BOOL
```

### Invoking Contracts (Rust SDK)

```rust
use soroban_sdk::{Address, Env, String, Vec};

// In a cross-contract call context:
let alert_registry = AlertRegistryClient::new(&env, &alert_registry_id);
let config_id = alert_registry.register_alert(
    &owner,
    &target_contract,
    &String::from_str(&env, "My Alert"),
    &String::from_str(&env, "<webhook-hash>"),
    &rules,
);
```

> **Re-entrancy safety:** Soroban executes contract calls atomically and prevents classic callback-based re-entrancy within the same transaction. The registry contracts only mutate local storage after `require_auth()` succeeds, and they do not invoke external contracts during state updates.

### Auth Flow

All mutating functions require Stellar auth signatures:

```
Owner signs → register_alert / update_alert / remove_alert
Admin signs → register_watcher / remove_watcher / transfer_admin
```

Stellar's `require_auth()` enforces this at the protocol level — no custom signature verification needed.

### Event Indexing (planned)

Contracts emit no custom events yet. Watchers poll via Horizon's transaction endpoint:

```
GET https://horizon-testnet.stellar.org/accounts/<CONTRACT_ID>/transactions
```

Future versions will emit `soroban_sdk::events` for real-time indexing.

## TypeScript Bindings

TypeScript bindings for `WatcherRegistry` are published to npm and generated
automatically from the compiled WASM on every release using
`stellar contract bindings typescript`.

```bash
npm install @tx-wat/watcher-registry @stellar/stellar-sdk
```

> **Note:** the npm packages are published by CI on the first tagged release. Until then, generate the bindings locally with `make bindings` from the repository root.

```typescript
import { Client, networks } from "@tx-wat/watcher-registry";

const client = new Client({
  contractId: networks.testnet.contractId,
  networkPassphrase: networks.testnet.networkPassphrase,
  rpcUrl: networks.testnet.rpcUrl,
});

const authorized = await client.is_authorized({ watcher: "GABC...XYZ" });
console.log(authorized.result); // true | false
```

See [bindings/watcher-registry/README.md](bindings/watcher-registry/README.md)
for the full API reference and usage examples.

## Deployed Addresses

See [DEPLOYMENTS.md](DEPLOYMENTS.md).

## Docs

- [Alert Registry function reference](docs/alert-registry.md)
- [Watcher Registry function reference](docs/watcher-registry.md)
- [Upgrade guide](docs/upgrade-guide.md)
- [Compatibility Matrix](docs/compatibility.md)
- [Ecosystem submission guide](docs/ecosystem-submission.md)

## Compatibility Matrix

| Contract Tag / Version | Soroban SDK | npm Bindings | Core Engine | Web Dashboard | Status |
|---|---|---|---|---|---|
| **`v0.1.0`** | 22.0.0 | `^0.1.0` | `^0.1.0` | `^0.1.0` | Stable |
| **`v0.2.0`** *(main)* | 22.0.0 | `^0.2.0` | `^0.2.0` | `^0.2.0` | Active Development |

See [docs/compatibility.md](docs/compatibility.md) for the detailed compatibility matrix, policy, and release checklist.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Sister Repos

- **Core engine:** https://github.com/Tx-wat/stellar-txwatch-core
- **Web dashboard:** https://github.com/Tx-wat/stellar-txwatch-web

## License

MIT

