#!/usr/bin/env python3
"""
scripts/update_deployments.py — Reliably update contract addresses and history in DEPLOYMENTS.md.

Usage:
  python3 scripts/update_deployments.py \
    --network testnet|mainnet \
    --alert-id <ALERT_CONTRACT_ID> \
    --watcher-id <WATCHER_CONTRACT_ID> \
    --alert-hash <ALERT_WASM_HASH> \
    --watcher-hash <WATCHER_WASM_HASH> \
    [--tag <RELEASE_TAG>] \
    [--notes <NOTES>] \
    [--file DEPLOYMENTS.md]
"""

import argparse
import os
import re
from datetime import datetime, timezone

def update_deployments_content(
    content: str,
    network: str,
    alert_id: str,
    watcher_id: str,
    alert_hash: str,
    watcher_hash: str,
    tag: str = "",
    notes: str = "",
    date_str: str = ""
) -> str:
    if not date_str:
        date_str = datetime.now(timezone.utc).strftime("%Y-%m-%d")

    network_capitalized = "Testnet" if network.lower() == "testnet" else "Mainnet"

    # 1. Update current deployment addresses & hashes for the specific network section
    # Find the section: ## Stellar <Network>
    section_pattern = rf"(## Stellar {network_capitalized}.*?)(?=## Stellar|\Z)"
    match = re.search(section_pattern, content, re.DOTALL)
    if not match:
        raise ValueError(f"Could not locate section '## Stellar {network_capitalized}' in DEPLOYMENTS.md")

    section_text = match.group(1)

    # Replace Alert Registry row in this section
    alert_row_pattern = r"(\| Alert Registry \| )`[^`]+`(\| )`[^`]+`"
    if re.search(alert_row_pattern, section_text):
        new_section = re.sub(alert_row_pattern, rf"\1`{alert_id}`\2`{alert_hash}`", section_text)
    else:
        # Fallback if placeholder formatting differs
        new_section = re.sub(
            r"(\| Alert Registry \| )`[^`]+`",
            rf"\1`{alert_id}` | `{alert_hash}`",
            section_text
        )

    # Replace Watcher Registry row in this section
    watcher_row_pattern = r"(\| Watcher Registry \| )`[^`]+`(\| )`[^`]+`"
    if re.search(watcher_row_pattern, new_section):
        new_section = re.sub(watcher_row_pattern, rf"\1`{watcher_id}`\2`{watcher_hash}`", new_section)
    else:
        new_section = re.sub(
            r"(\| Watcher Registry \| )`[^`]+`",
            rf"\1`{watcher_id}` | `{watcher_hash}`",
            new_section
        )

    content = content[:match.start()] + new_section + content[match.end():]

    # 2. Update Deployment History table
    # Format of history rows:
    # | Date | Network | Contract | Address | WASM Hash | Notes |
    row_note = notes if notes else (tag if tag else "Automated deployment")
    history_rows = (
        f"| {date_str} | {network_capitalized} | Alert Registry | `{alert_id}` | `{alert_hash}` | {row_note} |\n"
        f"| {date_str} | {network_capitalized} | Watcher Registry | `{watcher_id}` | `{watcher_hash}` | {row_note} |\n"
    )

    # Match exact placeholder row with any number of dashes
    # e.g. | — | — | — | — | — | — | Initial placeholder | or with 5 dashes
    placeholder_pattern = r"(\|\s*—\s*)+\|\s*Initial placeholder\s*\|"
    if re.search(placeholder_pattern, content):
        content = re.sub(
            placeholder_pattern,
            history_rows + r"\g<0>",
            content,
            count=1
        )
    else:
        # Prepend to the first data row under Deployment History table header
        history_header_pattern = r"(\| Date \| Network \| Contract \| Address \| WASM Hash \|( CLI Version \|)? Notes \|\n\|[-|\s]+\|\n)"
        content = re.sub(history_header_pattern, rf"\g<1>{history_rows}", content, count=1)

    return content

def main():
    parser = argparse.ArgumentParser(description="Update contract deployment addresses and history.")
    parser.add_argument("--network", required=True, choices=["testnet", "mainnet"])
    parser.add_argument("--alert-id", required=True)
    parser.add_argument("--watcher-id", required=True)
    parser.add_argument("--alert-hash", default="TODO")
    parser.add_argument("--watcher-hash", default="TODO")
    parser.add_argument("--tag", default="")
    parser.add_argument("--notes", default="")
    parser.add_argument("--file", default="DEPLOYMENTS.md")

    args = parser.parse_args()

    if not os.path.exists(args.file):
        raise FileNotFoundError(f"{args.file} not found")

    with open(args.file, "r", encoding="utf-8") as f:
        content = f.read()

    updated = update_deployments_content(
        content=content,
        network=args.network,
        alert_id=args.alert_id,
        watcher_id=args.watcher_id,
        alert_hash=args.alert_hash,
        watcher_hash=args.watcher_hash,
        tag=args.tag,
        notes=args.notes
    )

    with open(args.file, "w", encoding="utf-8") as f:
        f.write(updated)

    print(f"Successfully updated {args.file} for {args.network}")

if __name__ == "__main__":
    main()
