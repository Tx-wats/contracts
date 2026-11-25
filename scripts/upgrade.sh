#!/usr/bin/env bash
# scripts/upgrade.sh — Upgrade a deployed Stellar contract to a new WASM binary
# Usage: ./scripts/upgrade.sh --contract alert-registry|watcher-registry --contract-id <ID> [--network testnet|mainnet] [--yes]
# Usage:
#   Direct / Immediate upgrade (if timelock is disabled or alert-registry):
#     ./scripts/upgrade.sh --contract alert-registry|watcher-registry --contract-id <ID> [--network testnet|mainnet]
#   Timelocked upgrade - Step 1 (Propose):
#     ./scripts/upgrade.sh --contract watcher-registry --contract-id <ID> --propose [--network testnet|mainnet]
#   Timelocked upgrade - Step 2 (Execute after delay):
#     ./scripts/upgrade.sh --contract watcher-registry --contract-id <ID> --execute [--network testnet|mainnet]
set -euo pipefail

# shellcheck source=scripts/lib/common.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/common.sh"

PROPOSE=false
EXECUTE=false
NEW_WASM_HASH="${NEW_WASM_HASH:-}"

# Parse custom args before or along with common args
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --propose)
      PROPOSE=true
      shift
      ;;
    --execute)
      EXECUTE=true
      shift
      ;;
    --wasm-hash)
      NEW_WASM_HASH="${2:?--wasm-hash needs a value}"
      shift 2
      ;;
    *)
      ARGS+=("$1")
      shift
      ;;
  esac
done

parse_args "${ARGS[@]}"
require_contract_args
WASM="$(wasm_path "$CONTRACT")"
resolve_network

IDENTITY="${STELLAR_IDENTITY:-deployer}"
ADMIN="${ADMIN_ADDRESS:-$(stellar keys address "$IDENTITY")}"

echo "==> Network: $NETWORK"
echo "==> Upgrading contract: $CONTRACT ($CONTRACT_ID)"
echo "==> Identity: $IDENTITY ($ADMIN)"

# Build only the target crate with -p and --locked via common build_contract helper
WASM="$(wasm_path "$CONTRACT")"
build_contract "$CONTRACT"

echo "==> Fetching current deployed WASM hash..."
CURRENT_WASM_HASH=$(stellar contract info \
  --id "$CONTRACT_ID" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" 2>/dev/null \
  | grep -i "wasm hash" | awk '{print $NF}' | tr -d '"' || echo "unknown")

if [ "$EXECUTE" = true ]; then
  if [ "$CONTRACT" != "watcher-registry" ]; then
    echo "Error: --execute is only applicable to watcher-registry (which supports timelocked actions)" >&2
    exit 1
  fi
  # Execute only calls execute_admin_action: no build or upload, and the hash
  # that runs is the one that was proposed, not whatever --wasm-hash says.
  echo "==> Fetching pending timelocked action..."
  PENDING_ACTION=$(stellar contract invoke \
    --id "$CONTRACT_ID" \
    --source "$IDENTITY" \
    --network "$NETWORK" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- get_pending_action)
  NEW_WASM_HASH="$PENDING_ACTION"
  echo ""
  echo "==> Pending action to execute:"
  echo "    Contract ID:       $CONTRACT_ID"
  echo "    Current WASM Hash: $CURRENT_WASM_HASH"
  echo "    Pending action:    $PENDING_ACTION"
  echo ""
  confirm_mainnet "Contract: $CONTRACT ($CONTRACT_ID)" "Current WASM Hash: $CURRENT_WASM_HASH" "Pending action: $PENDING_ACTION"
  echo "==> Executing pending timelocked admin action on $CONTRACT_ID..."
  stellar contract invoke \
    --id "$CONTRACT_ID" \
    --source "$IDENTITY" \
    --network "$NETWORK" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- execute_admin_action \
    --caller "$ADMIN"
  echo "==> Timelocked admin action successfully executed on $CONTRACT_ID"
  exit 0
fi

# Build and upload only when no --wasm-hash / NEW_WASM_HASH was supplied.
if [ -z "$NEW_WASM_HASH" ]; then
  # Build only the target crate with -p and --locked via common build_contract helper
  build_contract "$CONTRACT"

  echo "==> Uploading new WASM on-chain..."
  NEW_WASM_HASH=$(stellar contract upload \
    --wasm "$WASM" \
    --source "$IDENTITY" \
    --network "$NETWORK" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE")
else
  echo "==> Using provided WASM hash; skipping build and upload."
fi

echo ""
echo "==> Contract WASM Hash Summary:"
echo "    Contract ID:       $CONTRACT_ID"
echo "    Current WASM Hash: $CURRENT_WASM_HASH"
echo "    New WASM Hash:     $NEW_WASM_HASH"
echo ""

confirm_mainnet "Contract: $CONTRACT ($CONTRACT_ID)" "Current WASM Hash: $CURRENT_WASM_HASH" "New WASM Hash: $NEW_WASM_HASH"

# For watcher-registry, check if timelock is enabled if --propose is not explicitly set
if [ "$PROPOSE" = false ] && [ "$CONTRACT" = "watcher-registry" ]; then
  echo "==> Checking timelock delay for watcher-registry..."
  DELAY=$(stellar contract invoke \
    --id "$CONTRACT_ID" \
    --source "$IDENTITY" \
    --network "$NETWORK" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- get_timelock_delay 2>/dev/null || echo "0")
  # Strip any quotes or whitespace
  DELAY=$(echo "$DELAY" | tr -d '"[:space:]')
  if [[ "$DELAY" =~ ^[0-9]+$ ]] && [ "$DELAY" -gt 0 ]; then
    echo "Notice: Contract $CONTRACT_ID has a configured timelock delay of $DELAY ledgers."
    echo "Direct upgrade will revert with TimelockRequired. Switching to --propose mode."
    PROPOSE=true
  fi
fi

if [ "$PROPOSE" = true ]; then
  if [ "$CONTRACT" != "watcher-registry" ]; then
    echo "Error: --propose is only applicable to watcher-registry" >&2
    exit 1
  fi
  echo "==> Proposing timelocked upgrade for $CONTRACT_ID with WASM hash $NEW_WASM_HASH..."
  READY_AT=$(stellar contract invoke \
    --id "$CONTRACT_ID" \
    --source "$IDENTITY" \
    --network "$NETWORK" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- propose_admin_action \
    --caller "$ADMIN" \
    --action "{\"Upgrade\":\"$NEW_WASM_HASH\"}")
  echo "==> Upgrade proposed! Action will be executable at ledger: $READY_AT"
  echo "Once that ledger has elapsed, execute the upgrade with:"
  echo "  $0 --contract watcher-registry --contract-id $CONTRACT_ID --network $NETWORK --execute"
  exit 0
fi

# Run an AlertRegistry function and strip quotes/whitespace from the result.
alert_invoke() {
  stellar contract invoke \
    --id "$CONTRACT_ID" \
    --source "$IDENTITY" \
    --network "$NETWORK" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- "$@" | tr -d '"[:space:]'
}

if [ "$CONTRACT" = "alert-registry" ]; then
  OLD_SCHEMA=$(alert_invoke get_schema_version)
fi

echo "==> Upgrading contract $CONTRACT_ID directly..."
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" \
  -- upgrade \
  --admin "$ADMIN" \
  --new_wasm_hash "$NEW_WASM_HASH"

echo "==> Verifying deployed WASM hash..."
DEPLOYED_HASH=$(deployed_wasm_hash "$CONTRACT_ID")
if [[ "$DEPLOYED_HASH" != "$NEW_WASM_HASH" ]]; then
  echo "Error: deployed WASM hash $DEPLOYED_HASH does not match expected $NEW_WASM_HASH" >&2
  exit 1
fi

if [ "$CONTRACT" = "alert-registry" ]; then
  # The new code's target version, read from the source it was built from.
  NEW_SCHEMA=$(sed -n 's/^pub const CURRENT_SCHEMA_VERSION: u32 = \([0-9]*\);/\1/p' contracts/alert-registry/src/lib.rs)
  echo "==> Schema version: on-chain $OLD_SCHEMA, new code $NEW_SCHEMA"
  if [[ "$OLD_SCHEMA" -lt "$NEW_SCHEMA" ]]; then
    ALERT_COUNT=$(alert_invoke get_alert_count)
    CURSOR=0
    while [[ "$(alert_invoke get_schema_version)" -lt "$NEW_SCHEMA" ]]; do
      CURSOR=$(alert_invoke migrate --caller "$ADMIN" --from "$OLD_SCHEMA" --to "$NEW_SCHEMA" --cursor "$CURSOR")
      echo "==> migrate: cursor $CURSOR / $ALERT_COUNT"
    done
    echo "==> Migration complete: schema version $NEW_SCHEMA"
  fi
fi

echo "==> Upgrade complete. Contract $CONTRACT_ID now runs WASM $NEW_WASM_HASH"
