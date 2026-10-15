#!/usr/bin/env python3
"""Compile isolated features and assert their normal dependency boundaries."""

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OPTIONAL = {"fujo-complete", "rpassword", "regex", "url", "chrono"}


def cargo(*args):
    result = subprocess.run(
        ["cargo", *args], cwd=ROOT, capture_output=True, text=True, check=False
    )
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    return result.stdout


cases = [
    ("minimal", ["--no-default-features"], set()),
    ("default", [], {"fujo-complete", "rpassword"}),
    ("all", ["--all-features"], OPTIONAL),
]
for feature, dependencies in [
    ("completion", {"fujo-complete"}),
    ("terminal-prompts", {"rpassword"}),
    ("terminal-activity", set()),
    ("regex", {"regex"}),
    ("url", {"url"}),
    ("chrono", {"chrono"}),
]:
    cases.append((
        feature,
        ["--no-default-features", "--features", feature],
        dependencies,
    ))

for label, flags, expected in cases:
    cargo("clippy", "-p", "fujo", "--lib", "--locked", *flags, "--", "-D", "warnings")
    tree = cargo(
        "tree",
        "-p",
        "fujo",
        "--edges",
        "normal",
        "--target",
        "all",
        "--prefix",
        "none",
        "--format",
        "{p}",
        "--locked",
        *flags,
    )
    dependencies = {line.split()[0] for line in tree.splitlines()}
    actual = dependencies & OPTIONAL
    if actual != expected:
        raise AssertionError(
            f"{label}: expected optional dependencies {expected}, got {actual}\n{tree}"
        )
    cargo("test", "-p", "fujo", "--test", "feature_contract", "--locked", *flags)
    print(
        f"{label}: compiled, integration tests passed; optional dependencies {sorted(actual)}",
        flush=True,
    )

for package, forbidden in [
    ("fujo-output", OPTIONAL | {"fujo", "fujo-core"}),
    ("fujo-complete", OPTIONAL | {"fujo", "fujo-output"}),
]:
    cargo("test", "-p", package, "--no-default-features", "--locked")
    tree = cargo(
        "tree",
        "-p",
        package,
        "--no-default-features",
        "--edges",
        "normal",
        "--target",
        "all",
        "--prefix",
        "none",
        "--format",
        "{p}",
        "--locked",
    )
    dependencies = {line.split()[0] for line in tree.splitlines()} - {package}
    if dependencies & forbidden:
        raise AssertionError(
            f"{package}: unexpected dependencies {dependencies & forbidden}\n{tree}"
        )
    print(
        f"{package}: standalone minimal tests and dependency boundary passed",
        flush=True,
    )
