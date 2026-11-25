#!/usr/bin/env python3
"""
scripts/check_doc_comments.py — Flag merge-splice leftovers in `///` doc comments.

The compiler does not notice when a merge duplicates a doc paragraph, lists a
`# Events` or `# Errors` section twice on one item, or stacks two
`#[deprecated]` attributes (or two `since`/`note` keys) on one item. This
script scans `contracts/**/*.rs` and reports those per item.

Known pre-existing offenders are listed in scripts/doc_comment_allowlist.txt
as `<path> <item> <rule>` lines. An allow-listed finding is not reported; an
allow-list entry that no longer matches anything fails the check, so the list
cannot rot silently.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCAN_DIR = ROOT / "contracts"
ALLOWLIST = Path(__file__).resolve().parent / "doc_comment_allowlist.txt"

MIN_PARAGRAPH_CHARS = 20
SECTION_RE = re.compile(r"^#\s+(Events|Errors)\s*$")
ITEM_RE = re.compile(r"\b(?:fn|struct|enum|const|static|type|trait|mod|union)\s+(\w+)")
FIELD_RE = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?(\w+)\s*:")


def item_name(line: str) -> str:
    m = ITEM_RE.search(line) or FIELD_RE.match(line.strip())
    return m.group(1) if m else line.strip()[:40]


def read_attribute(lines: list, start: int) -> tuple:
    """Return (attribute text, index of the line after it), balancing brackets."""
    text, depth, i = "", 0, start
    while i < len(lines):
        text += lines[i].strip() + " "
        depth += lines[i].count("[") - lines[i].count("]")
        i += 1
        if depth <= 0:
            break
    return text.strip(), i


def items(lines: list):
    """Yield (name, doc bodies, attribute texts) for every documented item."""
    docs, attrs = [], []
    i = 0
    while i < len(lines):
        stripped = lines[i].strip()
        if stripped.startswith("///"):
            body = stripped[3:]
            docs.append(body[1:] if body.startswith(" ") else body)
            i += 1
        elif stripped.startswith("#["):
            text, i = read_attribute(lines, i)
            attrs.append(text)
        elif stripped == "" or stripped.startswith("//"):
            i += 1
        else:
            if docs or attrs:
                yield item_name(stripped), docs, attrs
            docs, attrs = [], []
            i += 1


def paragraphs(docs: list) -> list:
    out, current = [], []
    for body in docs + [""]:
        if body.strip():
            current.append(body.strip())
        elif current:
            out.append(" ".join(current))
            current = []
    return out


def findings_for(name: str, docs: list, attrs: list) -> list:
    rules = []

    counts = {}
    for body in docs:
        m = SECTION_RE.match(body.strip())
        if m:
            counts[m.group(1)] = counts.get(m.group(1), 0) + 1
    for section, n in counts.items():
        if n > 1:
            rules.append(("duplicate-section", f"`# {section}` appears {n} times"))

    seen = set()
    for para in paragraphs(docs):
        if para.startswith("#") or len(para) < MIN_PARAGRAPH_CHARS:
            continue
        if para in seen:
            rules.append(("duplicate-paragraph", para[:60]))
        seen.add(para)

    deprecated = [a for a in attrs if a.startswith("#[deprecated")]
    if len(deprecated) > 1:
        rules.append(("duplicate-deprecated", f"{len(deprecated)} `#[deprecated]` attributes"))
    for attr in deprecated:
        for key in ("since", "note"):
            n = len(re.findall(rf"\b{key}\s*=", attr))
            if n > 1:
                rules.append(("duplicate-deprecated", f"`{key}` set {n} times in `#[deprecated]`"))

    return [(name, rule, detail) for rule, detail in rules]


def load_allowlist() -> set:
    if not ALLOWLIST.exists():
        return set()
    entries = set()
    for line in ALLOWLIST.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            entries.add(tuple(line.split()))
    return entries


def main():
    allowed = load_allowlist()
    used = set()
    failures = []

    for path in sorted(SCAN_DIR.rglob("*.rs")):
        if "target" in path.parts:
            continue
        rel = path.relative_to(ROOT).as_posix()
        lines = path.read_text().splitlines()
        for name, docs, attrs in items(lines):
            for item, rule, detail in findings_for(name, docs, attrs):
                key = (rel, item, rule)
                if key in allowed:
                    used.add(key)
                else:
                    failures.append(f"{rel}: `{item}` [{rule}] {detail}")

    stale = sorted(allowed - used)
    for f in sorted(set(failures)):
        print(f"❌ {f}")
    for rel, item, rule in stale:
        print(f"❌ stale allow-list entry: {rel} {item} {rule} (no longer matches; remove it)")
    if failures or stale:
        print(f"\n{len(set(failures))} doc-comment problem(s), {len(stale)} stale allow-list entr(y/ies).")
        sys.exit(1)
    print(f"✅ no doc-comment merge leftovers ({len(used)} allow-listed)")


if __name__ == "__main__":
    main()
