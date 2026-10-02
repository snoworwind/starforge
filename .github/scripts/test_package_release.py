"""Check the Windows download layout, excluded local files and checksums."""

import hashlib
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

from package_release import WINDOWS_TARGET, package_release


class PackageReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="starforge-package-test-")
        self.addCleanup(self.temporary.cleanup)
        self.repo = Path(self.temporary.name)
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)
        self.files = {
            "assets/models/creatures/test.gltf": b"model",
            "assets/shaders/test.wgsl": b"shader",
            "assets/licenses/test.txt": b"asset license",
            "assets/fonts/test.ttf": b"font",
            "assets/audio/test.wav": b"audio",
            "README.md": b"instructions",
            "CREDITS.md": b"credits",
            "LICENSE": b"MIT",
            "RELEASING.md": b"release instructions",
        }
        for relative, content in self.files.items():
            path = self.repo / "starforge-bevy" / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        subprocess.run(["git", "-c", "core.excludesfile=" + str(self.repo / ".git/info/exclude"),
                        "-C", str(self.repo), "add", "starforge-bevy"], check=True)
        # These files exist locally but must never enter the shipped package.
        for relative in ("assets/models/external/secret.glb", "assets/models/earth/scene.gltf",
                         "assets/local-debug.txt", "saves/world.json"):
            path = self.repo / "starforge-bevy" / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"local only")

    def test_windows_archive(self):
        binary = self.repo / "build" / "starforge-bevy.exe"
        binary.parent.mkdir()
        binary.write_bytes(b"test executable")
        archive = package_release(self.repo, "v0.1.0-rc.1", WINDOWS_TARGET, binary, self.repo / "dist")
        root = "starforge-v0.1.0-rc.1-" + WINDOWS_TARGET
        expected = {root + "/" + path for path in self.files}
        expected.add(root + "/starforge-bevy.exe")
        with zipfile.ZipFile(archive) as packed:
            actual = {entry.filename for entry in packed.infolist() if not entry.is_dir()}
            self.assertEqual(packed.read(root + "/starforge-bevy.exe"), b"test executable")
            self.assertEqual(actual, expected)
        checksum = Path(str(archive) + ".sha256").read_bytes()
        expected_checksum = hashlib.sha256(archive.read_bytes()).hexdigest() + "  " + archive.name + "\n"
        self.assertEqual(checksum, expected_checksum.encode("utf-8"))

    def test_missing_executable_fails(self):
        with self.assertRaisesRegex(ValueError, "executable"):
            package_release(self.repo, "v0.1.0", "x86_64-pc-windows-msvc",
                            self.repo / "starforge-bevy.exe", self.repo / "dist")

    def test_empty_executable_fails(self):
        binary = self.repo / "starforge-bevy.exe"
        binary.touch()
        with self.assertRaisesRegex(ValueError, "executable"):
            package_release(self.repo, "v0.1.0", WINDOWS_TARGET, binary, self.repo / "dist")

    def test_unsafe_tag_fails(self):
        with self.assertRaisesRegex(ValueError, "version tag"):
            package_release(self.repo, "../../escape", "x86_64-pc-windows-msvc",
                            self.repo / "starforge-bevy.exe", self.repo / "dist")


if __name__ == "__main__":
    unittest.main()
