#!/usr/bin/env python3

"""Release preflight rejects tags and manifests that could publish mixed versions."""

import copy
from pathlib import Path
import runpy
import unittest


validate = runpy.run_path(Path(__file__).with_name("check-release.py"))["validate"]


def metadata(version="0.1.0"):
    return {
        "workspace_members": ["error", "app"],
        "packages": [
            {
                "id": "error",
                "name": "fujo-error",
                "version": version,
                "publish": None,
                "manifest_path": "/workspace/error/Cargo.toml",
                "dependencies": [],
            },
            {
                "id": "app",
                "name": "fujo",
                "version": version,
                "publish": ["crates-io"],
                "manifest_path": "/workspace/app/Cargo.toml",
                "dependencies": [
                    {
                        "name": "fujo-error",
                        "path": "/workspace/error",
                        "req": f"^{version}",
                        "kind": None,
                    }
                ],
            },
        ],
    }


class ReleaseValidation(unittest.TestCase):
    def test_matching_stable_and_prerelease_tags(self):
        for version in ["0.1.0", "0.2.0-rc.1"]:
            self.assertEqual(
                validate(metadata(version), version, f"v{version}"),
                ["fujo", "fujo-error"],
            )

    def test_rejects_wrong_tag_and_missing_prefix(self):
        for tag in ["v0.2.0", "0.1.0", "v0.1.0-extra"]:
            with (
                self.subTest(tag=tag),
                self.assertRaisesRegex(ValueError, "must match"),
            ):
                validate(metadata(), "0.1.0", tag)

    def test_rejects_mixed_versions(self):
        data = metadata()
        data["packages"][0]["version"] = "0.2.0"
        with self.assertRaisesRegex(ValueError, "workspace version"):
            validate(data, "0.1.0")

    def test_rejects_stale_internal_requirement(self):
        data = metadata()
        data["packages"][1]["dependencies"][0]["req"] = "^0.0.1"
        with self.assertRaisesRegex(ValueError, "must depend"):
            validate(data, "0.1.0")

    def test_rejects_unpublished_dependency(self):
        data = metadata()
        data["packages"][0]["publish"] = []
        with self.assertRaisesRegex(ValueError, "unpublished path dependency"):
            validate(data, "0.1.0")

    def test_rejects_other_registry(self):
        data = metadata()
        data["packages"][0]["publish"] = ["private"]
        with self.assertRaisesRegex(ValueError, "does not allow"):
            validate(data, "0.1.0")

    def test_private_dev_dependency_is_not_published(self):
        data = metadata()
        private = copy.deepcopy(data["packages"][0])
        private.update(
            id="test-support",
            name="test-support",
            publish=[],
            manifest_path="/workspace/tests/Cargo.toml",
        )
        data["workspace_members"].append(private["id"])
        data["packages"].append(private)
        data["packages"][1]["dependencies"].append({
            "name": "test-support",
            "path": "/workspace/tests",
            "req": "*",
            "kind": "dev",
        })
        self.assertEqual(validate(data, "0.1.0"), ["fujo", "fujo-error"])


if __name__ == "__main__":
    unittest.main()
