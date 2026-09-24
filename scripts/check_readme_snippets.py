#!/usr/bin/env python3
"""
scripts/check_readme_snippets.py — Verify that the contract calls shown in
README.md exist in the compiled contract spec.

The spec is the JSON produced by
`stellar contract info interface --wasm <file> --output json`, i.e. exactly
what a client sees on-chain, so a renamed or removed function, or a renamed
argument, fails here instead of in a reader's terminal.

Checks, per documentation file:
1. Every `stellar contract invoke --id <X_CONTRACT_ID> ... -- <fn>` snippet
   names a function in contract X's spec.
2. Every `--<arg>` passed after the function name is an input of that function.
3. Every JS `contract.call("<fn>", ...)` names a function in the contract the
   enclosing snippet constructed with `new Contract("<X_CONTRACT_ID>")`.
4. Every TS `client.<fn>(...)` names a function in the contract whose bindings
   package the enclosing snippet imports.

Usage:
    check_readme_snippets.py --spec alert-registry=ar.json \
                             --spec watcher-registry=wr.json [FILE ...]

FILE defaults to README.md at the repository root.
"""

import argparse
import json
import os
import re
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

# Placeholders and package names the docs use to identify each contract.
CONTRACT_IDS = {
    "<ALERT_REGISTRY_CONTRACT_ID>": "alert-registry",
    "<WATCHER_REGISTRY_CONTRACT_ID>": "watcher-registry",
}
BINDINGS_PACKAGES = {
    "@tx-wat/alert-registry-bindings": "alert-registry",
    "@tx-wat/watcher-registry": "watcher-registry",
}

FENCE_RE = re.compile(r"^```([A-Za-z]*)[^\n]*\n(.*?)^```", re.M | re.S)


def load_spec(path: str) -> dict:
    """Return {function name: set of input names} from a spec JSON file."""
    with open(path, "r", encoding="utf-8") as f:
        entries = json.load(f)
    functions = {}
    for entry in entries:
        fn = entry.get("function_v0")
        if fn is None or fn["name"].startswith("__"):
            continue
        functions[fn["name"]] = {i["name"] for i in fn["inputs"]}
    return functions


def iter_blocks(text: str):
    """Yield (language, first line number, body) for every fenced code block."""
    for m in FENCE_RE.finditer(text):
        line = text.count("\n", 0, m.start(2)) + 1
        yield m.group(1).lower(), line, m.group(2)


def check_cli_block(body, first_line, specs, errors):
    # Join backslash continuations so each invocation is one logical line,
    # remembering the physical line it started on for error messages.
    logical, start = "", None
    commands = []
    for offset, raw in enumerate(body.split("\n")):
        line = raw.split(" #", 1)[0] if not raw.lstrip().startswith("#") else ""
        if start is None:
            start = first_line + offset
        if line.rstrip().endswith("\\"):
            logical += line.rstrip()[:-1] + " "
            continue
        logical += line
        commands.append((start, logical))
        logical, start = "", None

    for lineno, cmd in commands:
        if "stellar contract invoke" not in cmd:
            continue
        if " -- " not in cmd:
            errors.append(f"line {lineno}: invoke has no `-- <function>` separator")
            continue
        cli_part, call_part = cmd.split(" -- ", 1)
        id_match = re.search(r"--id\s+(\S+)", cli_part)
        contract = CONTRACT_IDS.get(id_match.group(1)) if id_match else None
        if contract is None:
            # Not one of this repo's contracts (or a literal ID) — nothing to check against.
            continue
        tokens = call_part.split()
        fn = tokens[0]
        spec = specs[contract]
        if fn not in spec:
            errors.append(f"line {lineno}: `{fn}` is not a function of {contract}")
            continue
        for tok in tokens[1:]:
            if tok.startswith("--"):
                arg = tok[2:].split("=", 1)[0]
                if arg not in spec[fn]:
                    expected = ", ".join(sorted(spec[fn])) or "none"
                    errors.append(
                        f"line {lineno}: `{fn}` has no argument `{arg}` "
                        f"(expects: {expected})"
                    )


def check_js_block(body, first_line, specs, errors):
    contract = None
    for m in re.finditer(r'new Contract\(\s*"([^"]+)"\s*\)', body):
        contract = CONTRACT_IDS.get(m.group(1), contract)
    for pkg, name in BINDINGS_PACKAGES.items():
        if re.search(r'from\s+"' + re.escape(pkg) + r'"', body):
            contract = name
    if contract is None:
        return
    spec = specs[contract]
    patterns = [r'contract\.call\(\s*"(\w+)"', r"\bclient\.(\w+)\("]
    for pattern in patterns:
        for m in re.finditer(pattern, body):
            fn = m.group(1)
            if fn not in spec:
                lineno = first_line + body.count("\n", 0, m.start())
                errors.append(f"line {lineno}: `{fn}` is not a function of {contract}")


def check_file(path, specs):
    with open(path, "r", encoding="utf-8") as f:
        text = f.read()
    errors = []
    for lang, line, body in iter_blocks(text):
        if lang in ("bash", "sh", "shell", ""):
            check_cli_block(body, line, specs, errors)
        elif lang in ("js", "javascript", "ts", "typescript"):
            check_js_block(body, line, specs, errors)
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "--spec",
        action="append",
        required=True,
        metavar="CONTRACT=PATH",
        help="contract spec JSON from `stellar contract info interface --output json`",
    )
    parser.add_argument("files", nargs="*", default=[os.path.join(REPO_ROOT, "README.md")])
    args = parser.parse_args()

    specs = {}
    for item in args.spec:
        name, _, path = item.partition("=")
        specs[name] = load_spec(path)
    missing = set(CONTRACT_IDS.values()) - set(specs)
    if missing:
        parser.error(f"missing --spec for: {', '.join(sorted(missing))}")

    failed = False
    for path in args.files:
        errors = check_file(path, specs)
        rel = os.path.relpath(path, REPO_ROOT)
        if errors:
            failed = True
            print(f"❌ {rel}:")
            for e in errors:
                print(f"   {e}")
        else:
            print(f"✅ {rel}: every contract call matches the spec")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
