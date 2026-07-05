#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile="${1:-release}"
destination="${2:-${repo_root}/dist/fly-ruler-msfs}"
target_dir="${repo_root}/target/x86_64-pc-windows-msvc/${profile}"

exe="${target_dir}/fly-ruler-msfs-bridge.exe"
simconnect="${target_dir}/SimConnect.dll"
web_dist="${repo_root}/web/dist"

test -f "${exe}"
test -f "${simconnect}"
test -f "${web_dist}/index.html"
test -n "$(find "${web_dist}/assets" -maxdepth 1 -type f -name '*.js' -print -quit)"
test -n "$(find "${web_dist}/assets" -maxdepth 1 -type f -name '*.css' -print -quit)"

rm -rf "${destination}"
install -d "${destination}/web"
install -m 0755 "${exe}" "${destination}/fly-ruler-msfs-bridge.exe"
install -m 0644 "${simconnect}" "${destination}/SimConnect.dll"
install -m 0644 \
  "${repo_root}/bindings/msfs/fly-ruler-msfs.example.toml" \
  "${destination}/fly-ruler-msfs.example.toml"
install -m 0644 "${repo_root}/bindings/msfs/README.md" "${destination}/README.md"
install -m 0644 "${repo_root}/RELEASING.md" "${destination}/RELEASING.md"
install -m 0644 "${repo_root}/LICENSE" "${destination}/LICENSE"
cp -a "${web_dist}" "${destination}/web/dist"

(
  cd "${destination}"
  find . -type f ! -name SHA256SUMS -print0 \
    | sort -z \
    | xargs -0 sha256sum > SHA256SUMS
)

printf 'MSFS bundle staged at %s\n' "${destination}"
