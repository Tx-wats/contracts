#!/usr/bin/env bash
# scripts/upgrade.sh — Upgrade a deployed Stellar contract to a new WASM binary
# Usage: ./scripts/upgrade.sh --contract alert-registry|watcher-registry --contract-id <ID> [--network testnet|mainnet]
set -euo pipefail

NETWORK="${NETWORK:-testnet}"
CONTRACT=""
CONTRACT_ID=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --network)     NETWORK="$2";     shift 2 ;;
    --contract)    CONTRACT="$2";    shift 2 ;;
    --contract-id) CONTRACT_ID="$2"; shift 2 ;;
    *) echo "Unknown argument: $1"; exit 1 ;;
  esac
done

[[ -z "$CONTRACT" ]]    && { echo "Error: --contract required (alert-registry|watcher-registry)"; exit 1; }
[[ -z "$CONTRACT_ID" ]] && { echo "Error: --contract-id required"; exit 1; }

case "$NETWORK" in
  testnet)
    RPC_URL="https://soroban-testnet.stellar.org"
    NETWORK_PASSPHRASE="Test SDF Network ; September 2015"
    ;;
  mainnet)
    RPC_URL="${MAINNET_RPC_URL:?MAINNET_RPC_URL must be set for mainnet}"
    NETWORK_PASSPHRASE="Public Global Stellar Network ; September 2015"
    ;;
  *)
    echo "Unsupported network: $NETWORK"; exit 1 ;;
esac

IDENTITY="${STELLAR_IDENTITY:-deployer}"
# `upgrade` is admin-gated, so the invocation needs the admin's address.
ADMIN="${ADMIN_ADDRESS:-$(stellar keys address "$IDENTITY")}"
WASM_NAME="${CONTRACT//-/_}"
WASM="target/wasm32-unknown-unknown/release/${WASM_NAME}.wasm"

echo "==> Network: $NETWORK"
echo "==> Building contracts..."
cargo build --release --target wasm32-unknown-unknown

echo "==> Installing new WASM on-chain..."
NEW_WASM_HASH=$(stellar contract install \
  --wasm "$WASM" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE")
echo "New WASM hash: $NEW_WASM_HASH"

echo "==> Upgrading contract $CONTRACT_ID..."
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" \
  -- upgrade \
  --admin "$ADMIN" \
  --new_wasm_hash "$NEW_WASM_HASH"

echo "==> Upgrade complete. Contract $CONTRACT_ID now runs WASM $NEW_WASM_HASH"
