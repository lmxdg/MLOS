//! 更新检测模块
//!
//! MLOS 内核运行于 no_std 裸机环境，没有网络协议栈，因此「检测更新」采用
//! 主机桥接方案：
//!   1. 宿主机执行 `tools/check_update.sh`，通过 GitHub Releases API 拉取
//!      仓库 lmxdg/MLOS 的最新版本 tag；
//!   2. 脚本把版本信息写入虚拟硬盘 LBA 3（版本信息扇区）；
//!   3. 内核 `update` 命令读取该扇区，与本地版本常量做语义版本比较并报告。
//!
//! 扇区布局（LBA 3，512 字节）：
//!   [0..4)    魔数 "UPDT" = 0x54445055
//!   [4..8)    版本号长度 N（u32 小端）
//!   [8..8+N)  版本字符串（ASCII，如 "1.0.6"）
//!   [508..512) 校验和（前 508 字节按 u16 求和）

use crate::ata;

/// 本地版本号（与 Cargo.toml 保持一致）
pub const LOCAL_VERSION: &str = "1.0.6";

/// GitHub Releases 页面
pub const RELEASES_URL: &str = "github.com/lmxdg/MLOS/releases";

const UPDATE_LBA: u32 = 3;
const MAGIC: u32 = 0x54445055; // "UPDT" 小端
const MAX_VER_LEN: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UpdateStatus {
    /// 已是最新版本
    UpToDate,
    /// 发现新版本
    NewAvailable,
    /// 未获取到远端版本信息（扇区无效/未运行主机脚本）
    NoInfo,
}

pub struct UpdateInfo {
    pub status: UpdateStatus,
    pub remote: [u8; MAX_VER_LEN],
    pub remote_len: usize,
}

impl UpdateInfo {
    pub fn remote_str(&self) -> &str {
        core::str::from_utf8(&self.remote[..self.remote_len]).unwrap_or("")
    }
}

fn read_u32(buf: &[u16; 256], word: usize) -> u32 {
    buf[word] as u32 | ((buf[word + 1] as u32) << 16)
}

fn checksum(buf: &[u16; 256]) -> u32 {
    let mut sum: u32 = 0;
    for &w in buf.iter().take(254) {
        sum = sum.wrapping_add(w as u32);
    }
    sum
}

/// 把 ASCII 版本串解析为 (major, minor, patch)
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let mut it = s.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// 读取版本扇区并与本地版本比较
pub fn check() -> UpdateInfo {
    let mut info = UpdateInfo {
        status: UpdateStatus::NoInfo,
        remote: [0; MAX_VER_LEN],
        remote_len: 0,
    };

    if !ata::detect() {
        return info;
    }

    let mut buf = [0u16; 256];
    if !ata::read_sector(UPDATE_LBA, &mut buf) {
        return info;
    }
    if read_u32(&buf, 0) != MAGIC {
        return info;
    }
    if checksum(&buf) != read_u32(&buf, 254) {
        return info;
    }

    let n = read_u32(&buf, 2) as usize;
    if n == 0 || n > MAX_VER_LEN {
        return info;
    }

    let mut remote = [0u8; MAX_VER_LEN];
    for i in 0..n {
        let w = buf[4 + i / 2];
        remote[i] = if i % 2 == 0 {
            (w & 0xFF) as u8
        } else {
            (w >> 8) as u8
        };
    }
    info.remote = remote;
    info.remote_len = n;

    let remote_str = info.remote_str();
    match (parse_version(remote_str), parse_version(LOCAL_VERSION)) {
        (Some(r), Some(l)) => {
            info.status = if r > l {
                UpdateStatus::NewAvailable
            } else {
                UpdateStatus::UpToDate
            };
        }
        _ => info.status = UpdateStatus::NoInfo,
    }

    info
}
