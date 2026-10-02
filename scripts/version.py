#!/usr/bin/env python3
"""统一的 FlyRuler Proto 版本管理脚本。

脚本提供三个子命令：``show`` 只读列出全部版本源、锁文件携带情况与工具版本；
``check`` 校验版本源一致，可附带 ``--expect`` 期望值；``set`` 把新版本写入
Rust、Python、Web、锁文件与选定的文档片段。脚本只改版本字段，不创建提交、
标签或发布产物。

Examples:
    ```bash
    scripts/version.py show
    scripts/version.py check --expect 0.5.0
    scripts/version.py set 0.5.0 --dry-run
    ```
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

PROJECT_CRATES = {
    "fly_ruler_proto_core",
    "fly_ruler_proto_godot",
    "fly_ruler_proto_msfs",
    "fly_ruler_proto_python",
    "fly_ruler_proto_server",
}

CARGO_TOML = "Cargo.toml"
CARGO_LOCK = "Cargo.lock"
PROTOCOL_RS = "core/src/lib.rs"
PACKAGE_JSON = "web/package.json"
PYPROJECT = "bindings/python/pyproject.toml"
UV_LOCK = "bindings/python/uv.lock"
PNPM_LOCK = "web/pnpm-lock.yaml"

ROOT = Path(__file__).resolve().parents[1]
SEMVER_RE = re.compile(
    r"^v?(?P<version>0|[1-9]\d*)\."
    r"(0|[1-9]\d*)\."
    r"(0|[1-9]\d*)"
    r"(?:-[0-9A-Za-z.-]+)?"
    r"(?:\+[0-9A-Za-z.-]+)?$"
)
PACKAGE_BLOCK_RE = re.compile(r"(?ms)(\[\[package\]\]\n.*?)(?=\n\[\[package\]\]|\Z)")

DOC_PATHS = [
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

ACTION_TOOLS: dict[str, tuple[str, str | None]] = {
    "pnpm/action-setup": ("pnpm", "version"),
    "actions/setup-node": ("node", "node-version"),
    "PyO3/maturin-action": ("maturin", None),
    "astral-sh/setup-uv": ("uv", "version"),
    "denoland/setup-deno": ("deno", "deno-version"),
}


@dataclass(frozen=True)
class VersionSource:
    """一个参与一致性校验的版本源。

    Attributes:
        name: 人类可读的来源名称。
        path: 相对仓库根的路径。
        value: 当前读到的版本号。
    """

    name: str
    path: str
    value: str


@dataclass(frozen=True)
class LockReport:
    """锁文件是否携带项目版本的只读结论。

    Attributes:
        path: 相对仓库根的锁文件路径。
        carries: 该锁文件是否记录了项目自身版本。
        detail: 实际读到的字段或缺失原因。
    """

    path: str
    carries: bool
    detail: str


@dataclass(frozen=True)
class ToolPin:
    """只读的工具版本钉。

    Attributes:
        tool: 工具名称。
        value: 该工具当前的版本或动作引用。
        source: 读取到该值的文件。
    """

    tool: str
    value: str
    source: str


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


def main(argv: list[str] | None = None) -> int:
    """解析命令行参数并分发到对应子命令。

    Args:
        argv: 命令行参数列表；``None`` 时使用 ``sys.argv[1:]``。

    Returns:
        进程退出码，成功时为 ``0``。

    Raises:
        SystemExit: 版本号非法或版本字段缺失时。
    """
    args = build_parser().parse_args(argv)
    if args.command == "show":
        return run_show(as_json=args.json)
    if args.command == "check":
        return run_check(expect=args.expect)
    return run_set(
        raw_version=args.version,
        dry_run=args.dry_run,
        no_locks=args.no_locks,
        no_docs=args.no_docs,
    )


def build_parser() -> argparse.ArgumentParser:
    """构造带子命令的命令行解析器。

    Returns:
        已注册 ``show``、``check``、``set`` 三个子命令的解析器。
    """
    parser = argparse.ArgumentParser(
        prog="version.py",
        description="统一的 FlyRuler Proto 版本管理入口。",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    show = subparsers.add_parser("show", help="只读列出全部版本源、锁文件与工具版本")
    show.add_argument("--json", action="store_true", help="以 JSON 输出")

    check = subparsers.add_parser("check", help="校验版本源一致，可附带期望值")
    check.add_argument(
        "--expect", metavar="VERSION", help="额外要求等于该版本，可带前导 v"
    )

    setter = subparsers.add_parser("set", help="把新版本写入全部版本源")
    setter.add_argument("version", help="新的语义化版本号，可带前导 v")
    setter.add_argument("--dry-run", action="store_true", help="只列出会变化的文件")
    setter.add_argument("--no-locks", action="store_true", help="不更新锁文件")
    setter.add_argument("--no-docs", action="store_true", help="不更新文档版本片段")
    return parser


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
    return value.removeprefix("v")


def read_text(relative_path: str) -> str:
    """读取仓库根目录下的 UTF-8 文本文件。

    Args:
        relative_path: 相对仓库根的路径。

    Returns:
        文件完整内容。
    """
    return (ROOT / relative_path).read_text(encoding="utf-8")


def read_workspace_version() -> str:
    """从 ``Cargo.toml`` 读取 workspace 版本。

    Returns:
        当前 workspace 版本号。

    Raises:
        SystemExit: 找不到 ``[workspace.package]`` 版本字段。
    """
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', read_text(CARGO_TOML))
    if not match:
        raise SystemExit(f"failed to find [workspace.package] version in {CARGO_TOML}")
    return match.group(1)


def read_protocol_version() -> str:
    """从 Rust 核心读取 ``PROTOCOL_VERSION``。

    Returns:
        当前协议版本字符串。

    Raises:
        SystemExit: 在 ``core/src/lib.rs`` 中找不到协议版本常量。
    """
    match = re.search(
        r'(?m)^pub const PROTOCOL_VERSION: &str = "([^"]+)";$',
        read_text(PROTOCOL_RS),
    )
    if not match:
        raise SystemExit(f"failed to find PROTOCOL_VERSION in {PROTOCOL_RS}")
    return match.group(1)


def read_web_version() -> str:
    """从 ``web/package.json`` 读取控制台版本。

    Returns:
        当前 Web 包版本号。

    Raises:
        SystemExit: 找不到字符串形式的 ``version`` 字段。
    """
    version = json.loads(read_text(PACKAGE_JSON)).get("version")
    if not isinstance(version, str):
        raise SystemExit(f"failed to find version in {PACKAGE_JSON}")
    return version


def read_cargo_lock_versions() -> dict[str, str]:
    """读取 ``Cargo.lock`` 中项目自有 crate 的版本。

    Returns:
        以 crate 名为键、版本号为值的映射；缺失的项目 crate 不会出现。
    """
    versions: dict[str, str] = {}
    for block in PACKAGE_BLOCK_RE.findall(read_text(CARGO_LOCK)):
        name_match = re.search(r'(?m)^name = "([^"]+)"$', block)
        if not name_match or name_match.group(1) not in PROJECT_CRATES:
            continue
        version_match = re.search(r'(?m)^version = "([^"]+)"$', block)
        if version_match:
            versions[name_match.group(1)] = version_match.group(1)
    return versions


def read_uv_lock_project_version() -> str | None:
    """读取 ``bindings/python/uv.lock`` 中项目包的版本字段。

    实际确认：editable 包块 ``fly-ruler-proto-python`` 只有 ``name`` 与
    ``source``，**不携带** ``version``，因此这里通常返回 ``None``。

    Returns:
        项目包版本号；该包块没有版本字段时为 ``None``。
    """
    for block in PACKAGE_BLOCK_RE.findall(read_text(UV_LOCK)):
        if not re.search(r'(?m)^name = "fly-ruler-proto-python"$', block):
            continue
        match = re.search(r'(?m)^version = "([^"]+)"$', block)
        return match.group(1) if match else None
    return None


def read_pnpm_lock_project_version() -> str | None:
    """读取 ``web/pnpm-lock.yaml`` 根 importer 的版本字段。

    实际确认：lockfile v9 的根 importer（``.``）只记录依赖，**不携带**
    ``version``，因此这里通常返回 ``None``。

    Returns:
        根 importer 的版本号；没有版本字段时为 ``None``。
    """
    text = read_text(PNPM_LOCK)
    importer = re.search(r"(?ms)^importers:\n  \.:\n(?P<body>.*?)(?=^  \S|\Z)", text)
    if not importer:
        return None
    match = re.search(r"(?m)^    version:\s*['\"]?([^'\"\s#]+)", importer.group("body"))
    return match.group(1) if match else None


def pyproject_version_field() -> tuple[bool, bool]:
    """检查 Python 绑定的版本声明方式。

    Returns:
        二元组 ``(是否写死 version, 是否声明 dynamic 版本)``。
    """
    project = tomllib.loads(read_text(PYPROJECT)).get("project")
    if not isinstance(project, dict):
        return False, False
    has_literal = "version" in project
    dynamic = project.get("dynamic")
    has_dynamic = isinstance(dynamic, list) and "version" in dynamic
    return has_literal, has_dynamic


def collect_version_sources() -> list[VersionSource]:
    """收集全部需要保持一致的版本源。

    Returns:
        版本源列表，前三条是三个权威来源，其余是 ``Cargo.lock`` 的项目 crate。
    """
    sources = [
        VersionSource(
            f"{CARGO_TOML} [workspace.package] version",
            CARGO_TOML,
            read_workspace_version(),
        ),
        VersionSource(
            f"{PROTOCOL_RS} PROTOCOL_VERSION",
            PROTOCOL_RS,
            read_protocol_version(),
        ),
        VersionSource(f"{PACKAGE_JSON} version", PACKAGE_JSON, read_web_version()),
    ]
    for name, value in sorted(read_cargo_lock_versions().items()):
        sources.append(VersionSource(f"{CARGO_LOCK} {name}", CARGO_LOCK, value))
    return sources


def describe_locks() -> list[LockReport]:
    """读取三个锁文件，判断它们是否携带项目版本。

    Returns:
        每个锁文件一条只读结论。
    """
    cargo_versions = read_cargo_lock_versions()
    if cargo_versions:
        joined = ", ".join(
            f"{name}={value}" for name, value in sorted(cargo_versions.items())
        )
        cargo_detail = f"{len(cargo_versions)} 个项目 crate：{joined}"
    else:
        cargo_detail = "未找到 PROJECT_CRATES 的版本字段"

    uv_version = read_uv_lock_project_version()
    uv_detail = (
        f"fly-ruler-proto-python={uv_version}"
        if uv_version is not None
        else "editable 包块 fly-ruler-proto-python 无 version 字段"
    )

    pnpm_version = read_pnpm_lock_project_version()
    pnpm_detail = (
        f"根 importer . 的 version={pnpm_version}"
        if pnpm_version is not None
        else "lockfile v9 根 importer（.）只记录依赖，无 version 字段"
    )

    return [
        LockReport(CARGO_LOCK, bool(cargo_versions), cargo_detail),
        LockReport(UV_LOCK, uv_version is not None, uv_detail),
        LockReport(PNPM_LOCK, pnpm_version is not None, pnpm_detail),
    ]


def scan_workflow_tool_pins() -> list[ToolPin]:
    """扫描 ``.github/workflows/`` 中实际出现的工具版本钉。

    只提取 node、pnpm、maturin、uv、deno；仓库里没有对应 workflow 或没有
    对应 pin 时自然略过。

    Returns:
        去重后的工具版本条目，同一工具值的多个 workflow 合并到一个来源。
    """
    workflow_dir = ROOT / ".github/workflows"
    if not workflow_dir.is_dir():
        return []

    collected: dict[tuple[str, str], list[str]] = {}
    paths = [*workflow_dir.glob("*.yml"), *workflow_dir.glob("*.yaml")]
    for path in sorted(paths):
        relative = path.relative_to(ROOT).as_posix()
        lines = path.read_text(encoding="utf-8").splitlines()
        for index, line in enumerate(lines):
            uses = re.search(r"uses:\s*([\w.-]+/[\w.-]+)@([\w./-]+)", line)
            if not uses:
                continue
            entry = ACTION_TOOLS.get(uses.group(1))
            if entry is None:
                continue
            tool, key = entry
            collected.setdefault((f"{tool} action", uses.group(2)), []).append(relative)
            if key is None:
                continue
            for follow in lines[index + 1 : index + 6]:
                if re.match(r"\s*-?\s*uses:", follow):
                    break
                match = re.match(rf"\s*{key}:\s*['\"]?([^'\"\s#]+)", follow)
                if match:
                    collected.setdefault((tool, match.group(1)), []).append(relative)
                    break

    pins = [
        ToolPin(tool, value, ", ".join(dict.fromkeys(files)))
        for (tool, value), files in collected.items()
    ]
    return sorted(pins, key=lambda pin: (pin.tool, pin.value))


def read_tool_versions() -> list[ToolPin]:
    """读取只读的工具版本清单。

    来源包括 ``Cargo.toml`` workspace 的 ``rust-version``、``web/package.json``
    的 ``packageManager`` 与 ``.github/workflows/`` 中实际出现的工具 pin。

    Returns:
        工具版本条目；缺失的工具如实标注为未声明。
    """
    pins: list[ToolPin] = []
    workspace = tomllib.loads(read_text(CARGO_TOML)).get("workspace")
    rust_version: object = None
    if isinstance(workspace, dict):
        package = workspace.get("package")
        if isinstance(package, dict):
            rust_version = package.get("rust-version")
    if isinstance(rust_version, str):
        pins.append(
            ToolPin(
                "rust", rust_version, f"{CARGO_TOML} [workspace.package] rust-version"
            )
        )
    else:
        pins.append(
            ToolPin(
                "rust", "未声明", f"{CARGO_TOML} [workspace.package] 无 rust-version"
            )
        )

    manager = json.loads(read_text(PACKAGE_JSON)).get("packageManager")
    if isinstance(manager, str):
        pins.append(
            ToolPin(
                "pnpm (packageManager)", manager.removeprefix("pnpm@"), PACKAGE_JSON
            )
        )
    pins.extend(scan_workflow_tool_pins())
    return pins


def run_show(as_json: bool) -> int:
    """执行 ``show`` 子命令，只读输出全部版本信息。

    Args:
        as_json: 为 ``True`` 时输出 JSON，否则输出人类可读文本。

    Returns:
        进程退出码，成功时为 ``0``。
    """
    sources = collect_version_sources()
    reference = sources[0].value
    consistent = all(source.value == reference for source in sources)
    literal, dynamic = pyproject_version_field()
    locks = describe_locks()
    tools = read_tool_versions()

    if as_json:
        payload = {
            "consistent": consistent,
            "version": reference if consistent else None,
            "reference": reference,
            "sources": [
                {"name": source.name, "path": source.path, "value": source.value}
                for source in sources
            ],
            "python_binding": {
                "path": PYPROJECT,
                "hardcoded_version": literal,
                "dynamic_version": dynamic,
            },
            "locks": [
                {
                    "path": lock.path,
                    "carries_project_version": lock.carries,
                    "detail": lock.detail,
                }
                for lock in locks
            ],
            "tools": [
                {"tool": pin.tool, "value": pin.value, "source": pin.source}
                for pin in tools
            ],
        }
        print(json.dumps(payload, ensure_ascii=False, indent=2))
        return 0

    width = max(len(source.name) for source in sources)
    print("版本源（只读）:")
    for source in sources:
        suffix = "" if consistent or source.value == reference else "  ← 不一致"
        print(f"  {source.name:<{width}}  {source.value}{suffix}")
    if literal:
        note = "写死 version（错误，应保持 dynamic）"
    elif dynamic:
        note = 'dynamic = ["version"]（不写死）'
    else:
        note = "未声明 dynamic（错误）"
    print(f"  {PYPROJECT:<{width}}  {note}")
    if consistent:
        print(f"一致性：全部一致，当前版本 {reference}")
    else:
        print(f"一致性：不一致，以 {CARGO_TOML} 的 {reference} 为基准")

    print()
    print("锁文件是否携带项目版本（实际读取）:")
    lock_width = max(len(lock.path) for lock in locks)
    for lock in locks:
        marker = "携带" if lock.carries else "不携带"
        print(f"  {lock.path:<{lock_width}}  {marker}：{lock.detail}")

    print()
    print("工具版本（只读，不参与 set）:")
    tool_width = max(len(pin.tool) for pin in tools)
    value_width = max(len(pin.value) for pin in tools)
    for pin in tools:
        print(f"  {pin.tool:<{tool_width}}  {pin.value:<{value_width}}  {pin.source}")

    print()
    print("改版本用 just set-version X.Y.Z；一致性核对用 scripts/version.py check。")
    return 0


def run_check(expect: str | None) -> int:
    """执行 ``check`` 子命令，校验版本源与可选期望值。

    Args:
        expect: 期望版本号，可带前导 ``v``；``None`` 时只校验一致性。

    Returns:
        一致时为 ``0``；不一致时向 stderr 打印差异并返回 ``1``。
    """
    sources = collect_version_sources()
    values = {source.value for source in sources}
    problems: list[str] = []
    if len(values) != 1:
        problems.append("版本源不一致：")
        problems.extend(f"  {source.name}: {source.value}" for source in sources)

    if expect is not None:
        try:
            expected = normalize_version(expect)
        except SystemExit as error:
            print(str(error), file=sys.stderr)
            return 1
        actual = read_workspace_version()
        if actual != expected:
            problems.append(f"期望版本 {expected}，实际 {actual}")

    literal, dynamic = pyproject_version_field()
    if literal:
        problems.append(f'{PYPROJECT} 不应写死版本，请保留 dynamic = ["version"]')
    elif not dynamic:
        problems.append(f'{PYPROJECT} 必须声明 dynamic = ["version"]')

    if problems:
        print("版本校验失败：", file=sys.stderr)
        for problem in problems:
            print(problem, file=sys.stderr)
        return 1

    print(f"版本一致：{read_workspace_version()}")
    return 0


def run_set(
    raw_version: str,
    dry_run: bool,
    no_locks: bool,
    no_docs: bool,
) -> int:
    """执行 ``set`` 子命令，把新版本写入全部版本源。

    Args:
        raw_version: 用户输入的版本号，可带前导 ``v``。
        dry_run: 为 ``True`` 时只列出会变化的文件。
        no_locks: 为 ``True`` 时跳过 ``Cargo.lock`` 与 ``uv.lock``。
        no_docs: 为 ``True`` 时跳过文档与测试中的版本片段。

    Returns:
        进程退出码，成功或版本未变化时为 ``0``。
    """
    new_version = normalize_version(raw_version)
    old_version = read_workspace_version()
    old_protocol_version = read_protocol_version()
    if old_version == new_version:
        print(f"version already set to {new_version}")
        return 0

    edits: list[Edit] = [
        update_cargo_toml(new_version),
        update_protocol_version(new_version),
        update_web_package_json(new_version),
    ]
    if not no_locks:
        edits.append(update_cargo_lock(new_version))
        edits.append(update_uv_lock(new_version))
    if not no_docs:
        edits.extend(update_docs(old_version, old_protocol_version, new_version))

    changed = [edit for edit in edits if edit.changed]
    if dry_run:
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


def update_cargo_toml(new_version: str) -> Edit:
    """为 ``Cargo.toml`` 的 workspace 版本字段生成替换。

    Args:
        new_version: 新的语义化版本号。

    Returns:
        待写入的替换记录。
    """
    path = ROOT / CARGO_TOML
    before = path.read_text(encoding="utf-8")
    after = re.sub(
        r'(?m)^version\s*=\s*"[^"]+"',
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

    path = ROOT / CARGO_LOCK
    before = path.read_text(encoding="utf-8")
    return Edit(path, before, PACKAGE_BLOCK_RE.sub(replace_block, before))


def update_protocol_version(new_version: str) -> Edit:
    """为 Rust 核心的 ``PROTOCOL_VERSION`` 常量生成替换。

    Args:
        new_version: 新的语义化版本号。

    Returns:
        待写入的替换记录。
    """
    path = ROOT / PROTOCOL_RS
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
        待写入的替换记录；该包块没有版本字段时内容保持不变。
    """

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

    path = ROOT / UV_LOCK
    before = path.read_text(encoding="utf-8")
    return Edit(path, before, PACKAGE_BLOCK_RE.sub(replace_block, before))


def update_web_package_json(new_version: str) -> Edit:
    """为 Web 控制台的 ``package.json`` 版本字段生成替换。

    Args:
        new_version: 新的语义化版本号。

    Returns:
        待写入的替换记录。
    """
    path = ROOT / PACKAGE_JSON
    before = path.read_text(encoding="utf-8")
    payload = json.loads(before)
    payload["version"] = new_version
    after = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"
    return Edit(path, before, after)


def version_replacement_pairs(
    old_version: str,
    old_protocol_version: str,
    new_version: str,
) -> list[tuple[str, str]]:
    """构造文档版本片段的旧→新替换对。

    覆盖旧脚本的全部片段形态：``vX.Y.Z``、``version = "X.Y.Z"``、
    ``"version": "X.Y.Z"``、``PROTOCOL_VERSION`` 两种写法，以及带引号的裸版本。
    旧串按长度降序排列，保证组合正则先匹配更具体的片段。

    Args:
        old_version: 替换前的包版本号。
        old_protocol_version: 替换前的协议版本号。
        new_version: 新的语义化版本号。

    Returns:
        去重且按旧串降序排列的 ``(旧片段, 新片段)`` 列表。
    """
    candidates = [
        (f"v{old_version}", f"v{new_version}"),
        (f'version = "{old_version}"', f'version = "{new_version}"'),
        (f'"version": "{old_version}"', f'"version": "{new_version}"'),
        (
            f'PROTOCOL_VERSION = "{old_protocol_version}"',
            f'PROTOCOL_VERSION = "{new_version}"',
        ),
        (
            f'PROTOCOL_VERSION: &str = "{old_protocol_version}"',
            f'PROTOCOL_VERSION: &str = "{new_version}"',
        ),
        (f'"{old_version}"', f'"{new_version}"'),
        (f'"{old_protocol_version}"', f'"{new_version}"'),
    ]
    pairs: dict[str, str] = {}
    for old, new in candidates:
        if old != new:
            pairs.setdefault(old, new)
    return sorted(pairs.items(), key=lambda item: len(item[0]), reverse=True)


def update_docs(
    old_version: str,
    old_protocol_version: str,
    new_version: str,
) -> list[Edit]:
    """为文档与测试中出现的版本片段生成替换。

    对每个文件只在原始内容上做一次正则替换，旧包版本与旧协议版本一起映射到
    新版本，不会因为链式 ``str.replace`` 造成重复替换。

    Args:
        old_version: 替换前的包版本号。
        old_protocol_version: 替换前的协议版本号。
        new_version: 新的语义化版本号。

    Returns:
        每个实际存在的目标文件对应一条替换记录。
    """
    pairs = version_replacement_pairs(old_version, old_protocol_version, new_version)
    if not pairs:
        return []
    pattern = re.compile("|".join(re.escape(old) for old, _ in pairs))
    mapping = dict(pairs)
    edits: list[Edit] = []
    for path in DOC_PATHS:
        if not path.exists():
            continue
        before = path.read_text(encoding="utf-8")
        after = pattern.sub(lambda match: mapping[match.group(0)], before)
        edits.append(Edit(path, before, after))
    return edits


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BrokenPipeError:
        raise SystemExit(1)
