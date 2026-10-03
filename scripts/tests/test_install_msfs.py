"""install-msfs.sh 的源码安装/卸载回归：假 HOME、假 curl/unzip，不联网。"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
INSTALLER = SCRIPTS / "install-msfs.sh"
PROTO_VERSION = "0.4.0"


def _write(path: Path, text: str, *, mode: int | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    if mode is not None:
        path.chmod(mode)


def _bundle(root: Path, bridge: str = "bridge\n") -> Path:
    """造一个最小但完整的 dist/fly-ruler-msfs（SHA256SUMS 是真实校验和）。"""
    bundle = root / "dist" / "fly-ruler-msfs"
    _write(bundle / "fly-ruler-msfs-bridge.exe", bridge)
    _write(bundle / "SimConnect.dll", "simconnect\n")
    _write(bundle / "web" / "dist" / "index.html", "<html></html>\n")
    files = "fly-ruler-msfs-bridge.exe SimConnect.dll web/dist/index.html"
    checksums = subprocess.run(
        ["sha256sum", *files.split()],
        cwd=bundle,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    _write(bundle / "SHA256SUMS", checksums)
    return bundle


class InstallerCase(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="install-msfs-test.")
        self.root = Path(self._tmp.name)
        self.home = self.root / "home"
        self.home.mkdir()
        # 假 curl/unzip：一旦被调用就写日志并失败，用来证明源码模式确实不联网。
        self.shims = self.root / "shims"
        self.shims.mkdir()
        self.shim_log = self.root / "shims.log"
        for tool in ("curl", "unzip"):
            _write(
                self.shims / tool,
                "#!/bin/sh\n"
                f'printf \'%s\\n\' "{tool}" >> "$SHIM_LOG"\n'
                "exit 7\n",
                mode=0o755,
            )
        self.src = self.root / "fly_ruler_proto"
        _write(self.src / "scripts" / "package_msfs_bundle.sh", "")
        _write(
            self.src / "Cargo.toml",
            f'[workspace.package]\nversion = "{PROTO_VERSION}"\n',
        )
        self.prefix = self.home / ".local" / "share" / "fly-ruler-msfs"
        self.config = self.home / ".config" / "fly-ruler-msfs" / "fly-ruler-msfs.toml"

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def run_installer(
        self,
        *args: str,
        env: dict[str, str] | None = None,
        expect: int | None = 0,
    ) -> subprocess.CompletedProcess[str]:
        merged = os.environ | {
            "HOME": str(self.home),
            "PATH": f"{self.shims}{os.pathsep}{os.environ['PATH']}",
            "SHIM_LOG": str(self.shim_log),
            "XDG_DATA_HOME": str(self.home / ".local" / "share"),
            "XDG_CONFIG_HOME": str(self.home / ".config"),
            "XDG_STATE_HOME": str(self.home / ".local" / "state"),
            "TMPDIR": str(self.root),
        }
        if env:
            merged.update(env)
        result = subprocess.run(
            ["bash", str(INSTALLER), "--yes", "--skip-checks", *args],
            env=merged,
            capture_output=True,
            text=True,
            check=False,
        )
        if expect is not None:
            self.assertEqual(result.returncode, expect, result.stdout + result.stderr)
        return result

    def assert_no_download(self) -> None:
        self.assertFalse(self.shim_log.exists(), "源码模式不应调用 curl/unzip")


@unittest.skipUnless(sys.platform == "linux", "install-msfs.sh 只支持 Linux")
class SourceInstallTests(InstallerCase):
    def test_dry_run_reports_packaged_bundle_without_writing(self) -> None:
        _bundle(self.src)
        result = self.run_installer("--source", str(self.src), "--no-build", "--dry-run")
        self.assertIn("使用已打包产物", result.stdout)
        self.assertFalse(self.prefix.exists())
        self.assert_no_download()

    def test_install_from_packaged_bundle(self) -> None:
        _bundle(self.src)
        result = self.run_installer("--source", str(self.src))
        version = self.prefix / "versions" / f"local-{PROTO_VERSION}"
        self.assertEqual((version / "fly-ruler-msfs-bridge.exe").read_text(), "bridge\n")
        self.assertEqual((self.prefix / "current").resolve(), version.resolve())
        self.assertTrue((self.home / ".local" / "bin" / "fly-ruler-msfs").is_symlink())
        self.assertTrue(self.config.is_file())
        self.assertIn(f"local-{PROTO_VERSION}", result.stdout)
        self.assert_no_download()

    def test_no_build_fails_when_bundle_is_missing(self) -> None:
        result = self.run_installer("--source", str(self.src), "--no-build", expect=None)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("just msfs package", result.stdout + result.stderr)
        self.assertFalse(self.prefix.exists())

    def test_builds_with_just_when_bundle_is_missing(self) -> None:
        calls = self.root / "just.log"
        _write(
            self.shims / "just",
            "#!/bin/sh\n"
            "set -eu\n"
            'printf \'%s @ %s\\n\' "$*" "$PWD" >"$JUST_LOG"\n'
            "mkdir -p dist/fly-ruler-msfs/web/dist\n"
            "printf 'built-bridge\\n' >dist/fly-ruler-msfs/fly-ruler-msfs-bridge.exe\n"
            "printf 'built-sim\\n' >dist/fly-ruler-msfs/SimConnect.dll\n"
            "printf '<html>built</html>\\n' >dist/fly-ruler-msfs/web/dist/index.html\n"
            "(cd dist/fly-ruler-msfs && sha256sum fly-ruler-msfs-bridge.exe "
            "SimConnect.dll web/dist/index.html >SHA256SUMS)\n",
            mode=0o755,
        )
        self.run_installer("--source", str(self.src), env={"JUST_LOG": str(calls)})
        installed = self.prefix / "versions" / f"local-{PROTO_VERSION}"
        self.assertEqual((installed / "fly-ruler-msfs-bridge.exe").read_text(), "built-bridge\n")
        self.assertIn(f"msfs package @ {self.src.resolve()}", calls.read_text())
        self.assert_no_download()

    def test_argument_conflicts(self) -> None:
        empty = self.root / "empty"
        empty.mkdir()
        cases = (
            (("--version", "v0.4.0", "--source", str(self.src)), "不能同时使用"),
            (("--no-build",), "只能与 --source"),
            (("--source", str(empty)), "package_msfs_bundle.sh"),
            (("--source", str(self.root / "missing")), "源码目录不存在"),
        )
        for args, needle in cases:
            result = self.run_installer(*args, expect=None)
            self.assertNotEqual(result.returncode, 0, args)
            self.assertIn(needle, result.stdout + result.stderr, args)

    def test_uninstall_keeps_config_and_purge_drops_it(self) -> None:
        _bundle(self.src)
        self.run_installer("--source", str(self.src))
        self.run_installer("--uninstall")
        self.assertFalse((self.prefix / "versions").exists())
        self.assertTrue(self.config.is_file())
        self.run_installer("--uninstall", "--purge")
        self.assertFalse((self.home / ".config" / "fly-ruler-msfs").exists())


if __name__ == "__main__":
    unittest.main()
