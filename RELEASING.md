# FlyRuler Release Guide

GitHub Actions publishes a release when a tag matching `v*.*.*` is pushed. The tag should match the workspace package version in `Cargo.toml`.

```bash
git tag v0.1.0
git push origin v0.1.0
```

## Release outputs

The release workflow builds and uploads:

- `fly-ruler-server-linux-x86_64.tar.gz`
- `fly-ruler-msfs-windows-x86_64.zip`
- Python wheels for Linux x86_64, Linux aarch64, and Windows x86_64
- the packaged Rust core crate

The Web console is built once by the `build-web` job. Its `web/dist` artifact is then consumed by both the Linux server and MSFS jobs, so the two bundles contain the exact same tested frontend.

## MSFS bundle contract

The MSFS archive is self-contained:

```text
fly-ruler-msfs/
├── fly-ruler-msfs-bridge.exe
├── SimConnect.dll
├── fly-ruler-msfs.example.toml
├── README.md
├── RELEASING.md
├── LICENSE
├── SHA256SUMS
└── web/
    └── dist/
        ├── index.html
        └── assets/
            ├── *.js
            └── *.css
```

CI validates every required file and tests the generated zip before uploading it. `SHA256SUMS` covers all files inside the bundle except the checksum file itself.

Build the same layout locally with:

```bash
just package-msfs
```

This writes `dist/fly-ruler-msfs/` and `dist/fly-ruler-msfs-windows-x86_64.zip`. Local packaging requires pnpm, `cargo-xwin`, the Windows MSVC Rust target, the MSFS SDK, and the `zip` command.

Run the bridge from the extracted `fly-ruler-msfs` directory so the default `web/dist` and `sessions` paths resolve inside that directory:

```bash
cd fly-ruler-msfs
protontricks-launch --appid 2537590 ./fly-ruler-msfs-bridge.exe
```

The management console is then available at `http://127.0.0.1:18003/`. Use `--web-root` and `--data-root` when launching from another working directory.

## MSFS SDK cache

The MSFS cross-build requires the private SDK cache `msfs2024-sdk-1.6.9-linux-v1`. Run the `Seed MSFS SDK cache` workflow on the self-hosted runner after installing or updating the SDK. The seed workflow stores only `SimConnect.h`, `SimConnect.lib`, and `SimConnect.dll`.

## Registry publishing

- crates.io publishing skips an already published workspace version.
- PyPI publishing uses Trusted Publishing and `skip-existing`.
- Registry publishing failures do not remove artifacts already built for the
  GitHub Release, but the corresponding job remains visibly failed.
