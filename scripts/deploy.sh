#!/usr/bin/env bash
# scripts/deploy.sh — Deploy both contracts to Stellar (testnet or mainnet)
# Usage: ./scripts/deploy.sh [--network testnet|mainnet]
#        NETWORK=mainnet ./scripts/deploy.sh
set -euo pipefail

# --- Network selection ---
NETWORK="${NETWORK:-testnet}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --network)
      NETWORK="$2"; shift 2 ;;
    *)
      echo "Unknown argument: $1"; exit 1 ;;
  esac
done

case "$NETWORK" in
  testnet)
    RPC_URL="https://soroban-testnet.stellar.org"
    NETWORK_PASSPHRASE="Test SDF Network ; September 2015"
    ;;
  mainnet)
    RPC_URL="${MAINNET_RPC_URL:?MAINNET_RPC_URL must be set for mainnet deployments}"
    NETWORK_PASSPHRASE="Public Global Stellar Network ; September 2015"
    ;;
  *)
    echo "Unsupported network: $NETWORK (use testnet or mainnet)"; exit 1 ;;
esac

IDENTITY="${STELLAR_IDENTITY:-deployer}"

echo "==> Network: $NETWORK"
echo "==> Checking Stellar CLI..."
stellar --version

if [[ "$NETWORK" == "testnet" ]]; then
  echo "==> Funding account on testnet..."
  stellar keys generate --overwrite "$IDENTITY" --network "$NETWORK" 2>/dev/null || true
  stellar keys fund "$IDENTITY" --network "$NETWORK"
fi

echo "==> Building contracts..."
# Only the contract crates: building the whole workspace for wasm32 pulls in
# test-utils, which force-enables soroban-sdk's std-only `testutils` feature.
cargo build --release --target wasm32-unknown-unknown --locked \
  -p alert-registry -p watcher-registry

# Rust 1.82+ emits the WebAssembly reference-types proposal, which the Soroban
# host rejects at upload ("reference-types not enabled"). `stellar contract
# optimize` runs wasm-opt, which lowers the module back into the accepted
# subset — so this step is required for deployability, not just size.
echo "==> Optimizing WASM for on-chain upload..."
for w in alert_registry watcher_registry; do
  stellar contract optimize --wasm "target/wasm32-unknown-unknown/release/$w.wasm"
done

ALERT_WASM="target/wasm32-unknown-unknown/release/alert_registry.optimized.wasm"
WATCHER_WASM="target/wasm32-unknown-unknown/release/watcher_registry.optimized.wasm"

echo "==> Deploying Alert Registry..."
ALERT_ID=$(stellar contract deploy \
  --wasm "$ALERT_WASM" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE")
echo "Alert Registry deployed: $ALERT_ID"

echo "==> Deploying Watcher Registry..."
WATCHER_ID=$(stellar contract deploy \
  --wasm "$WATCHER_WASM" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE")
echo "Watcher Registry deployed: $WATCHER_ID"

ADMIN_ADDRESS=$(stellar keys address "$IDENTITY")

echo "==> Initializing Watcher Registry..."
stellar contract invoke \
  --id "$WATCHER_ID" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" \
  -- initialize \
  --admin "$ADMIN_ADDRESS"

echo ""
echo "==> Deployment complete ($NETWORK). Update DEPLOYMENTS.md with:"
echo "    Alert Registry:   $ALERT_ID"
echo "    Watcher Registry: $WATCHER_ID"
