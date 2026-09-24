#!/usr/bin/env bash
# scripts/upgrade.sh — Upgrade a deployed Stellar contract to a new WASM binary
# Usage: ./scripts/upgrade.sh --contract alert-registry|watcher-registry --contract-id <ID> [--network testnet|mainnet]
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
build_contract "$CONTRACT"

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
