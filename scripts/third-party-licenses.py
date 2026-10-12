#!/usr/bin/env python3
"""Write the licenses of every crate that goes into the macOS arm64 build to stdout.

Reads `cargo metadata` for the build graph, and for each dependency prints its name, version,
declared license, source, and the text of the license/notice files found beside its sources (or in
the repository root it lives in). Run by scripts/package-app.sh; needs no network.
"""
import json
import os
import subprocess
import sys

NAMES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")


def license_files(directory):
    try:
        entries = sorted(os.listdir(directory))
    except OSError:
        return []
    found = []
    for entry in entries:
        path = os.path.join(directory, entry)
        if os.path.isfile(path) and entry.upper().startswith(NAMES):
            found.append(path)
    return found


def find_texts(manifest_path):
    """License files beside the crate, else in the nearest parent that has some (git workspaces)."""
    directory = os.path.dirname(manifest_path)
    for _ in range(4):
        files = license_files(directory)
        if files:
            return files
        parent = os.path.dirname(directory)
        if parent == directory:
            break
        directory = parent
    return []


def main():
    meta = json.loads(
        subprocess.check_output(
            [
                "cargo", "metadata", "--format-version", "1", "--locked", "--offline",
                "--filter-platform", "aarch64-apple-darwin",
            ]
        )
    )
    root = meta["resolve"]["root"]
    in_graph = {node["id"] for node in meta["resolve"]["nodes"]}
    packages = sorted(
        (p for p in meta["packages"] if p["id"] in in_graph and p["id"] != root),
        key=lambda p: (p["name"], p["version"]),
    )
    out = sys.stdout
    out.write("Third-party licenses for Ambien\n")
    out.write("================================\n\n")
    out.write(f"Ambien itself is MIT licensed (see LICENSE). It is built from {len(packages)} Rust crates:\n\n")
    for p in packages:
        out.write(f"* {p['name']} {p['version']} - {p['license'] or 'see license files'}\n")
    out.write("\n")
    first_seen = {}  # a text already printed under another crate is referenced, not repeated
    for p in packages:
        out.write("=" * 78 + "\n")
        out.write(f"{p['name']} {p['version']}\n")
        out.write(f"License: {p['license'] or 'see below'}\n")
        if p.get("repository"):
            out.write(f"Source: {p['repository']}\n")
        out.write("=" * 78 + "\n")
        seen = set()
        texts = find_texts(p["manifest_path"])
        for path in texts:
            with open(path, encoding="utf-8", errors="replace") as handle:
                body = handle.read().strip()
            if body in seen:
                continue
            seen.add(body)
            name = os.path.basename(path)
            if body in first_seen:
                out.write(f"\n--- {name}: the same text as {first_seen[body]} ---\n")
                continue
            first_seen[body] = f"{p['name']} {p['version']} ({name})"
            out.write(f"\n--- {name} ---\n{body}\n")
        if not texts:
            out.write("\n(no license file shipped with the sources; the license above is the one the crate declares)\n")
        out.write("\n")


if __name__ == "__main__":
    main()
