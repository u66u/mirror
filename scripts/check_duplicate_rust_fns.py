#!/usr/bin/env python3
"""Fail when Rust free functions are redefined across files.

This is intentionally narrow: it checks free `fn` items, not impl/trait
methods. Duplicate free helpers are usually drift in this codebase; move them
into a named module and import them instead.
"""

from __future__ import annotations

import argparse
import dataclasses
import pathlib
import re
import sys
from collections import defaultdict


FN_RE = re.compile(
    r"^\s*(?:(?:pub(?:\([^)]*\))?)\s+)?(?:(?:async|const|unsafe|extern\s+\"[^\"]+\")\s+)*fn\s+([A-Za-z_][A-Za-z0-9_]*)\b"
)
ITEM_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?P<kind>impl|trait|mod|fn|struct|enum)\b"
)
ALLOWED_DUPLICATE_NAMES = {"main"}


@dataclasses.dataclass(frozen=True)
class FunctionDef:
    name: str
    path: pathlib.Path
    line: int


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "roots",
        nargs="*",
        type=pathlib.Path,
        default=[pathlib.Path("src/backend/src"), pathlib.Path("src/backend/tests")],
        help="Rust files or directories to scan.",
    )
    args = parser.parse_args()

    duplicates = find_duplicates(args.roots)
    if not duplicates:
        return 0

    print("duplicate Rust free function names found; import a shared helper instead:\n")
    for name in sorted(duplicates):
        print(f"{name}:")
        for definition in sorted(duplicates[name], key=lambda item: (str(item.path), item.line)):
            print(f"  {definition.path}:{definition.line}")
        print()
    return 1


def find_duplicates(roots: list[pathlib.Path]) -> dict[str, list[FunctionDef]]:
    definitions: dict[str, list[FunctionDef]] = defaultdict(list)
    for path in rust_files(roots):
        for function in free_functions(path):
            definitions[function.name].append(function)

    return {
        name: defs
        for name, defs in definitions.items()
        if len(defs) > 1 and name not in ALLOWED_DUPLICATE_NAMES
    }


def rust_files(roots: list[pathlib.Path]) -> list[pathlib.Path]:
    files: list[pathlib.Path] = []
    for root in roots:
        if root.is_file() and root.suffix == ".rs":
            files.append(root)
        elif root.is_dir():
            files.extend(path for path in root.rglob("*.rs") if "target" not in path.parts)
    return sorted(files)


def free_functions(path: pathlib.Path) -> list[FunctionDef]:
    functions: list[FunctionDef] = []
    contexts: list[tuple[str, int]] = []
    depth = 0

    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        stripped = strip_line_comment(line)
        contexts = [(kind, start_depth) for kind, start_depth in contexts if depth > start_depth]

        item = ITEM_RE.match(stripped)
        pending_kind = item.group("kind") if item else None
        in_impl_or_trait = any(kind in {"impl", "trait"} for kind, _ in contexts)

        match = FN_RE.match(stripped)
        if match and not in_impl_or_trait:
            functions.append(FunctionDef(match.group(1), path, line_number))

        opens = stripped.count("{")
        closes = stripped.count("}")
        if pending_kind in {"impl", "trait", "mod"} and opens > 0:
            contexts.append((pending_kind, depth))
        depth += opens - closes

    return functions


def strip_line_comment(line: str) -> str:
    in_string = False
    escaped = False
    for index, char in enumerate(line):
        if escaped:
            escaped = False
            continue
        if char == "\\":
            escaped = True
            continue
        if char == '"':
            in_string = not in_string
            continue
        if not in_string and line[index : index + 2] == "//":
            return line[:index]
    return line


if __name__ == "__main__":
    sys.exit(main())
