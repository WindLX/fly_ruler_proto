#!/usr/bin/env python3
"""跨 Rust、Python、Web、锁文件与文档更新 FlyRuler 包版本。

脚本只更新项目自有的版本字段和选定的文档片段，不创建提交、标签或发布产物。

Examples:
    ```bash
    scripts/update_version.py 0.2.4
    scripts/update_version.py v0.2.4 --dry-run
    ```
"""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from pathlib import Path


PROJECT_CRATES = {
    "fly_ruler_proto_core",
    "fly_ruler_proto_godot",
    "fly_ruler_proto_msfs",
    "fly_ruler_proto_python",
    "fly_ruler_proto_server",
}


ROOT = Path(__file__).resolve().parents[1]
SEMVER_RE = re.compile(
    r"^v?(?P<version>0|[1-9]\d*)\."
    r"(0|[1-9]\d*)\."
    r"(0|[1-9]\d*)"
    r"(?:-[0-9A-Za-z.-]+)?"
    r"(?:\+[0-9A-Za-z.-]+)?$"
)


@dataclass
class Edit:
    """一次待写入文件的内容替换。

    Attributes:
        path: 目标文件路径。
        before: 替换前内容。
        after: 替换后内容。
    """

    path: Path
    before: str
    after: str

    @property
    def changed(self) -> bool:
        """返回替换前后内容是否不同。

        Returns:
            内容有变化时为 ``True``。
        """
        return self.before != self.after


def main() -> int:
    """解析命令行参数并执行版本同步。

    Returns:
        进程退出码，成功时为 ``0``。

    Raises:
        SystemExit: 版本号非法或找不到待更新的版本字段时。
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "version", help="New semantic version, with or without leading v"
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="Show files that would change"
    )
    parser.add_argument(
        "--no-locks", action="store_true", help="Do not update Cargo.lock/uv.lock"
    )
    parser.add_argument(
        "--no-docs", action="store_true", help="Do not update README snippets"
    )
    args = parser.parse_args()

    new_version = normalize_version(args.version)
    old_version = read_workspace_version()
    old_protocol_version = read_protocol_version()
    if old_version == new_version:
        print(f"version already set to {new_version}")
        return 0

    edits: list[Edit] = []
    edits.append(update_cargo_toml(new_version))
    edits.append(update_protocol_version(new_version))
    edits.append(update_web_package_json(new_version))
    if not args.no_locks:
        edits.append(update_cargo_lock(new_version))
        edits.append(update_uv_lock(new_version))
    if not args.no_docs:
        edits.extend(update_docs(old_version, old_protocol_version, new_version))

    changed = [edit for edit in edits if edit.changed]
    if args.dry_run:
        if changed:
            print(f"would update version {old_version} -> {new_version}:")
            for edit in changed:
                print(f"  {edit.path.relative_to(ROOT)}")
        else:
            print("no files would change")
        return 0

    for edit in changed:
        edit.path.write_text(edit.after, encoding="utf-8")
    print(f"updated version {old_version} -> {new_version} in {len(changed)} files")
    for edit in changed:
        print(f"  {edit.path.relative_to(ROOT)}")
    print()
    print("recommended follow-up:")
    print("  cargo metadata --format-version 1 >/dev/null")
    print("  cd bindings/python && uv lock")
    print("  cd web && pnpm install --lockfile-only")
    return 0


def normalize_version(raw: str) -> str:
    """规范化用户输入的语义化版本号，去掉可选的前导 ``v``。

    Args:
        raw: 原始版本字符串。

    Returns:
        不含前导 ``v`` 的版本号。

    Raises:
        SystemExit: 输入不符合语义化版本格式。
    """
    value = raw.strip()
    match = SEMVER_RE.match(value)
    if not match:
        raise SystemExit(f"invalid semantic version: {raw!r}")
    return value[1:] if value.startswith("v") else value


def read_workspace_version() -> str:
    """从 ``Cargo.toml`` 读取 workspace 版本。

    Returns:
        当前 workspace 版本号。

    Raises:
        SystemExit: 找不到 ``[workspace.package]`` 版本字段。
    """
    text = read("Cargo.toml")
    match = re.search(r"(?m)^version\s*=\s*\"([^\"]+)\"", text)
    if not match:
        raise SystemExit("failed to find [workspace.package] version in Cargo.toml")
    return match.group(1)


def read_protocol_version() -> str:
    """从 Rust 核心读取 ``PROTOCOL_VERSION``。

    Returns:
        当前协议版本字符串。

    Raises:
        SystemExit: 在 ``core/src/lib.rs`` 中找不到协议版本常量。
    """
    text = read("core/src/lib.rs")
    match = re.search(r'(?m)^pub const PROTOCOL_VERSION: &str = "([^"]+)";$', text)
    if not match:
        raise SystemExit("failed to find PROTOCOL_VERSION in core/src/lib.rs")
    return match.group(1)


def update_cargo_toml(new_version: str) -> Edit:
    """为 ``Cargo.toml`` 的 workspace 版本字段生成替换。

    Args:
        new_version: 新的语义化版本号。

    Returns:
        待写入的替换记录。
    """
    path = ROOT / "Cargo.toml"
    before = path.read_text(encoding="utf-8")
    after = re.sub(
        r"(?m)^version\s*=\s*\"[^\"]+\"",
        f'version = "{new_version}"',
        before,
        count=1,
    )
    return Edit(path, before, after)


def update_cargo_lock(new_version: str) -> Edit:
    """为 ``Cargo.lock`` 中项目自有 crate 的版本字段生成替换。

    Args:
        new_version: 新的语义化版本号。

    Returns:
        待写入的替换记录。
    """
    path = ROOT / "Cargo.lock"
    before = path.read_text(encoding="utf-8")
    package_re = re.compile(r"(?ms)(\[\[package\]\]\n.*?)(?=\n\[\[package\]\]|\Z)")

    def replace_block(match: re.Match[str]) -> str:
        block = match.group(1)
        name_match = re.search(r'(?m)^name = "([^"]+)"$', block)
        if not name_match or name_match.group(1) not in PROJECT_CRATES:
            return block
        return re.sub(
            r'(?m)^version = "[^"]+"$',
            f'version = "{new_version}"',
            block,
            count=1,
        )

    after = package_re.sub(replace_block, before)
    return Edit(path, before, after)


def update_protocol_version(new_version: str) -> Edit:
    """为 Rust 核心的 ``PROTOCOL_VERSION`` 常量生成替换。

    Args:
        new_version: 新的语义化版本号。

    Returns:
        待写入的替换记录。
    """
    path = ROOT / "core/src/lib.rs"
    before = path.read_text(encoding="utf-8")
    after = re.sub(
        r'(?m)^(pub const PROTOCOL_VERSION: &str = ")[^"]+(";)$',
        rf"\g<1>{new_version}\2",
        before,
        count=1,
    )
    return Edit(path, before, after)


def update_uv_lock(new_version: str) -> Edit:
    """为 ``uv.lock`` 中 ``fly-ruler-proto-python`` 的版本字段生成替换。

    Args:
        new_version: 新的语义化版本号。

    Returns:
        待写入的替换记录。
    """
    path = ROOT / "bindings/python/uv.lock"
    before = path.read_text(encoding="utf-8")
    package_re = re.compile(r"(?ms)(\[\[package\]\]\n.*?)(?=\n\[\[package\]\]|\Z)")

    def replace_block(match: re.Match[str]) -> str:
        block = match.group(1)
        if not re.search(r'(?m)^name = "fly-ruler-proto-python"$', block):
            return block
        return re.sub(
            r'(?m)^version = "[^"]+"$',
            f'version = "{new_version}"',
            block,
            count=1,
        )

    after = package_re.sub(replace_block, before)
    return Edit(path, before, after)


def update_web_package_json(new_version: str) -> Edit:
    """为 Web 控制台的 ``package.json`` 版本字段生成替换。

    Args:
        new_version: 新的语义化版本号。

    Returns:
        待写入的替换记录。
    """
    path = ROOT / "web/package.json"
    before = path.read_text(encoding="utf-8")
    payload = json.loads(before)
    payload["version"] = new_version
    after = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"
    return Edit(path, before, after)


def update_docs(
    old_version: str, old_protocol_version: str, new_version: str
) -> list[Edit]:
    """为文档与测试中出现的版本片段生成替换。

    Args:
        old_version: 替换前的包版本号。
        old_protocol_version: 替换前的协议版本号。
        new_version: 新的语义化版本号。

    Returns:
        每个实际存在的目标文件对应一条替换记录。
    """
    paths = [
        ROOT / "README.md",
        ROOT / "core/README.md",
        ROOT / "core/tests/integration_core_flow.rs",
        ROOT / "bindings/python/tests/test_core.py",
        ROOT / "bindings/python/README.md",
        ROOT / "web/README.md",
        ROOT / "server/README.md",
        ROOT / "bindings/msfs/README.md",
        ROOT / "bindings/godot/README.md",
    ]
    edits: list[Edit] = []
    for path in paths:
        if not path.exists():
            continue
        before = path.read_text(encoding="utf-8")
        after = before.replace(f"v{old_version}", f"v{new_version}")
        after = after.replace(
            f'version = "{old_version}"', f'version = "{new_version}"'
        )
        after = after.replace(
            f'"version": "{old_version}"', f'"version": "{new_version}"'
        )
        after = after.replace(
            f'PROTOCOL_VERSION = "{old_protocol_version}"',
            f'PROTOCOL_VERSION = "{new_version}"',
        )
        after = after.replace(
            f'PROTOCOL_VERSION: &str = "{old_protocol_version}"',
            f'PROTOCOL_VERSION: &str = "{new_version}"',
        )
        after = after.replace(f'"{old_protocol_version}"', f'"{new_version}"')
        edits.append(Edit(path, before, after))
    return edits


def read(relative_path: str) -> str:
    """读取仓库根目录下的 UTF-8 文本文件。

    Args:
        relative_path: 相对仓库根的路径。

    Returns:
        文件完整内容。
    """
    return (ROOT / relative_path).read_text(encoding="utf-8")


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BrokenPipeError:
        raise SystemExit(1)
