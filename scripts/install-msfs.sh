#!/usr/bin/env bash
#
# FlyRuler MSFS bridge installer (Linux + Proton, user space only).
#
# The bridge is a Windows binary that talks to MSFS 2024 through SimConnect.
# On Linux it has to run inside the MSFS Proton prefix, so this script only
# unpacks the released bundle into the user's home, writes a launcher and an
# optional systemd user unit, and never touches system directories.
#
# The bridge is NOT a service that runs forever: start MSFS 2024 first, then run
# the launcher, and stop it with Ctrl-C when you are done.
#
# Usage:
#   curl -fsSL https://github.com/WindLX/fly_ruler_proto/releases/latest/download/install-msfs.sh | bash
#   ./install-msfs.sh --version v0.4.0 --with-service
#   ./install-msfs.sh --uninstall [--purge]
#
set -euo pipefail

REPO="WindLX/fly_ruler_proto"
RELEASE_ASSET="fly-ruler-msfs-windows-x86_64.zip"
DEFAULT_APPID="2537590" # MSFS 2024 Steam AppID
MIN_FREE_KB=$((200 * 1024))

XDG_DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
XDG_STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"

PREFIX="$XDG_DATA_HOME/fly-ruler-msfs"
CONFIG_DIR="$XDG_CONFIG_HOME/fly-ruler-msfs"
CONFIG_FILE="$CONFIG_DIR/fly-ruler-msfs.toml"
ENV_FILE="$CONFIG_DIR/env"
STATE_DIR="$XDG_STATE_HOME/fly-ruler-msfs"
BIN_DIR="$HOME/.local/bin"
LAUNCHER="$BIN_DIR/fly-ruler-msfs"
UNIT_DIR="$XDG_CONFIG_HOME/systemd/user"
UNIT_FILE="$UNIT_DIR/fly-ruler-msfs.service"

VERSION=""
APPID="$DEFAULT_APPID"
WITH_SERVICE=false
DO_UNINSTALL=false
PURGE=false
DRY_RUN=false
ASSUME_YES=false
SKIP_CHECKS=false
STRICT=false
WARNINGS=0

usage() {
  cat <<'EOF'
用法：install-msfs.sh [选项]

安装（默认）：把 GitHub Release 里的 MSFS 桥接装到用户空间。
  --version vX.Y.Z   安装指定版本（默认取最新 Release）
  --prefix <目录>    程序安装目录（默认 ~/.local/share/fly-ruler-msfs）
  --appid <id>       MSFS 的 Steam AppID（默认 2537590）
  --with-service     额外写一个 systemd user unit（不 enable、不启动）
  --skip-checks      跳过环境检查（protontricks / MSFS / 端口占用等）
  --strict           把警告当作错误
  --dry-run          只解析版本并打印将要执行的动作
  --yes              跳过确认提示（非交互，管道执行时自动生效）

卸载：
  --uninstall        卸载程序、命令与 unit；配置、日志与会话数据保留
  --purge            与 --uninstall 同用，额外删除配置、日志与会话数据

其它：
  -h, --help         显示本帮助

桥接不是常驻服务：先启动 MSFS 2024 并进入 Free Flight，再运行 fly-ruler-msfs。
EOF
}

info() { printf '[install-msfs] %s\n' "$*"; }
warn() {
  printf '[install-msfs] 警告：%s\n' "$*" >&2
  WARNINGS=$((WARNINGS + 1))
}
die() {
  printf '[install-msfs] 错误：%s\n' "$*" >&2
  exit 1
}
note() { printf '  %s\n' "$*"; }

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
    --version)
      [ $# -ge 2 ] || die "--version 需要一个参数"
      VERSION="$2"
      shift 2
      ;;
    --prefix)
      [ $# -ge 2 ] || die "--prefix 需要一个参数"
      PREFIX="$2"
      shift 2
      ;;
    --appid)
      [ $# -ge 2 ] || die "--appid 需要一个参数"
      APPID="$2"
      shift 2
      ;;
    --with-service)
      WITH_SERVICE=true
      shift
      ;;
    --uninstall)
      DO_UNINSTALL=true
      shift
      ;;
    --purge)
      PURGE=true
      shift
      ;;
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    --yes)
      ASSUME_YES=true
      shift
      ;;
    --skip-checks)
      SKIP_CHECKS=true
      shift
      ;;
    --strict)
      STRICT=true
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      die "未知参数 $1"
      ;;
    esac
  done

  case "$PREFIX" in
  /*) ;;
  *) die "--prefix 必须是绝对路径" ;;
  esac
  if $PURGE && ! $DO_UNINSTALL; then
    die "--purge 只能与 --uninstall 一起用"
  fi
}

require_tools() {
  local missing=""
  for tool in curl unzip sha256sum; do
    command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
  done
  if [ -n "$missing" ]; then
    die "缺少必需命令：$missing（安装这些命令后再试）"
  fi
}

check_platform() {
  [ "$(uname -s)" = "Linux" ] || die "只支持 Linux + Proton；Windows 请直接解压 Release 里的 zip"
  case "$(uname -m)" in
  x86_64) ;;
  *) warn "本机架构是 $(uname -m)，Release 里的桥接是 x86_64 Windows 二进制，需要 Proton 能跑 x86_64" ;;
  esac
}

port_in_use() {
  local port="$1"
  if command -v ss >/dev/null 2>&1; then
    ss -H -lnut 2>/dev/null | awk '{print $5}' | grep -qE "[:.]${port}\$"
    return
  fi
  (exec 3<>"/dev/tcp/127.0.0.1/${port}") 2>/dev/null
}

find_msfs_dir() {
  local candidate
  for candidate in \
    "$HOME/.steam/steam/steamapps/common/MSFS2024" \
    "$HOME/.local/share/Steam/steamapps/common/MSFS2024"; do
    if [ -d "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

check_environment() {
  if ! command -v protontricks-launch >/dev/null 2>&1; then
    warn "找不到 protontricks-launch，桥接无法在 Proton 前缀里启动（安装 protontricks 后再试）"
  fi
  command -v steam >/dev/null 2>&1 || warn "找不到 steam 命令，MSFS 与 Proton 前缀都需要 Steam"
  if ! $SKIP_CHECKS; then
    local msfs_dir
    if msfs_dir="$(find_msfs_dir)"; then
      info "检测到 MSFS：$msfs_dir"
    else
      warn "没有找到 MSFS 2024 安装目录（steamapps/common/MSFS2024）；如果装在别处，用 --skip-checks 跳过这项"
    fi
    if port_in_use 18002; then
      warn "UDP 端口 18002 已被占用（可能已经有一个桥接或独立服务端在跑）"
    fi
    if port_in_use 18003; then
      warn "HTTP 端口 18003 已被占用（可能已经有一个桥接或独立服务端在跑）"
    fi
  fi
  case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) warn "$BIN_DIR 不在 PATH 里，装完请把 export PATH=\"$BIN_DIR:\$PATH\" 加进 shell 配置" ;;
  esac
  if $WITH_SERVICE; then
    if ! command -v systemctl >/dev/null 2>&1 || [ ! -d "/run/user/$(id -u)" ]; then
      warn "当前没有可用的 systemd user session，unit 会写出来但需要你自己确认怎么启动"
    fi
  fi
  local free_kb
  # df 在 HOME 不存在或无法 stat 时会失败；磁盘检查只是提醒，不能因此中断安装。
  free_kb="$(df -Pk "$HOME" 2>/dev/null | awk 'NR==2 {print $4}')" || free_kb=""
  if [ -n "$free_kb" ] && [ "$free_kb" -lt "$MIN_FREE_KB" ]; then
    warn "home 分区剩余空间不足 200 MB，解包可能失败"
  fi
}

confirm_plan() {
  local tag="$1"
  info "将安装 FlyRuler MSFS 桥接 $tag 到 $PREFIX"
  note "命令：$LAUNCHER"
  note "配置：$CONFIG_FILE（已存在则保留）"
  note "工作目录：$STATE_DIR"
  $WITH_SERVICE && note "systemd user unit：$UNIT_FILE（只写文件，不 enable、不启动）"
  if [ -t 0 ] && ! $ASSUME_YES; then
    printf '[install-msfs] 回车继续（或 Ctrl-C 取消）：'
    read -r _
  fi
}

resolve_version() {
  local tag="$1"
  if [ -n "$tag" ]; then
    case "$tag" in
    v*) ;;
    *) tag="v$tag" ;;
    esac
    printf '%s\n' "$tag"
    return 0
  fi
  local json
  json="$(curl -fsSL --retry 3 "https://api.github.com/repos/${REPO}/releases/latest")" ||
    die "无法访问 GitHub API 获取最新版本；用 --version vX.Y.Z 指定版本"
  tag="$(printf '%s' "$json" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  [ -n "$tag" ] || die "无法从 GitHub API 响应里解析 tag_name"
  printf '%s\n' "$tag"
}

launcher_body() {
  local protontricks
  protontricks="$(command -v protontricks-launch || true)"
  cat <<EOF
#!/usr/bin/env bash
# Generated by install-msfs.sh — FlyRuler MSFS bridge launcher.
# 桥接不是常驻服务：先启动 MSFS 2024 并进入 Free Flight，再运行本命令。
set -euo pipefail

export PATH="\$HOME/.local/bin:\$PATH"

root="\${FLY_RULER_MSFS_ROOT:-$PREFIX}"
config="\${FLY_RULER_MSFS_CONFIG:-$CONFIG_FILE}"
appid="\${FLY_RULER_MSFS_APPID:-$APPID}"
state="\${XDG_STATE_HOME:-\$HOME/.local/state}/fly-ruler-msfs"
exe="\$root/current/fly-ruler-msfs-bridge.exe"
protontricks="${protontricks:-protontricks-launch}"

if [ ! -x "\$exe" ]; then
  printf '%s\n' "找不到桥接程序：\$exe（先运行 install-msfs.sh 安装）" >&2
  exit 1
fi
if ! command -v "\$protontricks" >/dev/null 2>&1; then
  printf '%s\n' "找不到 protontricks-launch（安装 protontricks 后再试）" >&2
  exit 1
fi

mkdir -p "\$state"
cd "\$state"
exec "\$protontricks" --appid "\$appid" "\$exe" --config "\$config" "\$@"
EOF
}

write_user_config() {
  install -d -m 0755 "$CONFIG_DIR" "$PREFIX/sessions"
  if [ -f "$CONFIG_FILE" ]; then
    info "保留现有配置：$CONFIG_FILE"
    return 0
  fi
  cat >"$CONFIG_FILE" <<EOF
# Generated by install-msfs.sh for FlyRuler MSFS bridge $VERSION（$APPID）.
# 这份文件属于你：重装不会覆盖它，改完直接生效。
# 所有路径都是绝对路径，因此从任何工作目录启动都一样。

[bridge]
listen = "127.0.0.1:18002"
tick_hz = 240.0
render_hz = 240.0
smoothing_mode = "low_latency"
interpolation_delay_ms = 20
max_extrapolation_ms = 20
stale_timeout_ms = 500
enable_ai_aircraft = false
ai_aircraft_title = "Rafale M"
max_ai_aircraft = 8

[management]
enabled = true
listen = "127.0.0.1:18003"
data_root = "$PREFIX/sessions"
web_root = "$PREFIX/current/web/dist"
ws_hz = 30.0
# 控制台由桥自己托管，浏览器与 API 同源，不需要额外的跨源来源；
# 默认白名单已经覆盖本机的常用开发端口。真要收紧或换成别处托管的前端时，
# 才在这里写 cors_origins = [...]，注意写了是整体替换默认值，不是追加。

[logging]
level = "info"
# file_path = "$STATE_DIR/bridge.log"
EOF
  chmod 0644 "$CONFIG_FILE"
  info "写入配置：$CONFIG_FILE"
}

write_launcher() {
  install -d -m 0755 "$PREFIX/bin"
  if [ -e "$LAUNCHER" ] && [ ! -L "$LAUNCHER" ]; then
    die "$LAUNCHER 已经存在且不是本脚本创建的软链，先移走它"
  fi
  launcher_body >"$PREFIX/bin/fly-ruler-msfs"
  chmod 0755 "$PREFIX/bin/fly-ruler-msfs"
  install -d -m 0755 "$BIN_DIR"
  ln -sfn "$PREFIX/bin/fly-ruler-msfs" "$LAUNCHER"
  info "写入命令：$LAUNCHER"
}

write_service_unit() {
  local graphical_env
  install -d -m 0755 "$UNIT_DIR"
  cat >"$UNIT_FILE" <<EOF
[Unit]
Description=FlyRuler MSFS bridge (start on demand, not a persistent service)
Documentation=https://github.com/$REPO
After=graphical-session.target

[Service]
Type=simple
ExecStart=$PREFIX/bin/fly-ruler-msfs
Restart=no
EnvironmentFile=-$ENV_FILE

# 故意不提供 [Install] 段：桥接不是常驻服务，systemd --user enable 会直接失败。
# 需要时手动 systemctl --user start fly-ruler-msfs，用完 Ctrl-C 或 stop。
EOF
  chmod 0644 "$UNIT_FILE"
  info "写入 systemd user unit：$UNIT_FILE（未 enable、未启动）"

  # The user manager usually inherits the graphical variables from the login
  # session. Only write a fallback env file when it does not.
  graphical_env="$(systemctl --user show-environment 2>/dev/null || true)"
  if printf '%s\n' "$graphical_env" | grep -q '^DISPLAY=' ||
    printf '%s\n' "$graphical_env" | grep -q '^WAYLAND_DISPLAY='; then
    info "systemd user 环境里已有图形变量，不写 env 文件"
  else
    {
      [ -n "${DISPLAY:-}" ] && printf 'DISPLAY=%s\n' "$DISPLAY"
      [ -n "${WAYLAND_DISPLAY:-}" ] && printf 'WAYLAND_DISPLAY=%s\n' "$WAYLAND_DISPLAY"
      [ -n "${XAUTHORITY:-}" ] && printf 'XAUTHORITY=%s\n' "$XAUTHORITY"
    } >"$ENV_FILE"
    chmod 0600 "$ENV_FILE"
    warn "systemd user 环境里没有图形变量，已写入 $ENV_FILE 作为兜底"
  fi

  if command -v systemctl >/dev/null 2>&1 && [ -d "/run/user/$(id -u)" ]; then
    systemctl --user daemon-reload || warn "systemctl --user daemon-reload 失败，unit 可能需要手动 reload"
  fi
}

fetch_bundle() {
  local tag="$1" tmp_dir="$2"
  local url="https://github.com/${REPO}/releases/download/${tag}/${RELEASE_ASSET}"
  info "下载 $url"
  curl -fL --retry 3 --proto '=https' -o "$tmp_dir/bundle.zip" "$url" ||
    die "下载失败：$url（确认这个 tag 有 $RELEASE_ASSET 资产）"
  unzip -q "$tmp_dir/bundle.zip" -d "$tmp_dir/unpack" || die "解压 zip 失败"
}

verify_bundle() {
  local unpack="$1"
  local bundle_root
  bundle_root="$(find "$unpack" -maxdepth 2 -type f -name SHA256SUMS -printf '%h\n' | head -n 1)"
  [ -n "$bundle_root" ] || die "zip 里没有 SHA256SUMS，产物不完整"
  (cd "$bundle_root" && sha256sum -c SHA256SUMS --quiet) || die "SHA256SUMS 校验失败，产物可能损坏"
  local required
  for required in fly-ruler-msfs-bridge.exe SimConnect.dll web/dist/index.html; do
    [ -e "$bundle_root/$required" ] || die "zip 里缺少 $required"
  done
  printf '%s\n' "$bundle_root"
}

install_bridge() {
  local bundle_root="$1" tag="$2"
  local versions="$PREFIX/versions"
  local stage="$versions/.staging.$tag.$$"

  install -d -m 0755 "$PREFIX" "$versions" "$PREFIX/sessions" "$STATE_DIR"
  rm -rf "$stage"
  install -d -m 0755 "$stage"
  cp -a "$bundle_root/." "$stage/"
  [ -x "$stage/fly-ruler-msfs-bridge.exe" ] || chmod 0755 "$stage/fly-ruler-msfs-bridge.exe"
  rm -rf "$versions/$tag"
  mv "$stage" "$versions/$tag"
  ln -sfn "versions/$tag" "$PREFIX/.current.$$"
  mv -T "$PREFIX/.current.$$" "$PREFIX/current"
  info "安装程序：$versions/$tag（current 指向它）"

  write_user_config
  write_launcher
  $WITH_SERVICE && write_service_unit
  return 0
}

uninstall() {
  if [ -f "$UNIT_FILE" ]; then
    if command -v systemctl >/dev/null 2>&1 && [ -d "/run/user/$(id -u)" ]; then
      systemctl --user disable --now fly-ruler-msfs.service >/dev/null 2>&1 || true
      systemctl --user daemon-reload >/dev/null 2>&1 || true
    fi
    rm -f "$UNIT_FILE"
    info "已移除 unit：$UNIT_FILE"
  fi
  if [ -L "$LAUNCHER" ] && [ "$(readlink -f "$LAUNCHER" 2>/dev/null || true)" = "$PREFIX/bin/fly-ruler-msfs" ]; then
    rm -f "$LAUNCHER"
    info "已移除命令：$LAUNCHER"
  elif [ -e "$LAUNCHER" ]; then
    warn "$LAUNCHER 不是本脚本创建的，保留不动"
  fi
  rm -rf "$PREFIX/versions" "$PREFIX/current" "$PREFIX/bin"
  info "已移除程序目录：$PREFIX/versions 与 $PREFIX/current"

  if $PURGE; then
    rm -rf "$PREFIX/sessions" "$CONFIG_DIR" "$STATE_DIR"
    rmdir "$PREFIX" 2>/dev/null || true
    info "已删除配置、日志与会话数据（--purge）"
  else
    rmdir "$PREFIX" 2>/dev/null || true
    info "保留配置：$CONFIG_FILE"
    info "保留会话数据：$PREFIX/sessions"
    info "保留工作目录：$STATE_DIR"
    printf '[install-msfs] 需要连数据一起删就再跑一次 --uninstall --purge\n'
  fi
}

print_next_steps() {
  cat <<EOF

[install-msfs] 安装完成：FlyRuler MSFS 桥接 $VERSION

  程序    $PREFIX/versions/$VERSION（current 软链）
  命令    $LAUNCHER
  配置    $CONFIG_FILE
  工作目录 $STATE_DIR
  会话数据 $PREFIX/sessions
  控制台  http://127.0.0.1:18003

桥接不是常驻服务，不会开机自启、也不进后台。要用的时候：
  1. 启动 MSFS 2024 并进入 Free Flight（关闭 Active Pause）
  2. 运行 fly-ruler-msfs（或 systemctl --user start fly-ruler-msfs）
  3. 浏览器打开 http://127.0.0.1:18003
  4. 收工：在运行它的终端按 Ctrl-C
EOF
}

main() {
  parse_args "$@"
  [ "$(id -u)" -ne 0 ] || die "不要用 root 运行：这个脚本只装到当前用户的空间里"

  if $DO_UNINSTALL; then
    if $DRY_RUN; then
      info "dry-run：将卸载 $PREFIX$($PURGE && printf '，并删除配置与会话数据' || printf '，保留配置与会话数据')"
      exit 0
    fi
    uninstall
    return 0
  fi

  require_tools
  check_platform
  check_environment
  if $STRICT && [ "$WARNINGS" -gt 0 ]; then
    die "--strict：上面有 $WARNINGS 条警告，中止安装"
  fi

  VERSION="$(resolve_version "$VERSION")"
  info "目标版本：$VERSION"
  confirm_plan "$VERSION"

  if $DRY_RUN; then
    note "dry-run：将下载 https://github.com/${REPO}/releases/download/${VERSION}/${RELEASE_ASSET}"
    note "dry-run：校验 SHA256SUMS 后安装到 $PREFIX/versions/$VERSION"
    note "dry-run：写命令 $LAUNCHER、配置 $CONFIG_FILE（不存在时）"
    $WITH_SERVICE && note "dry-run：写 unit $UNIT_FILE（不 enable、不启动）"
    note "dry-run：以上都未执行"
    return 0
  fi

  local tmp_dir
  tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/fly-ruler-msfs.XXXXXX")"
  # Expand tmp_dir now: the EXIT trap runs after main's locals are gone.
  # shellcheck disable=SC2064
  trap "rm -rf '$tmp_dir'" EXIT

  fetch_bundle "$VERSION" "$tmp_dir"
  local bundle_root
  bundle_root="$(verify_bundle "$tmp_dir/unpack")"
  install_bridge "$bundle_root" "$VERSION"
  print_next_steps
}

main "$@"
