#!/usr/bin/env python3
"""Compare project's Cargo.toml rust lint config against rustc defaults.

Usage:
    python3 .github/checks/rust_lints_compare.py

Reads:
    - .github/checks/rust_lints_list.md  (all rustc lints + defaults)
    - Cargo.toml                         ([workspace.lints.rust] section)

Writes:
    - rust_lints_compared_list.md
"""

import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent.parent
LINTS_FILE = SCRIPT_DIR / "rust_lints_list.md"
CARGO_TOML = ROOT / "Cargo.toml"
OUTPUT = ROOT / "rust_lints_compared_list.md"

EMOJI = {"allow": "\U0001f4a4", "warn": "\u26a0\ufe0f", "deny": "\U0001f6ab"}

# Toggle: hide lines where current == default (warn) and irrelevant lines
HIDE_UNMODIFIED_AND_IRRELEVANT = True


def normalize(name: str) -> str:
    """Normalize lint name: strip non-alphanum except underscore, convert hyphens."""
    return re.sub(r"-", "_", name.strip())


def parse_lints_list(path: Path) -> tuple[dict[str, str], dict[str, str]]:
    """Return ({lint_name: default_level}, {lint_name: irrelevant_reason}) from rust_lints_list.md."""
    text = path.read_text()
    defaults: dict[str, str] = {}
    irrelevant: dict[str, str] = {}
    current_level = "allow"

    for raw_line in text.splitlines():
        # Strip line-number prefix (e.g. "     1→")
        line = raw_line.split("\u2192", 1)[1].strip() if "\u2192" in raw_line else raw_line.strip()
        if not line:
            continue

        # Section headers
        if line == "[allowed by default]":
            current_level = "allow"
            continue
        if line == "[warn by default]":
            current_level = "warn"
            continue
        if line == "[deny by default]":
            current_level = "deny"
            continue

        # Skip other section headers
        if line.startswith("["):
            continue

        # Check for #irrelevant tag
        reason = ""
        if "#irrelevant" in line:
            parts = line.split("#irrelevant", 1)
            line = parts[0].strip()
            reason = parts[1].strip()

        lint = normalize(line)
        if lint and re.fullmatch(r"[a-z_][a-z0-9_]*", lint):
            defaults[lint] = current_level
            if reason:
                irrelevant[lint] = reason

    return defaults, irrelevant


def parse_cargo_rust(path: Path) -> dict[str, str]:
    """Return configured rust lint levels from Cargo.toml [workspace.lints.rust]."""
    cargo = path.read_text()
    # Match the [workspace.lints.rust] section up to the next [section]
    match = re.search(r"\[workspace\.lints\.rust\](.*?)(?=\n\[)", cargo, re.DOTALL)
    section = match.group(1) if match else ""
    return {
        m.group(1): m.group(2)
        for m in re.finditer(r'^(\w+)\s*=\s*"(allow|warn|deny)"', section, re.MULTILINE)
    }


def main() -> None:
    defaults, irrelevant = parse_lints_list(LINTS_FILE)
    configured = parse_cargo_rust(CARGO_TOML)

    rows: list[str] = []
    hidden = 0
    for lint in sorted(defaults):
        default = defaults[lint]
        current = configured.get(lint, default)
        is_irrelevant = lint in irrelevant
        is_unmodified = default == current

        if HIDE_UNMODIFIED_AND_IRRELEVANT and (is_irrelevant or current == "deny"):
            hidden += 1
            continue

        d_emoji = EMOJI.get(default, "\u2753")
        c_emoji = EMOJI.get(current, "\u2753")
        changed = "\U0001f504" if default != current else ""
        irr = irrelevant.get(lint, "")
        irr_col = f"\U0001f6d1 {irr}" if irr else ""
        rows.append(f"| `{lint}` | {d_emoji} {default} | {c_emoji} {current} | {changed} | {irr_col} |")

    overridden = sum(1 for l in defaults if l not in irrelevant and configured.get(l, defaults[l]) != defaults[l])
    at_default = sum(1 for l in defaults if l not in irrelevant and configured.get(l, defaults[l]) == defaults[l])
    irr_count = len(irrelevant)
    # Lints in Cargo.toml but not in rustc list (extras)
    extras = sorted(set(configured) - set(defaults))
    extra_rows = []
    for lint in extras:
        current = configured[lint]
        c_emoji = EMOJI.get(current, "\u2753")
        extra_rows.append(f"| `{lint}` | \u2753 ??? | {c_emoji} {current} | \U0001f50d | |")

    hidden_note = f", **{hidden}** hidden" if HIDE_UNMODIFIED_AND_IRRELEVANT and hidden else ""
    header = (
        "# Rust Compiler Lint Configuration vs Defaults\n\n"
        f"> **{len(defaults)}** known rustc lints — **{overridden}** overridden, "
        f"**{at_default}** at default, **{irr_count}** irrelevant{hidden_note}\n\n"
        "| Lint | Default | Current | | Irrelevant |\n"
        "|------|---------|---------|---|---|\n"
    )

    content = header + "\n".join(rows)
    if extra_rows:
        content += (
            "\n\n## Configured but not in rustc list\n\n"
            "| Lint | Default | Current | | Irrelevant |\n"
            "|------|---------|---------|---|---|\n"
            + "\n".join(extra_rows)
        )
    content += "\n"

    OUTPUT.write_text(content)
    print(f"Wrote {OUTPUT}: {len(defaults)} lints ({overridden} overridden, {irr_count} irrelevant), {len(extras)} extras.")


if __name__ == "__main__":
    main()
