#!/usr/bin/env python3
"""Validate the shared crate version and internal dependency requirements."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tomllib


def validate(metadata, version, tag=None):
    if tag is not None and tag != f"v{version}":
        raise ValueError(f"Tag {tag!r} must match workspace version v{version}")
    members = {
        package["id"]: package
        for package in metadata["packages"]
        if package["id"] in metadata["workspace_members"]
    }
    published = {p["name"]: p for p in members.values() if p["publish"] != []}
    if not published:
        raise ValueError("The workspace has no publishable crates")
    by_path = {Path(p["manifest_path"]).parent: p for p in members.values()}
    for package in published.values():
        name = package["name"]
        if package["version"] != version:
            raise ValueError(f"{name} must use workspace version {version}")
        if package["publish"] is not None and "crates-io" not in package["publish"]:
            raise ValueError(f"{name} does not allow publication to crates.io")
        for dependency in package["dependencies"]:
            if dependency.get("path") is None or dependency["kind"] == "dev":
                continue
            target = by_path.get(Path(dependency["path"]))
            if target is None or target["name"] not in published:
                raise ValueError(
                    f"{name} has an unpublished path dependency: {dependency['name']}"
                )
            if dependency["req"] not in (f"^{version}", f"={version}"):
                raise ValueError(
                    f"{name} must depend on {target['name']} at version {version}, "
                    f"not {dependency['req']}"
                )
    return sorted(published)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--tag",
        default=os.environ.get("RELEASE_TAG") or None,
        help="Release tag, including the v prefix (defaults to RELEASE_TAG)",
    )
    args = parser.parse_args()
    root = Path.cwd()
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--locked", "--format-version", "1"],
            cwd=root,
            text=True,
        )
    )
    version = manifest["workspace"]["package"]["version"]
    try:
        names = validate(metadata, version, args.tag)
    except ValueError as error:
        parser.exit(1, f"check-release: {error}\n")
    print(f"Release v{version}: {', '.join(names)}")


if __name__ == "__main__":
    main()
