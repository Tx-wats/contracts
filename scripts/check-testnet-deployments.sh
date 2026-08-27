#!/usr/bin/env bash
# scripts/check-testnet-deployments.sh — Verify that testnet addresses in DEPLOYMENTS.md are still live
set -euo pipefail

RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"
DEPLOYMENTS_FILE="${1:-DEPLOYMENTS.md}"

echo "======================================================="
echo "TxWatch Testnet Deployment Liveness Check"
echo "======================================================="
echo "RPC URL:          $RPC_URL"
echo "Deployments File: $DEPLOYMENTS_FILE"
echo ""

if [[ ! -f "$DEPLOYMENTS_FILE" ]]; then
    echo "Error: Deployments file '$DEPLOYMENTS_FILE' not found."
    exit 1
fi

# 1. Check Soroban Testnet RPC health
echo "--> Checking Soroban Testnet RPC health..."
RPC_HEALTH=$(curl -s -X POST "$RPC_URL" -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' || true)

if echo "$RPC_HEALTH" | grep -q '"status":"healthy"'; then
    LATEST_LEDGER=$(echo "$RPC_HEALTH" | grep -o '"latestLedger":[0-9]*' | cut -d: -f2)
    echo "    RPC is healthy. Latest ledger: $LATEST_LEDGER"
else
    echo "    Warning: Soroban RPC health check failed or returned unexpected response:"
    echo "    $RPC_HEALTH"
fi
echo ""

# 2. Extract Testnet contract addresses from DEPLOYMENTS.md
echo "--> Extracting testnet contract addresses from $DEPLOYMENTS_FILE..."

# Parse addresses using python
CONTRACTS_JSON=$(python3 - <<'PYEOF'
import re, json, sys

try:
    with open("DEPLOYMENTS.md", "r") as f:
        content = f.read()
except Exception as e:
    print(json.dumps({"error": str(e)}))
    sys.exit(1)

# Extract testnet section
testnet_section_match = re.search(r"## Stellar Testnet.*?(?=## Stellar Mainnet|\Z)", content, re.DOTALL)
if not testnet_section_match:
    print(json.dumps({"error": "No Stellar Testnet section found"}))
    sys.exit(1)

section = testnet_section_match.group(0)
contracts = []

# Match table rows like: | Alert Registry | `CDSO4...` | `...` |
for line in section.splitlines():
    m = re.search(r"\|\s*([^|]+?)\s*\|\s*`([C][A-Z0-9]{55})`\s*\|\s*`?([^`|]+)?`?\s*\|", line)
    if m:
        name = m.group(1).strip()
        address = m.group(2).strip()
        wasm_hash = (m.group(3) or "").strip()
        if not address.startswith("CXXXXXXXX"):
            contracts.append({"name": name, "address": address, "wasm_hash": wasm_hash})

print(json.dumps({"contracts": contracts}))
PYEOF
)

CONTRACT_COUNT=$(echo "$CONTRACTS_JSON" | python3 -c 'import sys, json; data=json.load(sys.stdin); print(len(data.get("contracts", [])))')

if [[ "$CONTRACT_COUNT" -eq 0 ]]; then
    echo "    Error: No valid testnet contract addresses found in $DEPLOYMENTS_FILE."
    exit 1
fi

echo "    Found $CONTRACT_COUNT testnet contract(s) to verify."
echo ""

# 3. Check each contract address
STALE_FOUND=0

echo "$CONTRACTS_JSON" | python3 -c '
import sys, json, urllib.request, urllib.error

data = json.load(sys.stdin)
contracts = data.get("contracts", [])
failed = []

for c in contracts:
    name = c["name"]
    addr = c["address"]
    print(f"--> Checking {name} ({addr})...")
    
    # Query Stellar Expert testnet contract endpoint
    url = f"https://api.stellar.expert/explorer/testnet/contract/{addr}"
    req = urllib.request.Request(url, headers={"User-Agent": "TxWatch-LivenessCheck/1.0"})
    
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            if resp.status == 200:
                res_data = json.loads(resp.read().decode())
                wasm = res_data.get("wasm", "unknown")
                created = res_data.get("created", "unknown")
                print(f"    [LIVE] Contract exists on testnet ledger.")
                print(f"           Created at: {created}, WASM: {wasm}")
            else:
                print(f"    [FAIL] Unexpected HTTP status {resp.status}")
                failed.append((name, addr, f"HTTP {resp.status}"))
    except urllib.error.HTTPError as e:
        if e.code == 404:
            print(f"    [STALE] Contract NOT FOUND on testnet ledger (404 Not Found).")
            print(f"            Testnet may have been reset or contract has expired.")
        else:
            print(f"    [FAIL] HTTP error {e.code}: {e.reason}")
        failed.append((name, addr, f"HTTP {e.code}"))
    except Exception as e:
        print(f"    [ERROR] Query failed: {e}")
        failed.append((name, addr, str(e)))

print("")
if failed:
    print("=======================================================")
    print("❌ LIVENESS CHECK FAILED: Stale or missing contract(s):")
    for name, addr, reason in failed:
        print(f"  - {name}: {addr} ({reason})")
    print("")
    print("Remediation steps:")
    print("  1. Refer to docs/deployment-guide.md for the reset recovery process.")
    print("  2. Re-deploy contracts: bash scripts/deploy.sh")
    print("  3. Update DEPLOYMENTS.md with the new contract IDs.")
    print("=======================================================")
    sys.exit(1)
else:
    print("=======================================================")
    print("✅ ALL TESTNET CONTRACT ADDRESSES ARE LIVE")
    print("=======================================================")
    sys.exit(0)
'
