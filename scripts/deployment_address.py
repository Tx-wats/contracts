#!/usr/bin/env python3
"""Extract a deployed contract address from DEPLOYMENTS.md.

DEPLOYMENTS.md is the human-maintained source of truth for deployed contract
addresses. CI jobs (notably .github/workflows/publish-bindings.yml) must read
the real address from it rather than baking a placeholder into published
artifacts. This script is that single reader.

Usage:
    deployment_address.py --network testnet --contract "Watcher Registry"

Prints the address on stdout. Exits non-zero with a message on stderr if the
requested cell is missing or still holds a placeholder / TODO value.
"""

# === Imports ===
import argparse
import re
import sys
from pathlib import Path

# === Constants ===
# Stellar contract addresses are strkey-encoded: 'C' followed by 55 base32
# characters (RFC 4648 alphabet without 0, 1, 8, 9).
STRKEY_RE = re.compile(r"^C[A-Z2-7]{55}$")

# The repo uses a run of 'X' as the placeholder marker throughout
# (DEPLOYMENTS.md, workflows, bindings). Four or more in a row is never a real
# strkey in practice and unambiguously flags an un-filled cell.
PLACEHOLDER_RUN_RE = re.compile(r"X{4,}")

DEFAULT_DEPLOYMENTS = Path(__file__).resolve().parent.parent / "DEPLOYMENTS.md"

SECTION_HEADERS = {
    "testnet": "## Stellar Testnet",
    "mainnet": "## Stellar Mainnet",
}


# === Parsing ===
def extract_section(text: str, header: str) -> str:
    """Return the markdown between `header` and the next `## ` header."""
    lines = text.splitlines()
    out: list[str] = []
    collecting = False
    for line in lines:
        if line.strip() == header:
            collecting = True
            continue
        if collecting and line.startswith("## "):
            break
        if collecting:
            out.append(line)
    return "\n".join(out)


def find_address(section: str, contract: str) -> str | None:
    """Return the first backtick-quoted value in the `contract` table row."""
    for line in section.splitlines():
        if not line.lstrip().startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if not cells or cells[0].lower() != contract.lower():
            continue
        match = re.search(r"`([^`]+)`", line)
        if match:
            return match.group(1).strip()
    return None


def is_placeholder(address: str) -> bool:
    if address.upper() in {"TODO", "TBD", "N/A", ""}:
        return True
    if PLACEHOLDER_RUN_RE.search(address):
        return True
    return not STRKEY_RE.match(address)


# === Entry point ===
def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--network", required=True, choices=sorted(SECTION_HEADERS))
    parser.add_argument(
        "--contract",
        required=True,
        help='Contract name as it appears in the DEPLOYMENTS.md table, e.g. "Watcher Registry"',
    )
    parser.add_argument(
        "--file",
        type=Path,
        default=DEFAULT_DEPLOYMENTS,
        help="Path to DEPLOYMENTS.md (defaults to repo root)",
    )
    args = parser.parse_args()

    if not args.file.is_file():
        print(f"error: {args.file} not found", file=sys.stderr)
        return 2

    text = args.file.read_text(encoding="utf-8")
    section = extract_section(text, SECTION_HEADERS[args.network])
    if not section.strip():
        print(
            f"error: no '{SECTION_HEADERS[args.network]}' section in {args.file}",
            file=sys.stderr,
        )
        return 2

    address = find_address(section, args.contract)
    if address is None:
        print(
            f"error: no row for '{args.contract}' in the {args.network} table of {args.file}",
            file=sys.stderr,
        )
        return 2

    if is_placeholder(address):
        print(
            f"error: {args.contract} {args.network} address is still a placeholder "
            f"('{address}'). Deploy the contract and update {args.file.name} before publishing.",
            file=sys.stderr,
        )
        return 1

    print(address)
    return 0


if __name__ == "__main__":
    sys.exit(main())
