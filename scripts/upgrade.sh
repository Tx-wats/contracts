#!/usr/bin/env bash
# scripts/upgrade.sh — Upgrade a deployed Stellar contract to a new WASM binary
# Usage: ./scripts/upgrade.sh --contract alert-registry|watcher-registry --contract-id <ID> [--network testnet|mainnet] [--yes]
set -euo pipefail

# shellcheck source=scripts/lib/common.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/common.sh"

parse_args "$@"
require_contract_args
resolve_network

IDENTITY="${STELLAR_IDENTITY:-deployer}"
# `upgrade` is admin-gated, so the invocation needs the admin's address.
ADMIN="${ADMIN_ADDRESS:-$(stellar keys address "$IDENTITY")}"
WASM="$(wasm_path "$CONTRACT")"

echo "==> Network: $NETWORK"
echo "==> Upgrading contract: $CONTRACT ($CONTRACT_ID)"
echo "==> Identity: $IDENTITY ($ADMIN)"

# Build only the target crate with -p and --locked via common build_contract helper
build_contract "$CONTRACT"

echo "==> Fetching current deployed WASM hash..."
CURRENT_WASM_HASH=$(stellar contract info \
  --id "$CONTRACT_ID" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" 2>/dev/null \
  | grep -i "wasm hash" | awk '{print $NF}' | tr -d '"' || echo "unknown")

echo "==> Uploading new WASM on-chain..."
NEW_WASM_HASH=$(stellar contract upload \
  --wasm "$WASM" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE")

echo ""
echo "==> Contract WASM Hash Summary:"
echo "    Contract ID:       $CONTRACT_ID"
echo "    Current WASM Hash: $CURRENT_WASM_HASH"
echo "    New WASM Hash:     $NEW_WASM_HASH"
echo ""

if [[ "$NETWORK" == "mainnet" ]]; then
  if [ "$ASSUME_YES" = true ]; then
    echo "==> Mainnet upgrade confirmed via --yes flag."
  else
    echo "======================================================================"
    echo "WARNING: You are about to upgrade a production contract on MAINNET!"
    echo "  Contract:          $CONTRACT ($CONTRACT_ID)"
    echo "  Current WASM Hash: $CURRENT_WASM_HASH"
    echo "  New WASM Hash:     $NEW_WASM_HASH"
    echo "======================================================================"
    read -r -p "Type 'yes' or 'CONFIRM' to proceed with the mainnet upgrade: " CONFIRMATION
    if [[ "$CONFIRMATION" != "yes" && "$CONFIRMATION" != "CONFIRM" ]]; then
      echo "==> Upgrade cancelled by user."
      exit 1
    fi
  fi
fi

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
