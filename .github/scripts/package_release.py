"""Package STARFORGE for Windows using only versioned assets (Python 3.8+)."""

import argparse
import hashlib
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


WINDOWS_TARGET = "x86_64-pc-windows-msvc"


def package_release(repo, tag, target, binary, output):
    if not re.fullmatch(r"v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", tag):
        raise ValueError("Expected a version tag such as v0.1.0")
    if target != WINDOWS_TARGET:
        raise ValueError("Unsupported release target: " + target)
    binary = Path(binary).resolve()
    expected_name = "starforge-bevy.exe"
    if binary.name != expected_name or not binary.is_file() or binary.stat().st_size == 0:
        raise ValueError("Missing or invalid release executable: " + str(binary))

    repo = Path(repo).resolve()
    output = Path(output).resolve()
    tracked = subprocess.check_output(
        ["git", "ls-files", "-z", "--", "starforge-bevy/assets/"], cwd=repo
    ).decode("utf-8").split("\0")
    assets = [
        path for path in tracked if path
        and not path.startswith((
            "starforge-bevy/assets/models/external/",
            "starforge-bevy/assets/models/earth/",
        ))
    ]
    for group in ("models", "shaders", "licenses"):
        if not any(path.startswith("starforge-bevy/assets/" + group + "/") for path in assets):
            raise ValueError("No versioned assets in " + group)

    output.mkdir(parents=True, exist_ok=True)
    name = "starforge-{}-{}".format(tag, target)
    with tempfile.TemporaryDirectory(prefix="starforge-release-") as temporary:
        package = Path(temporary) / name
        package.mkdir()
        executable = package / expected_name
        shutil.copyfile(binary, executable)
        for relative in assets:
            destination = package / Path(relative).relative_to("starforge-bevy")
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(repo / relative, destination)
        for document in ("README.md", "CREDITS.md", "LICENSE", "RELEASING.md"):
            shutil.copyfile(repo / "starforge-bevy" / document, package / document)
        archive = Path(shutil.make_archive(
            str(output / name), "zip", root_dir=temporary, base_dir=name
        ))

    digest = hashlib.sha256()
    with archive.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    # Use LF even on Windows so Linux's sha256sum can consume the manifest.
    Path(str(archive) + ".sha256").write_bytes(
        "{}  {}\n".format(digest.hexdigest(), archive.name).encode("utf-8")
    )
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--target", required=True, choices=[WINDOWS_TARGET])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    print(package_release(repo, arguments.tag, arguments.target, arguments.binary, arguments.output))


if __name__ == "__main__":
    main()
