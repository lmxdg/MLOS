#!/usr/bin/env bash
# MLOS 更新检测 - 主机侧辅助脚本
#
# 作用：MLOS 内核运行于无网络协议栈的裸机环境，无法自行联网。
#       本脚本在宿主机通过 GitHub Releases API 拉取 lmxdg/MLOS 的最新版本，
#       并将其写入虚拟硬盘镜像的 LBA 3 扇区。随后在 MLOS 中执行 `update`
#       即可读取该扇区并与本地版本对比。
#
# 用法：
#   ./tools/check_update.sh [硬盘镜像路径]
#   默认镜像路径为 ./mlos-disk.img
#
# 依赖：curl、python3（仅需标准库）

set -euo pipefail

REPO="lmxdg/MLOS"
DISK="${1:-./mlos-disk.img}"
SECTOR=3  # LBA 3，避开 MBR(LBA0) 与配置扇区(LBA1)

echo "==> MLOS update checker (host side)"
echo "    repository : ${REPO}"
echo "    disk image : ${DISK}"

if [ ! -f "${DISK}" ]; then
    echo "!! error: disk image not found: ${DISK}" >&2
    echo "   run 'make disk' first, then pass the correct path." >&2
    exit 1
fi

# 从 GitHub Releases API 获取最新 tag
echo "==> fetching latest release from GitHub ..."
LATEST="$(curl -fsSL -H 'Accept: application/vnd.github+json' \
    "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
    | python3 -c 'import sys,json
try:
    d=json.load(sys.stdin); print(d.get("tag_name",""))
except Exception:
    pass' || true)"

if [ -z "${LATEST}" ]; then
    echo "!! warning: could not fetch latest release (offline? no release yet?)"
    echo "   falling back to the version in Cargo.toml."
    LATEST="$(grep -m1 '^version' "$(dirname "$0")/../Cargo.toml" \
        | sed -E 's/.*"([^"]+)".*/\1/' | sed 's/^/v/')"
fi

VERSION="${LATEST#v}"
echo "==> latest version: ${VERSION}"

# 构造 512 字节扇区并写入 LBA 3
python3 - "${DISK}" "${SECTOR}" "${VERSION}" <<'PY'
import sys, struct

disk, sector, version = sys.argv[1], int(sys.argv[2]), sys.argv[3]
MAGIC = 0x54445055  # "UPDT" 小端

sector_bytes = bytearray(512)
struct.pack_into('<I', sector_bytes, 0, MAGIC)
vb = version.encode('ascii')[:32]
struct.pack_into('<I', sector_bytes, 4, len(vb))
sector_bytes[8:8+len(vb)] = vb
# 校验和：前 508 字节按 u16 小端求和
words = struct.unpack_from('<254H', sector_bytes, 0)
checksum = sum(words) & 0xFFFFFFFF
struct.pack_into('<I', sector_bytes, 508, checksum)

with open(disk, 'r+b') as f:
    f.seek(sector * 512)
    f.write(sector_bytes)

print(f"==> wrote version '{version}' to LBA {sector} of {disk}")
PY

echo "==> done. Boot MLOS and run 'update' to compare with local version."
