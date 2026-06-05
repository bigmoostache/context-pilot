#!/usr/bin/env python3
"""Generate SQL to sync lints.current_level with Cargo.toml overrides.

Usage:
    1. Dump lints table from entity_sql:
         entity_sql(sql="SELECT * FROM lints", output_path="/tmp/lints_dump.md")
    2. Run this script:
         python3 .github/checks/update_lint_levels.py /tmp/lints_dump.md
    3. Import the generated SQL:
         entity_sql(request_path="/tmp/update_lint_levels.sql")

Reads:
    - <dump_file>   (entity_sql markdown table output)
    - Cargo.toml    ([workspace.lints.clippy] and [workspace.lints.rust] sections)

Writes:
    - /tmp/update_lint_levels.sql  (or custom path via second argument)
"""

import re
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent.parent
CARGO_TOML = ROOT / "Cargo.toml"
DEFAULT_OUTPUT = Path("/tmp/update_lint_levels.sql")


def parse_dump(path: Path) -> list[str]:
    """Extract lint names from an entity_sql markdown table dump."""
    names: list[str] = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line.startswith("|"):
            continue
        # Skip header and separator rows
        if line.startswith("| name") or line.startswith("|---"):
            continue
        cols = [c.strip() for c in line.split("|")]
        # cols[0] is empty (before first |), cols[1] is name
        if len(cols) >= 2 and cols[1]:
            names.append(cols[1])
    return names


def parse_cargo_overrides(path: Path) -> dict[str, str]:
    """Return {lint_name: level} from both [workspace.lints.clippy] and [workspace.lints.rust]."""
    cargo = path.read_text()
    overrides: dict[str, str] = {}

    # Clippy lints → prefixed with clippy::, underscores → hyphens
    match = re.search(r"\[workspace\.lints\.clippy\](.*?)(?=\n\[)", cargo, re.DOTALL)
    if match:
        for m in re.finditer(
            r'^(\w+)\s*=\s*"(allow|warn|deny|forbid)"',
            match.group(1),
            re.MULTILINE,
        ):
            name = f"clippy::{m.group(1).replace('_', '-')}"
            overrides[name] = m.group(2)

    # Rust lints → no prefix, underscores → hyphens
    match = re.search(r"\[workspace\.lints\.rust\](.*?)(?=\n\[)", cargo, re.DOTALL)
    if match:
        for m in re.finditer(
            r'^(\w+)\s*=\s*"(allow|warn|deny|forbid)"',
            match.group(1),
            re.MULTILINE,
        ):
            name = m.group(1).replace("_", "-")
            overrides[name] = m.group(2)

    return overrides


def escape_sql(s: str) -> str:
    """Escape single quotes for SQL string literals."""
    return s.replace("'", "''")


def main() -> None:
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <lints_dump.md> [output.sql]", file=sys.stderr)
        sys.exit(1)

    dump_path = Path(sys.argv[1])
    output = Path(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT_OUTPUT

    if not dump_path.exists():
        print(f"Error: dump file not found: {dump_path}", file=sys.stderr)
        sys.exit(1)

    lint_names = parse_dump(dump_path)
    if not lint_names:
        print("Error: no lint names found in dump file", file=sys.stderr)
        sys.exit(1)

    overrides = parse_cargo_overrides(CARGO_TOML)

    stmts: list[str] = []
    updated = 0
    cleared = 0

    for name in lint_names:
        level = overrides.get(name)
        if level:
            stmts.append(
                f"UPDATE lints SET current_level = '{level}' "
                f"WHERE name = '{escape_sql(name)}';"
            )
            updated += 1
        else:
            stmts.append(
                f"UPDATE lints SET current_level = NULL "
                f"WHERE name = '{escape_sql(name)}';"
            )
            cleared += 1

    output.write_text("\n".join(stmts) + "\n")
    print(f"Wrote {output}: {updated} overrides set, {cleared} reset to NULL, {len(stmts)} total statements")


if __name__ == "__main__":
    main()
