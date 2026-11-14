"""Build packaged workspace crates against each other's packaged sources."""

import json
from pathlib import Path
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def cargo(*args, capture=False):
    return subprocess.run(
        ["cargo", *args],
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else None,
    )


def main():
    metadata = json.loads(
        cargo(
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--locked",
            capture=True,
        ).stdout
    )
    packages = {
        package["name"]: package
        for package in metadata["packages"]
        if package["id"] in metadata["workspace_members"] and package["publish"] != []
    }

    def dependencies(name):
        found = set()
        pending = [name]
        while pending:
            package = packages[pending.pop()]
            for dependency in package["dependencies"]:
                target = dependency["name"]
                if target in packages and target not in found:
                    found.add(target)
                    pending.append(target)
        return sorted(found)

    # Cargo's verifier prefers an already-published copy of the same version.
    # Extract every archive first so patches refer only to shipped files.
    cargo(
        "package",
        "--workspace",
        "--locked",
        "--all-features",
        "--registry",
        "crates-io",
        "--no-verify",
        "--allow-dirty",
    )
    target = Path(metadata["target_directory"])
    with tempfile.TemporaryDirectory(prefix="fujo-packages-") as temporary:
        destination = Path(temporary)
        directories = {}
        for name, package in packages.items():
            stem = f"{name}-{package['version']}"
            with tarfile.open(target / "package" / f"{stem}.crate") as source:
                source.extractall(destination, filter="data")
            directories[name] = destination / stem
        for name, directory in directories.items():
            patches = []
            for dependency in dependencies(name):
                path = json.dumps(directories[dependency].as_posix())
                patches.extend([
                    "--config",
                    f"patch.crates-io.{dependency}.path={path}",
                ])
            # Reconcile only the extracted lockfile's internal registry entries
            # with path patches. The workspace lockfile stays locked.
            cargo(
                "build",
                "--manifest-path",
                str(directory / "Cargo.toml"),
                "--all-features",
                "--target-dir",
                str(target / "package-check"),
                *patches,
            )
            print(f"Verified packaged sources: {directory.name}", flush=True)


if __name__ == "__main__":
    main()
