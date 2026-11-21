#!/usr/bin/env bash
# scripts/deploy.sh — Deploy both contracts to Stellar (testnet or mainnet)
# Usage: ./scripts/deploy.sh [--network testnet|mainnet] [--no-gating]
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
  --network-passphrase "$NETWORK_PASSPHRASE" \
  -- --admin "$ADMIN_ADDRESS")
echo "Watcher Registry deployed: $WATCHER_ID"

if [ "$NO_GATING" = true ]; then
  echo "==> Skipping WatcherRegistry gating (--no-gating specified)."
else
  echo "==> Configuring Watcher Registry gating on Alert Registry..."
  stellar contract invoke \
    --id "$ALERT_ID" \
    --source "$IDENTITY" \
    --network "$NETWORK" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- set_watcher_registry \
    --admin "$ADMIN_ADDRESS" \
    --watcher_registry "$WATCHER_ID"
fi

ALERT_HASH=$(sha256sum "$ALERT_WASM" | awk '{print $1}')
WATCHER_HASH=$(sha256sum "$WATCHER_WASM" | awk '{print $1}')

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    printf 'alert_id=%s\n' "$ALERT_ID"
    printf 'watcher_id=%s\n' "$WATCHER_ID"
    printf 'alert_hash=%s\n' "$ALERT_HASH"
    printf 'watcher_hash=%s\n' "$WATCHER_HASH"
  } >> "$GITHUB_OUTPUT"
fi

echo ""
echo "=================================================================="
echo "Deployment Complete ($NETWORK)"
echo "=================================================================="
echo "Alert Registry:   $ALERT_ID (WASM: $ALERT_HASH)"
echo "Watcher Registry: $WATCHER_ID (WASM: $WATCHER_HASH)"
echo "Admin Address:    $ADMIN_ADDRESS"

echo ""
echo "==> Verifying Alert Registry Configuration:"
ALERT_ADMIN=$(stellar contract invoke \
  --id "$ALERT_ID" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" \
  -- get_admin)
if [[ "$ALERT_ADMIN" != *"$ADMIN_ADDRESS"* ]]; then
  echo "Error: Alert Registry admin verification failed (expected $ADMIN_ADDRESS, got $ALERT_ADMIN)" >&2
  exit 1
fi
echo "    Configured Admin:     $ALERT_ADMIN"

ALERT_WATCHREG=$(stellar contract invoke \
  --id "$ALERT_ID" \
  --source "$IDENTITY" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" \
  -- get_watcher_registry)
if [[ "$ALERT_WATCHREG" != *"$WATCHER_ID"* ]]; then
  echo "Error: Alert Registry watcher verification failed (expected $WATCHER_ID, got $ALERT_WATCHREG)" >&2
  exit 1
fi
echo "    Gated WatcherRegistry: $ALERT_WATCHREG"

echo ""
echo "==> Update DEPLOYMENTS.md with the addresses and hashes above."
