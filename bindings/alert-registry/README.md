# @tx-wat/alert-registry-bindings

TypeScript bindings for the AlertRegistry Soroban smart contract.

## Installation

```bash
npm install @tx-wat/alert-registry-bindings
```

> **Note:** the npm packages are published by CI on the first tagged release. Until then, generate the bindings locally with `make bindings` from the repository root.

## Usage

```typescript
import { Contract, networks } from '@tx-wat/alert-registry-bindings';
import { SorobanRpc, Keypair } from '@stellar/stellar-sdk';

const rpcUrl = 'https://soroban-testnet.stellar.org';
const contractId = 'YOUR_CONTRACT_ID';
const keypair = Keypair.fromSecret('YOUR_SECRET_KEY');

const contract = new Contract({
  contractId,
  networkPassphrase: networks.testnet.networkPassphrase,
  rpcUrl,
});

// Register an alert
const result = await contract.register_alert({
  owner: keypair.publicKey(),
  target_contract: 'CONTRACT_ADDRESS_TO_WATCH',
  label: 'My Alert',
  webhook_hash: 'sha256_hash_of_webhook_url',
  rules: ['rule:transfer', 'rule:mint'],
}, {
  keypair,
});

console.log('Alert ID:', result);

// Get alerts for a contract
const alerts = await contract.get_alerts_for_contract({
  target_contract: 'CONTRACT_ADDRESS',
});

console.log('Alerts:', alerts);

// Get paginated alerts
const paginatedAlerts = await contract.get_alerts_by_owner_paginated({
  owner: keypair.publicKey(),
  offset: 0,
  limit: 10,
});

console.log('Paginated alerts:', paginatedAlerts);
```

## API

The bindings provide TypeScript types and methods for all AlertRegistry contract functions:

### Write Methods (require authentication)
- `register_alert` - Register a new alert configuration
- `update_alert` - Update rules and active status
- `update_webhook` - Update webhook hash
- `remove_alert` - Remove an alert
- `initialize` - Initialize the contract with an admin (one-time)
- `transfer_admin` - Transfer admin role
- `set_per_owner_alert_limit` - Set per-owner alert limit (admin only)
- `remove_alert_by_admin` - Remove any alert (admin only)

### Read Methods
- `get_alert` - Get a single alert by ID
- `get_alerts_for_contract` - Get all alerts for a contract
- `get_alerts_by_owner` - Get all alerts owned by an address
- `get_contract_alerts_paginated` - Get paginated alerts for a contract
- `get_alerts_by_owner_paginated` - Get paginated alerts by owner
- `get_alert_count` - Get total number of alerts ever registered
- `get_active_alert_count` - Get number of active alerts for an owner
- `get_admin` - Get the current admin address
- `get_per_owner_alert_limit` - Get the per-owner alert limit

## Building from Source

```bash
# Build the contract first
cd ../..
cargo build --release --target wasm32-unknown-unknown

# Generate bindings
cd bindings/alert-registry
npm install
npm run build
```

## License

MIT
