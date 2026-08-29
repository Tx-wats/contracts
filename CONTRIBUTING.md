# Contributing

## Prerequisites

Install the Rust toolchain and Soroban target:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown
```

Install the Stellar CLI:

```bash
cargo install --locked stellar-cli --features opt
```

## Build

```bash
cargo build --release --target wasm32-unknown-unknown
```

## Test

```bash
cargo test
```

Tests run natively (no WASM target needed). Each contract has a `#[cfg(test)]` module covering happy paths, unauthorized rejections, and edge cases.

## Deploy to Testnet

1. Set up a funded testnet identity:

```bash
stellar keys generate deployer --network testnet
stellar keys fund deployer --network testnet
```

2. Run the deploy script:

```bash
bash scripts/deploy.sh
```

3. Update `DEPLOYMENTS.md` with the printed contract addresses.

## Verifying a Deployment

Verify that a deployed on-chain contract matches the locally compiled WASM binary:

1. Run the verification script against the target contract and network:

```bash
# Verify Alert Registry on Testnet
bash scripts/verify.sh --contract alert-registry --contract-id <ALERT_REGISTRY_CONTRACT_ID> --network testnet

# Verify Watcher Registry on Testnet
bash scripts/verify.sh --contract watcher-registry --contract-id <WATCHER_REGISTRY_CONTRACT_ID> --network testnet
```

2. For mainnet verification, ensure `MAINNET_RPC_URL` is exported:

```bash
export MAINNET_RPC_URL="https://mainnet.stellar.validationcloud.io/v1/<API_KEY>"
bash scripts/verify.sh --contract alert-registry --contract-id <ALERT_REGISTRY_CONTRACT_ID> --network mainnet
```

The script compiles the contracts locally in release mode, calculates the local SHA-256 hash, retrieves the deployed WASM hash from the network via Stellar CLI, and asserts that they match.

## Upgrading a Deployed Contract

Upgrade an already-deployed contract to a new WASM binary:

1. Ensure the deployer identity is configured and funded (defaults to `deployer`, or customize via `STELLAR_IDENTITY`):

```bash
export STELLAR_IDENTITY=deployer
```

2. Run the upgrade script for the target contract:

```bash
# Upgrade Alert Registry
bash scripts/upgrade.sh --contract alert-registry --contract-id <ALERT_REGISTRY_CONTRACT_ID> --network testnet

# Upgrade Watcher Registry
bash scripts/upgrade.sh --contract watcher-registry --contract-id <WATCHER_REGISTRY_CONTRACT_ID> --network testnet
```

3. For mainnet upgrades, export `MAINNET_RPC_URL`:

```bash
export MAINNET_RPC_URL="https://mainnet.stellar.validationcloud.io/v1/<API_KEY>"
bash scripts/upgrade.sh --contract alert-registry --contract-id <ALERT_REGISTRY_CONTRACT_ID> --network mainnet
```

The script builds the contract locally, installs the new WASM on-chain via `stellar contract install`, and invokes the contract's `upgrade` function with the new WASM hash.

4. Update `DEPLOYMENTS.md` with the new WASM hash and version details.

## Adding a New Function to an Existing Contract

1. Add the function inside the `#[contractimpl]` block in `contracts/<name>/src/lib.rs`.
2. If it mutates state, call `<caller>.require_auth()` as the first line.
3. Add at least one test in the `#[cfg(test)]` module covering the happy path and any auth rejection.
4. Run `cargo test` to confirm everything passes.
5. Update the relevant doc in `docs/`.

## Sister Repos
 
- **Core engine:** https://github.com/Tx-wat/stellar-txwatch-core
- **Web dashboard:** https://github.com/Tx-wat/stellar-txwatch-web

See [docs/compatibility.md](docs/compatibility.md) for the cross-repository compatibility matrix and release checklist.

