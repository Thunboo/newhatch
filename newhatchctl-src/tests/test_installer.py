#!/usr/bin/env python3
"""Exercise the real installer with local release fixtures and no root/network."""
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest


INSTALLER = Path(__file__).resolve().parents[2] / "install" / "unix.sh"
REPOSITORY = "https://github.com/Thunboo/newhatch"
TAG = "newhatchctl-v0.1.0"
PLATFORMS = ("linux-amd64", "linux-arm64", "macos-amd64", "macos-arm64")

# Every external command that could download or elevate privileges is a stub.
# Other utilities operate on the fixture directory only.
COMMAND_STUB = r'''
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

fixture = Path(os.environ["INSTALLER_TEST_ROOT"])
settings = json.loads((fixture / "settings.json").read_text())
command = Path(sys.argv[0]).name
args = sys.argv[1:]
with (fixture / "commands.jsonl").open("a") as record:
    record.write(json.dumps([command, *args]) + "\n")

if command == "uname":
    print(settings["os"] if args == ["-s"] else settings["arch"])
elif command == "id":
    print("1000")
elif command == "curl":
    assert args[0] == "--disable", args
    assert "--fail" in args and "--location" in args, args
    assert args[args.index("--proto") + 1] == "=https", args
    assert args[args.index("--proto-redir") + 1] == "=https", args
    url = args[-1]
    output = Path(args[args.index("--output") + 1])
    if url.endswith("/releases/latest"):
        if settings.get("missing_release"):
            sys.exit(22)
        assert output == Path("/dev/null"), output
        print(settings.get("latest_url", "https://github.com/Thunboo/newhatch/releases/tag/newhatchctl-v0.1.0"), end="")
    else:
        assert url.startswith("https://github.com/Thunboo/newhatch/releases/download/newhatchctl-v0.1.0/"), url
        assert output.is_relative_to(fixture), output
        asset = url.rsplit("/", 1)[1]
        if asset == settings.get("failed_download"):
            output.write_bytes(b"partial download")
            sys.exit(22)
        shutil.copyfile(fixture / "release" / asset, output)
elif command == "sudo":
    assert args[0] in ("mkdir", "mktemp", "install", "mv", "rm"), args
    for value in args:
        if value.startswith("/"):
            assert Path(value).is_relative_to(fixture), value
    # Simulate privilege to write our own read-only fixture directory only.
    (fixture / "destination").chmod(0o755)
    result = subprocess.run(args, check=False)
    sys.exit(result.returncode)
elif command in ("install", "mv"):
    for value in args:
        if value.startswith("/"):
            assert Path(value).is_relative_to(fixture), value
    if command == settings.get("failed_install_command"):
        sys.exit(1)
    result = subprocess.run([settings["real_commands"][command], *args], check=False)
    sys.exit(result.returncode)
elif command in ("sha256sum", "shasum"):
    assert command == "sha256sum" or args == ["-a", "256"], args
    print(hashlib.sha256(sys.stdin.buffer.read()).hexdigest(), " -")
elif command == "docker":
    raise AssertionError("The installer must not invoke Docker")
else:
    raise AssertionError(command)
'''


def executable(version):
    return (
        '#!/bin/bash\n'
        '[ "$#" -eq 1 ] && [ "$1" = --version ] || exit 80\n'
        f"printf 'newhatchctl {version}\\n'\n"
    ).encode()


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="newhatchctl-installer-test-")
        self.root = Path(self.temporary.name).resolve()
        self.destination = self.root / "destination"
        self.destination.mkdir()
        self.bin = self.root / "commands"
        self.bin.mkdir()
        self.downloads = self.root / "release"
        self.downloads.mkdir()
        self.scratch = self.root / "scratch with spaces"
        self.scratch.mkdir()
        self.binary = self.destination / "newhatchctl"
        self.settings = {
            "os": "Linux",
            "arch": "x86_64",
            "real_commands": {name: shutil.which(name) for name in ("install", "mv")},
        }
        for name in ("tar", "gzip", "mktemp", "chmod", "awk", "mkdir", "dirname", "rm"):
            (self.bin / name).symlink_to(shutil.which(name))
        for name in ("uname", "id", "curl", "sudo", "install", "mv", "sha256sum", "shasum", "docker"):
            path = self.bin / name
            path.write_text(f"#!{sys.executable}\n" + COMMAND_STUB)
            path.chmod(0o755)
        self.make_release()

    def tearDown(self):
        self.destination.chmod(0o755)
        self.temporary.cleanup()

    def make_release(self, version="0.1.0", member="newhatchctl", kind="file"):
        checksums = []
        for platform in PLATFORMS:
            archive = self.downloads / f"newhatchctl-{platform}.tar.gz"
            with tarfile.open(archive, "w:gz") as release:
                metadata = tarfile.TarInfo(member)
                metadata.mode = 0o755
                if kind == "file":
                    content = executable(version)
                    metadata.size = len(content)
                    release.addfile(metadata, io.BytesIO(content))
                else:
                    metadata.type = tarfile.SYMTYPE if kind == "symlink" else tarfile.LNKTYPE
                    metadata.linkname = str(self.root / "unrelated")
                    release.addfile(metadata)
            checksums.append(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n")
        (self.downloads / "SHA256SUMS").write_text("".join(checksums))

    def installed_version(self, version):
        self.binary.write_bytes(executable(version))
        self.binary.chmod(0o755)

    def run_installer(self, piped=False, install_dir=None):
        (self.root / "settings.json").write_text(json.dumps(self.settings))
        environment = os.environ.copy()
        environment.update({
            "PATH": str(self.bin),
            "TMPDIR": str(self.scratch),
            "NEWHATCHCTL_INSTALL_DIR": str(install_dir or self.destination),
            "INSTALLER_TEST_ROOT": str(self.root),
        })
        shell = os.environ.get("NEWHATCHCTL_TEST_BASH", "/bin/bash")
        command = [shell] if piped else [shell, str(INSTALLER)]
        result = subprocess.run(
            command,
            input=INSTALLER.read_text() if piped else None,
            cwd=self.root,
            env=environment,
            capture_output=True,
            text=True,
            timeout=20,
        )
        self.assertFalse(list(self.scratch.iterdir()), "download directory was not cleaned")
        self.assertFalse(list(self.destination.rglob(".newhatchctl.*")), "staging file was not cleaned")
        self.assertFalse(any(command[0] == "docker" for command in self.commands()))
        return result

    def commands(self):
        record = self.root / "commands.jsonl"
        return [json.loads(line) for line in record.read_text().splitlines()] if record.exists() else []

    def assert_failed_update(self, message, **kwargs):
        self.installed_version("0.0.9")
        original = self.binary.read_bytes()
        result = self.run_installer(**kwargs)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(message, result.stderr)
        self.assertEqual(self.binary.read_bytes(), original)
        return result

    def test_four_platforms_and_aliases(self):
        for os_name, arch, platform in (
            ("Linux", "x86_64", "linux-amd64"),
            ("Linux", "aarch64", "linux-arm64"),
            ("Darwin", "x86_64", "macos-amd64"),
            ("Darwin", "arm64", "macos-arm64"),
            ("Linux", "amd64", "linux-amd64"),
            ("Darwin", "aarch64", "macos-arm64"),
        ):
            with self.subTest(os=os_name, arch=arch):
                self.settings.update(os=os_name, arch=arch)
                self.installed_version("0.0.9")
                result = self.run_installer()
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("Installed newhatchctl 0.1.0", result.stdout)
                self.assertEqual(self.binary.read_bytes(), executable("0.1.0"))
                self.assertEqual(self.binary.stat().st_mode & 0o777, 0o755)
                downloads = [command[-1] for command in self.commands() if command[0] == "curl"]
                self.assertEqual(downloads[-3:], [
                    f"{REPOSITORY}/releases/latest",
                    f"{REPOSITORY}/releases/download/{TAG}/newhatchctl-{platform}.tar.gz",
                    f"{REPOSITORY}/releases/download/{TAG}/SHA256SUMS",
                ])

    def test_install_from_pipe_without_existing_binary(self):
        result = self.run_installer(piped=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.binary.is_file())
        self.assertIn("Run: newhatchctl", result.stdout)

    def test_same_version_does_not_download_or_install(self):
        self.installed_version("0.1.0")
        before = self.binary.stat()
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("already installed", result.stdout)
        self.assertEqual(self.binary.stat().st_ino, before.st_ino)
        self.assertEqual([command[0] for command in self.commands()], ["uname", "uname", "curl"])

    def test_update_replaces_inode(self):
        self.installed_version("0.0.9")
        before = self.binary.stat().st_ino
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotEqual(self.binary.stat().st_ino, before)
        self.assertFalse(any(command[0] == "sudo" for command in self.commands()))

    def test_privileged_commands_only_after_verification(self):
        self.installed_version("0.0.9")
        self.destination.chmod(0o555)
        result = self.run_installer(piped=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        commands = self.commands()
        sudo_commands = [command[1] for command in commands if command[0] == "sudo"]
        self.assertEqual(sudo_commands, ["mkdir", "mktemp", "install", "mv"])
        last_download = max(i for i, command in enumerate(commands) if command[0] == "curl")
        first_sudo = next(i for i, command in enumerate(commands) if command[0] == "sudo")
        self.assertLess(last_download, first_sudo)

    def test_new_install_directory(self):
        directory = self.destination / "new" / "bin"
        result = self.run_installer(install_dir=directory)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((directory / "newhatchctl").read_bytes(), executable("0.1.0"))

    def test_macos_checksum_fallback(self):
        (self.bin / "sha256sum").unlink()
        self.settings.update(os="Darwin", arch="arm64")
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(any(command[0] == "shasum" for command in self.commands()))

    def test_unsupported_platforms(self):
        for os_name, arch, message in (
            ("FreeBSD", "x86_64", "Supported operating systems"),
            ("Linux", "riscv64", "Supported architectures"),
        ):
            with self.subTest(os=os_name, arch=arch):
                self.settings.update(os=os_name, arch=arch)
                self.assert_failed_update(message)
                self.assertFalse(any(command[0] == "curl" for command in self.commands()))

    def test_missing_release(self):
        self.settings["missing_release"] = True
        self.assert_failed_update("Could not find the latest release")

    def test_invalid_latest_release(self):
        for url, message in (
            (f"{REPOSITORY}/releases", "GitHub did not return"),
            (f"{REPOSITORY}/releases/tag/newhatchctl-v0.1.0-beta", "not a stable"),
            (f"{REPOSITORY}/releases/tag/v0.1.0", "not a stable"),
            ("https://example.com/releases/tag/newhatchctl-v0.1.0", "GitHub did not return"),
        ):
            with self.subTest(url=url):
                self.settings["latest_url"] = url
                self.assert_failed_update(message)

    def test_failed_downloads(self):
        for asset in ("newhatchctl-linux-amd64.tar.gz", "SHA256SUMS"):
            with self.subTest(asset=asset):
                self.settings["failed_download"] = asset
                self.assert_failed_update("Could not download")

    def test_bad_checksum(self):
        with (self.downloads / "newhatchctl-linux-amd64.tar.gz").open("ab") as archive:
            archive.write(b"corrupted")
        self.assert_failed_update("SHA256 verification failed")

    def test_invalid_manifest(self):
        manifest = self.downloads / "SHA256SUMS"
        valid = manifest.read_text()
        for content in ("", valid + valid, "bad-hash  newhatchctl-linux-amd64.tar.gz\n"):
            with self.subTest(content=content):
                manifest.write_text(content)
                self.assert_failed_update("checksum")

    def test_archive_paths_and_links_rejected_before_extraction(self):
        unrelated = self.root / "unrelated"
        unrelated.write_bytes(b"keep me")
        original_mode = unrelated.stat().st_mode
        for member, kind, message in (
            ("../unrelated", "file", "must contain only"),
            ("newhatchctl", "symlink", "regular executable"),
            ("newhatchctl", "hardlink", "regular executable"),
        ):
            with self.subTest(kind=kind):
                self.make_release(member=member, kind=kind)
                self.assert_failed_update(message)
                self.assertEqual(unrelated.read_bytes(), b"keep me")
                self.assertEqual(unrelated.stat().st_mode, original_mode)

    def test_binary_version_mismatch(self):
        self.make_release(version="0.1.1")
        self.assert_failed_update("version does not match")

    def test_staging_and_rename_failure_preserve_old_binary(self):
        for command in ("install", "mv"):
            with self.subTest(command=command):
                self.settings["failed_install_command"] = command
                self.assert_failed_update("existing installation was not changed")

    def test_privileged_staging_failure_cleans_up(self):
        self.settings["failed_install_command"] = "install"
        self.installed_version("0.0.9")
        self.destination.chmod(0o555)
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.binary.read_bytes(), executable("0.0.9"))
        self.assertEqual([command[1] for command in self.commands() if command[0] == "sudo"], ["mkdir", "mktemp", "install", "rm"])

    def test_relative_install_directory_rejected(self):
        self.assert_failed_update("must be an absolute path", install_dir="relative/bin")


if __name__ == "__main__":
    unittest.main()
