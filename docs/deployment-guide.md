# Deployment & Testnet Liveness Guide

This guide describes how TxWatch contracts are deployed, monitored for liveness on Stellar Testnet, and recovered when periodic network resets occur.

---

## 1. Overview of Deployed Contracts

TxWatch maintains two core Soroban smart contracts:

1. **`AlertRegistry`**: Manages on-chain alert configurations, two-phase webhook rotations, and rule subscriptions.
2. **`WatcherRegistry`**: Manages authorized watcher node identities and multi-admin controls.

Tracked deployment addresses and network parameters are recorded in [`DEPLOYMENTS.md`](../DEPLOYMENTS.md).

---

## 2. Testnet Resets & Address Expiration

### Why Testnet Resets Happen
The Stellar Development Foundation (SDF) periodically resets the Stellar Testnet (typically once per quarter or as announced) to clean up ledger state and maintain testnet performance.

When a reset occurs:
- All contract bytecode, storage entries, accounts, and balances on the testnet ledger are **permanently wiped**.
- Existing contract IDs (e.g. `CDSO4...`, `CCSHR...`) become non-existent and will return `404 Not Found` upon query.
- Addresses documented in `DEPLOYMENTS.md` become stale until redeployed.

---

## 3. Automated Liveness Monitoring

To prevent developers and integration pipelines from encountering silent failures from expired testnet contracts:

### A. Scheduled CI Check
A GitHub Actions workflow ([`.github/workflows/check-testnet-deployments.yml`](../.github/workflows/check-testnet-deployments.yml)) runs daily at `00:00 UTC` (and on manual `workflow_dispatch`):
1. Queries the Soroban RPC `getHealth` endpoint.
2. Extracts active testnet contract addresses from `DEPLOYMENTS.md`.
3. Verifies that each contract is live on the testnet ledger.
4. Fails loudly with remediation instructions if any address is missing.

### B. Local CLI Check
You can run the liveness check locally at any time:

```bash
./scripts/check-testnet-deployments.sh
```

Sample output when contracts are live:
```
=======================================================
TxWatch Testnet Deployment Liveness Check
=======================================================
RPC URL:          https://soroban-testnet.stellar.org
Deployments File: DEPLOYMENTS.md

--> Checking Soroban Testnet RPC health...
    RPC is healthy. Latest ledger: 4362672

--> Extracting testnet contract addresses from DEPLOYMENTS.md...
    Found 2 testnet contract(s) to verify.

--> Checking Alert Registry (CDSO4GGZH7KBUQYKOIQDCMCFSRYEPOVDUX7Z4IB5TWNTLT2GDRKDQOYR)...
    [LIVE] Contract exists on testnet ledger.
--> Checking Watcher Registry (CCSHRYACRNVSLC5NP3V2DL6LGID57TQT2TJXVUVXBBZX6SED6N3F7X6J)...
    [LIVE] Contract exists on testnet ledger.

=======================================================
✅ ALL TESTNET CONTRACT ADDRESSES ARE LIVE
=======================================================
```

---

## 4. Remediation: Recovery from a Testnet Reset

When the liveness check fails or an SDF testnet reset occurs, follow these steps to redeploy and update all systems:

### Step 1: Fund Deployer Account
Ensure your Stellar deployer key is funded on testnet via Friendbot:

```bash
# If using Stellar CLI
stellar keys fund deployer --network testnet

# Or via Friendbot curl
curl "https://friendbot.stellar.org?addr=$(stellar keys address deployer)"
```

### Step 2: Build and Deploy Contracts
Run the deployment script:

```bash
bash scripts/deploy.sh
```

This will:
- Compile `alert-registry` and `watcher-registry` to optimized `wasm32-unknown-unknown` binaries.
- Install the WASM bytecode onto Testnet.
- Instantiate and initialize `AlertRegistry` and `WatcherRegistry`.
- Print the newly assigned contract addresses and WASM hashes.

### Step 3: Verify Deployed WASM Hashes
Confirm that deployed bytecode matches the local build:

```bash
./scripts/verify.sh --contract alert-registry --contract-id <NEW_ALERT_ID> --network testnet
./scripts/verify.sh --contract watcher-registry --contract-id <NEW_WATCHER_ID> --network testnet
```

### Step 4: Update `DEPLOYMENTS.md`
Update the table in [`DEPLOYMENTS.md`](../DEPLOYMENTS.md) with the new contract IDs and WASM hashes:

```markdown
## Stellar Testnet

| Contract | Address | WASM Hash |
|---|---|---|
| Alert Registry | `<NEW_ALERT_ID>` | `<NEW_WASM_HASH>` |
| Watcher Registry | `<NEW_WATCHER_ID>` | `<NEW_WASM_HASH>` |
```

Commit and push the update:
```bash
git add DEPLOYMENTS.md
git commit -m "deploy: update testnet addresses after network reset"
git push
```

### Step 5: Update TypeScript Bindings & Sister Repositories
1. Re-generate TypeScript bindings:
   ```bash
   stellar contract bindings typescript \
     --wasm target/wasm32-unknown-unknown/release/alert_registry.wasm \
     --output-dir bindings/alert-registry \
     --overwrite

   stellar contract bindings typescript \
     --wasm target/wasm32-unknown-unknown/release/watcher_registry.wasm \
     --output-dir bindings/watcher-registry \
     --overwrite
   ```
2. Update the contract addresses in `stellar-txwatch-core` and `stellar-txwatch-web` environment configs (`.env` / configuration constants).

---

## 5. Mainnet Deployments

Mainnet contracts are permanent and do not reset. Follow the release procedure outlined in [`docs/compatibility.md`](compatibility.md) and [`CONTRIBUTING.md`](../CONTRIBUTING.md) when deploying to mainnet.
