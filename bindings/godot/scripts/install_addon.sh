#!/usr/bin/env bash
set -euo pipefail

# Install fly_ruler_proto_godot addon assets into a Godot project.
# Usage:
#   ./bindings/godot/scripts/install_addon.sh /path/to/godot/project [debug|release]

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "Usage: $0 <godot_project_path> [debug|release]" >&2
  exit 1
fi

PROJECT_DIR="$1"
PROFILE="${2:-debug}"

if [[ ! -d "$PROJECT_DIR" ]]; then
  echo "Godot project path does not exist: $PROJECT_DIR" >&2
  exit 1
fi

if [[ "$PROFILE" != "debug" && "$PROFILE" != "release" ]]; then
  echo "Profile must be 'debug' or 'release', got: $PROFILE" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "$0")/../../.." && pwd)"
ADDON_DIR="$PROJECT_DIR/addons/fly_ruler_proto"
LIB_BASENAME="libfly_ruler_proto_godot.so"

mkdir -p "$ADDON_DIR"

# Build selected profile
pushd "$ROOT_DIR" >/dev/null
if [[ "$PROFILE" == "release" ]]; then
  cargo build -p fly_ruler_proto_godot --release
  LIB_SRC="$ROOT_DIR/target/release/$LIB_BASENAME"
else
  cargo build -p fly_ruler_proto_godot
  LIB_SRC="$ROOT_DIR/target/debug/$LIB_BASENAME"
fi
popd >/dev/null

if [[ ! -f "$LIB_SRC" ]]; then
  echo "Built library not found: $LIB_SRC" >&2
  exit 1
fi

cp "$LIB_SRC" "$ADDON_DIR/$LIB_BASENAME"
cp "$ROOT_DIR/bindings/godot/templates/fly_ruler_proto_godot.gdextension" \
   "$ADDON_DIR/fly_ruler_proto_godot.gdextension"

# Optional demo script copy (safe overwrite)
cp "$ROOT_DIR/bindings/godot/templates/FlyRulerDemo.gd" \
   "$ADDON_DIR/FlyRulerDemo.gd"

cat <<EOF
Installed fly_ruler_proto addon to:
  $ADDON_DIR

Files:
  - $ADDON_DIR/$LIB_BASENAME
  - $ADDON_DIR/fly_ruler_proto_godot.gdextension
  - $ADDON_DIR/FlyRulerDemo.gd

Next:
  1) Open Godot project
  2) Ensure fly_ruler_proto_godot.gdextension is imported
  3) Attach FlyRulerDemo.gd or create your own Node script using FlyRulerServer
EOF
