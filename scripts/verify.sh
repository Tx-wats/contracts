#!/usr/bin/env bash
# scripts/verify.sh — Verify deployed contract WASM hash matches local build
# Usage: ./scripts/verify.sh --contract alert-registry|watcher-registry --contract-id <ID> [--network testnet|mainnet]
set -euo pipefail

# shellcheck source=scripts/lib/common.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/common.sh"

parse_args "$@"
require_contract_args
resolve_network

WASM="$(wasm_path "$CONTRACT")"

echo "==> Network: $NETWORK"
build_contract "$CONTRACT"

# Compute local WASM hash (sha256, hex only)
LOCAL_HASH=$(sha256sum "$WASM" | awk '{print $1}')
echo "==> Local WASM hash:    $LOCAL_HASH"

# Fetch deployed WASM hash via Stellar CLI
DEPLOYED_HASH=$(stellar contract info \
  --id "$CONTRACT_ID" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" \
  | grep -i "wasm hash" | awk '{print $NF}' | tr -d '"')
echo "==> Deployed WASM hash: $DEPLOYED_HASH"

if [[ "$LOCAL_HASH" == "$DEPLOYED_HASH" ]]; then
  echo "==> MATCH: deployed contract matches local build."
else
  echo "==> MISMATCH: deployed contract does NOT match local build!"
  exit 1
fi
