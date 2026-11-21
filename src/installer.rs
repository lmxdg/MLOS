//! 将 MLOS 完整写入硬盘，实现免 ISO 启动。
//!
//! 磁盘布局：
//! | LBA        | 内容                                                |
//! |------------|-----------------------------------------------------|
//! | 0          | MBR（自研 stage 1）                                 |
//! | 1          | 用户配置（storage.rs）                              |
//! | 3          | 更新信息（update.rs）                               |
//! | 4+         | MiniFS 文件区                                       |
//! | 2048..2111 | Stage 2（64 个扇区 = 32 KiB，含 VBE 初始化）        |
//! | 4096       | 内核头 "MLBX"（扇区数 + 入口偏移）                  |
//! | 4097+      | 内核扁平镜像（仅加载段，BSS 由内核清零）            |
//!
//! 镜像只复制 `__kernel_start..__kernel_data_end`，不包含 BSS；因此
//! 内核镜像很小，启动阶段会自行清零 BSS 并重建页表。

use crate::ata;
use alloc::{format, string::ToString};

const MBR_LBA: u32 = 0;
const STAGE2_LBA: u32 = 2048;
const STAGE2_SECTORS: u32 = 64;
const HDR_LBA: u32 = 4096;
const IMG_LBA: u32 = 4097;
const MAX_IMAGE_SECTORS: u32 = 1120;

const SPIN: [u8; 4] = [b'/', b'-', b'\\', b'|'];
static mut SPIN_INDEX: u8 = 0;

extern "C" {
    static __kernel_start: u8;
    static __kernel_data_end: u8;
    static _start: u8;
}

mod bootbins {
    include!(concat!(env!("OUT_DIR"), "/bootbins.rs"));
}

pub fn install() -> Result<u32, &'static str> {
    if bootbins::MBR.len() != 512 {
        return Err("MBR binary missing or corrupt");
    }
    if bootbins::STAGE2.len() != (STAGE2_SECTORS as usize) * 512 {
        return Err("stage 2 binary is not 32 KiB");
    }
    if !ata::detect() {
        return Err("no hard disk");
    }
    let capacity = ata::capacity_sectors();
    if capacity > 0 && capacity < crate::hw::MIN_DISK_SECTORS {
        return Err("disk too small (need >= 16 MiB)");
    }

    let img_base = unsafe { &__kernel_start as *const u8 };
    let img_end = unsafe { &__kernel_data_end as *const u8 };
    let img_len = img_end as usize - img_base as usize;
    if img_len == 0 {
        return Err("kernel image is empty");
    }
    let img_sectors = ((img_len + 511) / 512) as u32;
    if img_sectors == 0 || img_sectors > MAX_IMAGE_SECTORS {
        return Err("kernel image size out of range");
    }
    if capacity > 0 && IMG_LBA + img_sectors >= capacity {
        return Err("disk too small for kernel image");
    }

    let mut written: u32 = 0;

    // ---- stage 2 ----
    if !write_bytes(STAGE2_LBA, bootbins::STAGE2) {
        return Err("failed to write stage 2 bootloader");
    }
    written += STAGE2_SECTORS;

    // ---- kernel header ----
    let mut hdr = [0u16; 256];
    hdr[0] = 0x4C4D; // "ML"
    hdr[1] = 0x5842; // "BX"
    hdr[2] = (img_sectors & 0xFFFF) as u16;
    hdr[3] = (img_sectors >> 16) as u16;
    let entry_off = (unsafe { &_start as *const u8 as usize })
        - (unsafe { &__kernel_start as *const u8 as usize });
    hdr[4] = (entry_off & 0xFFFF) as u16;
    hdr[5] = ((entry_off >> 16) & 0xFFFF) as u16;
    if !ata::write_sector(HDR_LBA, &hdr) {
        return Err("failed to write kernel header");
    }
    written += 1;

    // ---- kernel image ----
    let img = unsafe { core::slice::from_raw_parts(img_base, img_len) };
    if !write_padded(IMG_LBA, img) {
        return Err("failed to write kernel image");
    }
    written += img_sectors;

    // ---- MBR last ----
    if !write_bytes(MBR_LBA, bootbins::MBR) {
        return Err("failed to write MBR");
    }
    written += 1;

    // ---- verify ----
    let mut vbuf = [0u16; 256];
    if !ata::read_sector(HDR_LBA, &mut vbuf) || vbuf[0] != 0x4C4D || vbuf[1] != 0x5842 {
        return Err("kernel header verification failed");
    }
    if !ata::read_sector(STAGE2_LBA, &mut vbuf) || (vbuf[0] & 0xFF) != 0xFA {
        return Err("stage 2 verification failed");
    }
    if !ata::read_sector(MBR_LBA, &mut vbuf) || vbuf[255] != 0xAA55 {
        return Err("MBR signature verification failed");
    }
    // 清除进度旋转字符，避免它残留在状态行
    crate::vga::with_writer(|w| {
        w.set_pos(21, 12);
        w.put_byte(b' ');
    });
    Ok(written)
}

pub fn is_installed() -> bool {
    let mut buf = [0u16; 256];
    if !ata::read_sector(HDR_LBA, &mut buf) {
        return false;
    }
    buf[0] == 0x4C4D && buf[1] == 0x5842
}

pub fn installed_sectors() -> u32 {
    if !is_installed() {
        return 0;
    }
    let mut buf = [0u16; 256];
    if !ata::read_sector(HDR_LBA, &mut buf) {
        return 0;
    }
    let img_sectors = ((buf[3] as u32) << 16) | buf[2] as u32;
    img_sectors + 1 + STAGE2_SECTORS + 1
}

fn write_bytes(lba: u32, bytes: &[u8]) -> bool {
    write_padded(lba, bytes)
}

/// 按 512 字节扇区写入，最后一个不完整扇区用 0 填充。
fn write_padded(lba: u32, bytes: &[u8]) -> bool {
    let sectors = (bytes.len() + 511) / 512;
    for i in 0..sectors {
        let mut sec = [0u16; 256];
        let start = i * 512;
        let end = (start + 512).min(bytes.len());
        let chunk = &bytes[start..end];
        for (j, pair) in chunk.chunks(2).enumerate() {
            let lo = pair[0] as u16;
            let hi = if pair.len() > 1 { pair[1] as u16 } else { 0 };
            sec[j] = lo | (hi << 8);
        }
        if !ata::write_sector(lba + i as u32, &sec) {
            return false;
        }
        if i % 16 == 15 {
            unsafe { SPIN_INDEX = SPIN_INDEX.wrapping_add(1) % 4; }
            let frame = SPIN[unsafe { SPIN_INDEX as usize }];
            crate::vga::with_writer(|w| {
                w.set_pos(21, 12);
                w.put_byte(frame);
            });
        }
    }
    true
}

pub fn status_line() -> alloc::string::String {
    if is_installed() {
        format!(
            "  Disk boot    : INSTALLED ({} sectors used) - boots without ISO",
            installed_sectors()
        )
    } else {
        "  Disk boot    : not installed (run 'bootinstall')".to_string()
    }
}