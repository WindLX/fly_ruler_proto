# 发布说明

本仓库用 GitHub Actions 发布：推送 `v*.*.*` 形式的标签即触发 `.github/workflows/release.yml`，构建全部产物并创建 GitHub Release。日常提交由 `.github/workflows/ci.yml` 在 `main` 与 PR 上验证。

## 一、改版本号

版本号只有一处入口：

```bash
just set-version 0.5.0
```

`scripts/update_version.py` 会同步 `Cargo.toml` 的 workspace 版本、`core/src/lib.rs` 的 `PROTOCOL_VERSION`、`web/package.json`、`Cargo.lock`、`bindings/python/uv.lock` 与文档中的版本引用。Python 包的版本是动态的，由 maturin 从 `Cargo.toml` 读取，`bindings/python/pyproject.toml` 里不写死版本号。

改完先跑 `just _check-version` 确认三处版本一致，再提交。

## 二、本地门禁

发布前在本机跑完整核对：

```bash
just check-release
```

它依次执行 `just check`、`just test`、`just msfs check` 与 `just msfs package`。没有 Windows 交叉工具链或 MSFS SDK 时，至少保证 `just pre-commit` 通过，其余步骤由 CI 覆盖。MSFS 包的内容与校验逻辑见 `scripts/package_msfs_bundle.sh`。

## 三、打标签

```bash
git tag -a v0.5.0 -m "v0.5.0"
git push origin v0.5.0
```

标签必须与 `Cargo.toml` 中的 workspace 版本一致，`release.yml` 用标签名去掉前缀后的字符串去查询各注册表。

## 四、一次性配置

- PyPI：仓库需要名为 `pypi` 的 Environment，并在 PyPI 侧为该 Environment 配置 Trusted Publishing（OIDC），发布 job 会声明 `id-token: write`，不使用长期令牌。
- crates.io：仓库需要 `CARGO_REGISTRY_TOKEN` secret，只有该 secret 存在时 `cargo publish` 才能成功。
- MSFS SDK：交叉编译需要缓存 `msfs2024-sdk-1.6.9-linux-v1`。在安装了 MSFS 2024 SDK 的自托管 runner 上手动运行 `Seed MSFS SDK cache` 工作流（`.github/workflows/seed-msfs-sdk.yml`）来填充缓存；缓存未命中时 `build-msfs` 会直接失败并提示缓存键名。

## 五、产物清单

`release.yml` 的 job 依赖关系是：`build-web` 先产出 `web/dist`，`build-server` 与 `build-msfs` 各自把它打进自己的包；`package-core`、`build-wheels-linux`、`build-wheel-windows` 独立并行；全部构建完成后 `github-release` 汇总产物。

| 产物 | 内容 |
| --- | --- |
| `fly-ruler-server-linux-x86_64.tar.gz` | `fly-ruler-server/` 目录，含服务进程、`fly-ruler-server.example.toml`、`web/dist/`、`README.md`、`LICENSE`、`RELEASING.md` |
| `fly-ruler-msfs-windows-x86_64.zip` | `fly-ruler-msfs/` 目录，含 `fly-ruler-msfs-bridge.exe`、`SimConnect.dll`、`fly-ruler-msfs.example.toml`、`README.md`、`RELEASING.md`、`LICENSE`、`SHA256SUMS`、`web/dist/` |
| `install-msfs.sh` | 面向用户的 MSFS 桥安装/卸载脚本，文档里的地址是 `/releases/latest/download/install-msfs.sh`，所以每个 Release 都必须带上它 |
| Python wheel | Linux x86_64、Linux aarch64、Windows x86_64 三个 `fly_ruler_proto_python` wheel |
| `fly_ruler_proto_core-*.crate` | 内核 crate 源码包 |

工作流在打包后立即校验上述文件清单，缺文件会让发布失败，不会产出半成品 Release。MSFS 包里的 `SHA256SUMS` 覆盖除自身以外的全部文件。

## 六、注册表发布的幂等语义

两个注册表都会先查询版本是否已存在：

- crates.io：请求 `https://crates.io/api/v1/crates/fly_ruler_proto_core/<版本>`，返回 200 表示已发布，跳过；返回 404 才执行 `cargo publish`。
- PyPI：请求 `https://pypi.org/pypi/fly-ruler-proto-python/<版本>/json`，返回 200 表示已发布，跳过上传。

因此重复推送同一个标签不会报错，也不会覆盖已发布版本。注册表发布失败不会删除已经构建好的 GitHub Release 产物，但对应的 job 会保持失败状态，便于发现令牌或网络问题。

## 七、发布后核对

在 GitHub Release 页确认四类产物都在，并逐个校验：解压服务包后能直接运行 `./fly-ruler-server`，解压 MSFS 包后目录结构与上表一致，PyPI 与 crates.io 的版本页能看到新版本，控制台包内含与发布标签匹配的前端资源；`install-msfs.sh` 用 `curl -fsSL .../releases/latest/download/install-msfs.sh | bash -s -- --dry-run` 能跑通。
