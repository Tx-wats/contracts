#!/usr/bin/env bash
# scripts/deploy.sh — Deploy both contracts to Stellar (testnet or mainnet)
# Usage: ./scripts/deploy.sh [--network testnet|mainnet]
#        NETWORK=mainnet ./scripts/deploy.sh
set -euo pipefail

# shellcheck source=scripts/lib/common.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/common.sh"

parse_args "$@"
resolve_network

IDENTITY="${STELLAR_IDENTITY:-deployer}"

echo "==> Network: $NETWORK"
echo "==> Checking Stellar CLI..."
stellar --version

if [[ "$NETWORK" == "testnet" ]]; then
  echo "==> Checking testnet key..."
  if ! stellar keys address "$IDENTITY" &>/dev/null; then
    echo "==> Generating missing identity $IDENTITY on testnet..."
    stellar keys generate "$IDENTITY" --network "$NETWORK"
  else
    echo "==> Using existing identity $IDENTITY"
  fi
  echo "==> Ensuring account is funded on testnet..."
  stellar keys fund "$IDENTITY" --network "$NETWORK" 2>/dev/null || true
fi

build_contract "${ALL_CONTRACTS[@]}"

ALERT_WASM="$(wasm_path alert-registry)"
WATCHER_WASM="$(wasm_path watcher-registry)"

ADMIN_ADDRESS=$(stellar keys address "$IDENTITY")

echo "==> Deploying Alert Registry..."
ALERT_ID=$(stellar contract deploy \
  --wasm "$ALERT_WASM" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" \
  -- --admin "$ADMIN_ADDRESS")
echo "Alert Registry deployed: $ALERT_ID"

echo "==> Deploying Watcher Registry..."
WATCHER_ID=$(stellar contract deploy \
  --wasm "$WATCHER_WASM" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE")
echo "Watcher Registry deployed: $WATCHER_ID"

ADMIN_ADDRESS="${ADMIN_ADDRESS:-$(stellar keys address "$IDENTITY")}"

echo "==> Initializing Watcher Registry..."
stellar contract invoke \
  --id "$WATCHER_ID" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" \
  -- --admin "$ADMIN_ADDRESS")
echo "Watcher Registry deployed: $WATCHER_ID"

ALERT_HASH=$(sha256sum "$ALERT_WASM" | awk '{print $1}')
WATCHER_HASH=$(sha256sum "$WATCHER_WASM" | awk '{print $1}')

echo ""
echo "==> Deployment complete ($NETWORK). Update DEPLOYMENTS.md with:"
echo "    Alert Registry:   $ALERT_ID"
echo "    Watcher Registry: $WATCHER_ID"
echo ""
echo "==> WASM hashes (copy into DEPLOYMENTS.md):"
echo "    Alert Registry:   $ALERT_HASH"
echo "    Watcher Registry: $WATCHER_HASH"
