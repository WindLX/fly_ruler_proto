#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "Usage: $0 <godot_project_path> [debug|release]" >&2
  exit 1
fi

PROJECT_DIR="$1"
PROFILE="${2:-debug}"
[[ -d "$PROJECT_DIR" ]] || { echo "Godot project path does not exist: $PROJECT_DIR" >&2; exit 1; }
[[ "$PROFILE" == "debug" || "$PROFILE" == "release" ]] || { echo "Profile must be debug or release" >&2; exit 1; }

ROOT_DIR="$(cd "$(dirname "$0")/../../.." && pwd)"
ADDON_DIR="$PROJECT_DIR/addons/fly_ruler_proto"
LIB_BASENAME="libfly_ruler_proto_godot.so"

pushd "$ROOT_DIR" >/dev/null
pnpm --dir web build
export GODOT4_BIN="${GODOT4_BIN:-/usr/bin/godot-mono}"
if [[ "$PROFILE" == "release" ]]; then
  cargo build -p fly_ruler_proto_godot --release
  LIB_SRC="$ROOT_DIR/target/release/$LIB_BASENAME"
else
  cargo build -p fly_ruler_proto_godot
  LIB_SRC="$ROOT_DIR/target/debug/$LIB_BASENAME"
fi
popd >/dev/null

[[ -f "$LIB_SRC" ]] || { echo "Built library not found: $LIB_SRC" >&2; exit 1; }
[[ -f "$ROOT_DIR/web/dist/index.html" ]] || { echo "Web build is missing index.html" >&2; exit 1; }
find "$ROOT_DIR/web/dist/assets" -type f \( -name '*.js' -o -name '*.css' \) -print -quit | grep -q . || {
  echo "Web build is missing hashed JavaScript/CSS assets" >&2
  exit 1
}

rm -rf "$ADDON_DIR"
mkdir -p "$ADDON_DIR/web"
cp "$LIB_SRC" "$ADDON_DIR/$LIB_BASENAME"
cp "$ROOT_DIR/bindings/godot/templates/fly_ruler_proto_godot.gdextension" "$ADDON_DIR/fly_ruler_proto_godot.gdextension"
cp "$ROOT_DIR/bindings/godot/templates/fly_ruler_runtime_example.gd" "$ADDON_DIR/fly_ruler_runtime_example.gd"
cp "$ROOT_DIR/bindings/godot/README.md" "$ADDON_DIR/README.md"
cp -R "$ROOT_DIR/web/dist/." "$ADDON_DIR/web/"

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -1)"
cat > "$ADDON_DIR/manifest.json" <<EOF
{"plugin":"fly_ruler_proto_godot","version":"$VERSION","protocol_version":"$VERSION","platform":"linux.x86_64","profile":"$PROFILE"}
EOF

echo "Installed fly_ruler_proto_godot $VERSION ($PROFILE) to $ADDON_DIR"
