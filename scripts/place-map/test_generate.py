#!/usr/bin/env python3
import hashlib
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import zipfile
from copy import deepcopy


SCRIPT = Path(__file__).with_name("generate.py")
SPEC = importlib.util.spec_from_file_location("place_map_generate", SCRIPT)
assert SPEC and SPEC.loader
generate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(generate)


class SourceBoundaryTests(unittest.TestCase):
    def test_checked_manifest_has_the_full_pinned_source_set(self):
        manifest, digest = generate.load_manifest()
        sources = generate.validate_manifest(manifest)
        self.assertEqual({source["name"] for source in sources}, generate.EXPECTED_NAMES)
        self.assertEqual(len(digest), 64)
        self.assertTrue(all(source.get("members") for source in sources if source["name"] != "earth_relief_06m_g"))

    def test_duplicate_sources_are_rejected(self):
        manifest, _ = generate.load_manifest()
        duplicate = deepcopy(manifest)
        duplicate["sources"].append(deepcopy(duplicate["sources"][0]))
        with self.assertRaises(generate.VerificationError):
            generate.validate_manifest(duplicate)

    def test_zip_verification_requires_all_declared_sidecars(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = root / "example.zip"
            with zipfile.ZipFile(payload, "w") as archive:
                archive.writestr("example.shp", b"shape")
                archive.writestr("example.dbf", b"attributes")
            source = {
                "name": "example",
                "artifact": payload.name,
                "sha256": hashlib.sha256(payload.read_bytes()).hexdigest(),
                "size_bytes": payload.stat().st_size,
                "members": ["example.shp", "example.dbf", "example.shx", "example.prj"],
                "layer": "example",
            }
            with self.assertRaises(generate.VerificationError):
                generate.verify_source(source, root)

    def test_symlinked_artifact_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target.bin"
            target.write_bytes(b"not a source")
            artifact = root / "artifact.bin"
            artifact.symlink_to(target)
            source = {
                "name": "example",
                "artifact": artifact.name,
                "sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
                "size_bytes": target.stat().st_size,
                "member": artifact.name,
                "layer": "example",
            }
            with self.assertRaises(generate.VerificationError):
                generate.verify_source(source, root)

    def test_receipt_bytes_are_deterministic(self):
        value = {"schema": 1, "sources": [{"name": "land", "size_bytes": 3}]}
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.json"
            second = Path(directory) / "second.json"
            generate.write_receipt(first, value)
            generate.write_receipt(second, value)
            self.assertEqual(first.read_bytes(), second.read_bytes())

    def test_cli_requires_an_explicit_offline_source_dir(self):
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--check"],
            capture_output=True,
            text=True,
            env={"PATH": str(Path(sys.executable).parent)},
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("network fallback is disabled", result.stderr)

if __name__ == "__main__":
    unittest.main()
