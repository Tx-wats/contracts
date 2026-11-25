#!/usr/bin/env bash
# scripts/keeper.sh — Reference keeper: keeps registry instances and alert TTLs alive
#
# Usage:
#   ./scripts/keeper.sh instance                          # bump_instance_ttl on both contracts
#   ./scripts/keeper.sh alerts --ids-file ids.txt [--ttl 535680]
#   ./scripts/keeper.sh prune --owners-file owners.txt
#   ./scripts/keeper.sh prune-contracts --targets-file targets.txt
#
# Environment:
#   NETWORK                 testnet (default) or mainnet
#   STELLAR_IDENTITY        Stellar CLI identity paying fees (default: keeper)
#   ALERT_REGISTRY_ID       AlertRegistry contract ID
#   WATCHER_REGISTRY_ID     WatcherRegistry contract ID (instance subcommand only)
#   MAINNET_RPC_URL         required when NETWORK=mainnet
#
# The bump entrypoints need no auth, so the identity only pays fees. See
# docs/keeper-guide.md for cadence, batching, monitoring and restore advice.
set -euo pipefail

# shellcheck source=scripts/lib/common.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/common.sh"

NETWORK="${NETWORK:-testnet}"
IDENTITY="${STELLAR_IDENTITY:-keeper}"
resolve_network

# Invoke a contract function. Arguments after the contract ID are passed to
# the function. stdin is closed so the CLI cannot swallow an ID-file read loop.
invoke() {
  local contract_id="$1"
  shift
  stellar contract invoke \
    --id "$contract_id" \
    --source "$IDENTITY" \
    --network "$NETWORK" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- "$@" </dev/null
}

require_env() {
  local name
  for name in "$@"; do
    [[ -n "${!name:-}" ]] || { echo "Error: $name must be set" >&2; exit 1; }
  done
}

# Extend both registries' instance entries. Each call is a no-op until the
# remaining TTL drops below INSTANCE_BUMP_THRESHOLD, so run it every 12 hours.
cmd_instance() {
  require_env ALERT_REGISTRY_ID WATCHER_REGISTRY_ID
  echo "==> bump_instance_ttl: alert-registry ($ALERT_REGISTRY_ID)"
  invoke "$ALERT_REGISTRY_ID" bump_instance_ttl
  echo "==> bump_instance_ttl: watcher-registry ($WATCHER_REGISTRY_ID)"
  invoke "$WATCHER_REGISTRY_ID" bump_instance_ttl
}

# Bump each alert ID listed in --ids-file (one ID per line, # comments allowed)
# to --ttl ledgers. Soroban allows one invoke per transaction, so each ID is a
# separate call; failures are reported and the script exits non-zero at the end.
cmd_alerts() {
  local ids_file="" ttl="535680" id failures=0
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --ids-file) ids_file="${2:?--ids-file needs a value}"; shift 2 ;;
      --ttl)      ttl="${2:?--ttl needs a value}";           shift 2 ;;
      *) echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
  done
  [[ -n "$ids_file" ]] || { echo "Error: --ids-file required" >&2; exit 1; }
  require_env ALERT_REGISTRY_ID

  while read -r id; do
    if [[ -z "$id" || "$id" == \#* ]]; then
      continue
    fi
    if invoke "$ALERT_REGISTRY_ID" bump_alert --config_id "$id" --ttl "$ttl" >/dev/null; then
      echo "==> bumped alert $id to $ttl ledgers"
    else
      echo "==> FAILED alert $id (archived? paused? see docs/keeper-guide.md)" >&2
      failures=$((failures + 1))
    fi
  done < "$ids_file"

  if [[ $failures -gt 0 ]]; then
    echo "Error: $failures alert bump(s) failed" >&2
    exit 1
  fi
}

# Run prune_expired_alerts for each owner address in --owners-file. Each call
# scans a bounded window of the owner index, so repeat it until
# get_prune_cursor reports no scan in progress.
cmd_prune() {
  local owners_file="" owner
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --owners-file) owners_file="${2:?--owners-file needs a value}"; shift 2 ;;
      *) echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
  done
  [[ -n "$owners_file" ]] || { echo "Error: --owners-file required" >&2; exit 1; }
  require_env ALERT_REGISTRY_ID

  while read -r owner; do
    if [[ -z "$owner" || "$owner" == \#* ]]; then
      continue
    fi
    local cursor pass=0
    while :; do
      pass=$((pass + 1))
      echo "==> prune_expired_alerts: $owner (pass $pass)"
      invoke "$ALERT_REGISTRY_ID" prune_expired_alerts --owner "$owner"
      cursor=$(invoke "$ALERT_REGISTRY_ID" get_prune_cursor --owner "$owner" | tr -d '"[:space:]')
      [[ -z "$cursor" || "$cursor" == "null" ]] && break
    done
  done < "$owners_file"
}

# Run prune_contract_index for each target contract address in --targets-file,
# releasing the per-contract quota held by expired alerts.
cmd_prune_contracts() {
  local targets_file="" target failures=0
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --targets-file) targets_file="${2:?--targets-file needs a value}"; shift 2 ;;
      *) echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
  done
  [[ -n "$targets_file" ]] || { echo "Error: --targets-file required" >&2; exit 1; }
  require_env ALERT_REGISTRY_ID

  while read -r target; do
    if [[ -z "$target" || "$target" == \#* ]]; then
      continue
    fi
    echo "==> prune_contract_index: $target"
    if ! invoke "$ALERT_REGISTRY_ID" prune_contract_index --target "$target"; then
      echo "==> FAILED prune_contract_index $target" >&2
      failures=$((failures + 1))
    fi
  done < "$targets_file"

  if [[ $failures -gt 0 ]]; then
    echo "Error: $failures contract prune(s) failed" >&2
    exit 1
  fi
}

case "${1:-}" in
  instance) shift; cmd_instance "$@" ;;
  alerts)   shift; cmd_alerts "$@" ;;
  prune)    shift; cmd_prune "$@" ;;
  prune-contracts) shift; cmd_prune_contracts "$@" ;;
  *)
    sed -n '2,13p' "$0" >&2
    exit 1
    ;;
esac
