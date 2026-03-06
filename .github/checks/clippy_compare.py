#!/usr/bin/env python3
"""Compare project's Cargo.toml clippy lint config against clippy defaults.

Usage:
    python3 .github/checks/clippy_compare.py

Reads:
    - .github/checks/clippy_lints_list.md  (all clippy lints + defaults)
    - Cargo.toml                           ([workspace.lints.clippy] section)

Writes:
    - clippy_compared_list.md
"""

import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent.parent
LINTS_FILE = SCRIPT_DIR / "clippy_lints_list.md"
CARGO_TOML = ROOT / "Cargo.toml"
OUTPUT = ROOT / "clippy_compared_list.md"

EMOJI = {"allow": "\U0001f4a4", "warn": "\u26a0\ufe0f", "deny": "\U0001f6ab", "forbid": "\u2620\ufe0f"}
CAT_EMOJI = {
    "style": "\U0001f3a8",
    "correctness": "\U0001f534",
    "complexity": "\U0001f9e9",
    "perf": "\U0001f3ce\ufe0f",
    "pedantic": "\U0001f50d",
    "suspicious": "\U0001f575\ufe0f",
    "restriction": "\U0001f512",
    "nursery": "\U0001f331",
    "cargo": "\U0001f4e6",
}


def parse_lints_list(path: Path) -> tuple[dict[str, str], dict[str, str]]:
    """Return (defaults, categories) dicts keyed by lint name."""
    lines = path.read_text().splitlines()
    text_lines = [l.split("\u2192", 1)[1].strip() if "\u2192" in l else l.strip() for l in lines]
    text_lines = [l for l in text_lines if l]

    defaults: dict[str, str] = {}
    categories: dict[str, str] = {}
    idx = 0
    while idx < len(text_lines):
        line = text_lines[idx]
        if idx + 1 < len(text_lines):
            parts = text_lines[idx + 1].split()
            if len(parts) >= 2 and parts[1] in ("allow", "warn", "deny"):
                lint = re.sub(r"[^a-z_]", "", line.split()[0]) if line.split() else ""
                if lint:
                    defaults[lint] = parts[1]
                    categories[lint] = parts[0]
                idx += 2
                continue
        idx += 1
    return defaults, categories


def parse_cargo_clippy(path: Path) -> dict[str, str]:
    """Return configured clippy lint levels from Cargo.toml."""
    cargo = path.read_text()
    match = re.search(r"\[workspace\.lints\.clippy\](.*?)(?=\n\[)", cargo, re.DOTALL)
    section = match.group(1) if match else ""
    return {
        m.group(1): m.group(2)
        for m in re.finditer(r'^(\w+)\s*=\s*"(allow|warn|deny|forbid)"', section, re.MULTILINE)
    }


def main() -> None:
    defaults, categories = parse_lints_list(LINTS_FILE)
    configured = parse_cargo_clippy(CARGO_TOML)

    rows: list[str] = []
    forbid_count = 0
    for lint in sorted(defaults, key=lambda l: (categories.get(l, "zzz"), l)):
        default = defaults[lint]
        current = configured.get(lint, default)
        # Hide lints that are at forbid level in the current project
        if current == "forbid":
            forbid_count += 1
            continue
        cat = categories[lint]
        d_emoji = EMOJI.get(default, "\u2753")
        c_emoji = EMOJI.get(current, "\u2753")
        ce = CAT_EMOJI.get(cat, "")
        changed = "\U0001f504" if default != current else ""
        # ⚓ = explicitly pinned at this level in Cargo.toml (not just inherited default)
        pinned = "\u2693" if lint in configured else ""
        rows.append(f"| `{lint}` | {ce} {cat} | {d_emoji} {default} | {c_emoji} {current} | {changed} {pinned} |")

    overridden = sum(1 for l in defaults if configured.get(l, defaults[l]) != defaults[l])
    header = (
        "# Clippy Lint Configuration — Non-Forbid Lints\n\n"
        f"> **{len(defaults)}** total clippy lints — **{forbid_count}** at forbid, "
        f"**{len(rows)}** shown below (not yet at forbid level)\n\n"
        "| Lint | Category | Default | Current | |\n"
        "|------|----------|---------|---------|---|\n"
    )

    OUTPUT.write_text(header + "\n".join(rows) + "\n")
    print(f"Wrote {OUTPUT}: {len(rows)} non-forbid lints shown ({forbid_count} at forbid, hidden).")


if __name__ == "__main__":
    main()
