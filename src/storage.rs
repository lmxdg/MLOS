//! 系统配置持久化层
//!
//! 将配置存放在硬盘 LBA 1（第二个扇区，避开 MBR）。
//! 扇区布局（512 字节）：
//!   [0..4)   魔数 "MLOS" = 0x4D4C4F53
//!   [4..8)   版本号
//!   [8..12)  已安装标志
//!   [12..44) 用户名（32 字节）
//!   [44..52) 密码哈希（u64）
//!   [52..56) 语言偏好：0 = English, 1 = 中文预览版
//!   [56..508) 预留
//!   [508..512) 校验和（所有前 508 字节求和）
//! 数据以小端序存放，通过 u16 words 缓冲读写。

use crate::ata;

const CONFIG_LBA: u32 = 1;
const MAGIC: u32 = 0x4D4C4F53; // "MLOS"
const VERSION: u32 = 1;

pub struct Config {
    pub username: [u8; 32],
    pub pass_hash: u64,
    pub installed: bool,
    /// 界面语言偏好：false = English, true = 中文（预览版）
    pub lang_zh: bool,
}

impl Config {
    pub fn empty() -> Self {
        Self {
            username: [0; 32],
            pass_hash: 0,
            installed: false,
            lang_zh: false,
        }
    }

    /// 返回用户名的 &str 视图
    pub fn user_str(&self) -> &str {
        let len = self.username.iter().position(|&b| b == 0).unwrap_or(32);
        core::str::from_utf8(&self.username[..len]).unwrap_or("user")
    }
}

/// 从字节片读取 u32（小端）
fn read_u32(buf: &[u16; 256], word_idx: usize) -> u32 {
    buf[word_idx] as u32 | ((buf[word_idx + 1] as u32) << 16)
}

/// 写 u32 到字节片（小端）
fn write_u32(buf: &mut [u16; 256], word_idx: usize, val: u32) {
    buf[word_idx] = (val & 0xFFFF) as u16;
    buf[word_idx + 1] = (val >> 16) as u16;
}

/// 从字节片读取 u64（小端）
fn read_u64(buf: &[u16; 256], word_idx: usize) -> u64 {
    read_u32(buf, word_idx) as u64 | ((read_u32(buf, word_idx + 2) as u64) << 32)
}

/// 写 u64 到字节片（小端）
fn write_u64(buf: &mut [u16; 256], word_idx: usize, val: u64) {
    write_u32(buf, word_idx, val as u32);
    write_u32(buf, word_idx + 2, (val >> 32) as u32);
}

/// 计算前 508 字节（254 words）的校验和
fn checksum(buf: &[u16; 256]) -> u32 {
    let mut sum: u32 = 0;
    for &w in buf.iter().take(254) {
        sum = sum.wrapping_add(w as u32);
    }
    sum
}

/// 从硬盘读取配置，失败或校验错误返回 None
pub fn load() -> Option<Config> {
    let mut buf = [0u16; 256];
    if !ata::read_sector(CONFIG_LBA, &mut buf) {
        return None;
    }
    if read_u32(&buf, 0) != MAGIC || read_u32(&buf, 2) != VERSION {
        return None;
    }
    // 校验和
    let stored = read_u32(&buf, 254);
    if checksum(&buf) != stored {
        return None;
    }
    let installed = read_u32(&buf, 4) != 0;

    // 用户名：字节偏移 12..44 = words 6..22
    let mut username = [0u8; 32];
    for i in 0..16 {
        let w = buf[6 + i];
        username[i * 2] = (w & 0xFF) as u8;
        username[i * 2 + 1] = (w >> 8) as u8;
    }
    // 密码哈希：字节偏移 44..52 = words 22..26
    let pass_hash = read_u64(&buf, 22);
    // 语言偏好：字节偏移 52..56 = word 26
    let lang_zh = read_u32(&buf, 26) != 0;

    Some(Config {
        username,
        pass_hash,
        installed,
        lang_zh,
    })
}

/// 保存配置到硬盘，返回是否成功
pub fn save(cfg: &Config) -> bool {
    let mut buf = [0u16; 256];
    write_u32(&mut buf, 0, MAGIC);
    write_u32(&mut buf, 2, VERSION);
    write_u32(&mut buf, 4, if cfg.installed { 1 } else { 0 });

    // 用户名：words 6..22
    for i in 0..16 {
        let lo = cfg.username[i * 2] as u16;
        let hi = cfg.username[i * 2 + 1] as u16;
        buf[6 + i] = lo | (hi << 8);
    }
    // 密码哈希：words 22..26
    write_u64(&mut buf, 22, cfg.pass_hash);
    // 语言偏好：word 26
    write_u32(&mut buf, 26, if cfg.lang_zh { 1 } else { 0 });

    // 校验和：word 254
    let sum = checksum(&buf);
    write_u32(&mut buf, 254, sum);

    ata::write_sector(CONFIG_LBA, &buf)
}

/// FNV-1a 64 位哈希（用于密码存储）
pub fn fnv1a(input: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in input {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}
