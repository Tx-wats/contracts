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
    [--cli-version <STELLAR_CLI_VERSION>] \
    [--tag <RELEASE_TAG>] \
    [--notes <NOTES>] \
    [--file DEPLOYMENTS.md]

If --cli-version is omitted, it is read from `stellar --version`.
"""

import argparse
import os
import re
import subprocess
from datetime import datetime, timezone

def table_cells(line: str) -> list[str]:
    # Trimmed cells of a Markdown table row, or [] for any other line
    stripped = line.strip()
    if not stripped.startswith("|"):
        return []
    return [cell.strip() for cell in stripped.strip("|").split("|")]

def format_row(cells: list[str]) -> str:
    return "| " + " | ".join(cells) + " |"

def get_cli_version() -> str:
    # `stellar --version` prints e.g. "stellar 28.0.0 (...)"
    output = subprocess.run(["stellar", "--version"], capture_output=True, text=True, check=True).stdout
    version = output.split()[1]
    return version if version.startswith("v") else f"v{version}"

def update_deployments_content(
    content: str,
    network: str,
    alert_id: str,
    watcher_id: str,
    alert_hash: str,
    watcher_hash: str,
    tag: str = "",
    notes: str = "",
    date_str: str = "",
    cli_version: str = "",
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

    # Rewrite each table row cell-wise so old cell contents cannot be left behind.
    current_rows = {
        "Alert Registry": [f"`{alert_id}`", f"`{alert_hash}`"],
        "Watcher Registry": [f"`{watcher_id}`", f"`{watcher_hash}`"],
    }
    section_lines = []
    updated_names = []
    for line in match.group(1).split("\n"):
        cells = table_cells(line)
        if cells and cells[0] in current_rows:
            updated_names.append(cells[0])
            line = format_row([cells[0], *current_rows[cells[0]]])
        section_lines.append(line)

    if any(updated_names.count(name) != 1 for name in current_rows):
        raise ValueError(f"Expected one Alert Registry and Watcher Registry row in the {network_capitalized} section")

    content = content[:match.start()] + "\n".join(section_lines) + content[match.end():]

    # 2. Update Deployment History table
    # Format of history rows:
    # | Date | Network | Contract | Address | WASM Hash | CLI Version | Notes |
    row_note = notes if notes else (tag if tag else "Automated deployment")
    history_rows = [
        format_row([date_str, network_capitalized, "Alert Registry", f"`{alert_id}`", f"`{alert_hash}`", cli_version, row_note]),
        format_row([date_str, network_capitalized, "Watcher Registry", f"`{watcher_id}`", f"`{watcher_hash}`", cli_version, row_note]),
    ]

    lines = content.split("\n")
    header_index = next(
        (i for i, line in enumerate(lines) if table_cells(line)[:2] == ["Date", "Network"]),
        None,
    )
    if header_index is None:
        raise ValueError("Could not locate the Deployment History table in DEPLOYMENTS.md")

    # Insert before the "Initial placeholder" row if there is one, otherwise right under the header separator
    insert_at = header_index + 2
    for i in range(header_index + 2, len(lines)):
        cells = table_cells(lines[i])
        if not cells:
            break
        if cells[-1] == "Initial placeholder":
            insert_at = i
            break

    lines[insert_at:insert_at] = history_rows
    return "\n".join(lines)

def main():
    parser = argparse.ArgumentParser(description="Update contract deployment addresses and history.")
    parser.add_argument("--network", required=True, choices=["testnet", "mainnet"])
    parser.add_argument("--alert-id", required=True)
    parser.add_argument("--watcher-id", required=True)
    parser.add_argument("--alert-hash", default="TODO")
    parser.add_argument("--watcher-hash", default="TODO")
    parser.add_argument("--cli-version", default="")
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
        cli_version=args.cli_version or get_cli_version(),
        tag=args.tag,
        notes=args.notes
    )

    with open(args.file, "w", encoding="utf-8") as f:
        f.write(updated)

    print(f"Successfully updated {args.file} for {args.network}")

if __name__ == "__main__":
    main()
