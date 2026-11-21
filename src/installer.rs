//! Disk self-boot installer for MLOS.
//!
//! Writes a complete, ISO-independent bootable system onto the hard disk:
//!
//! | LBA        | Content                                            |
//! |------------|----------------------------------------------------|
//! | 0          | MBR boot sector (custom MLOS bootloader, stage 1)  |
//! | 1          | User configuration (existing, untouched)           |
//! | 3          | Update info (existing, untouched)                  |
//! | 4+         | MiniFS file store (existing, untouched)            |
//! | 2048..2175 | Stage 2 bootloader (loads kernel, builds MB2 info) |
//! | 4096       | Kernel image header ("MLBX", sectors, entry off)   |
//! | 4097+      | Kernel flat image (the running kernel itself)      |
//!
//! After a successful install the machine boots MLOS directly from the hard
//! disk - no ISO required.

use crate::ata;
use alloc::{format, string::ToString};

const MBR_LBA: u32 = 0;
const STAGE2_LBA: u32 = 2048;
const HDR_LBA: u32 = 4096;
const IMG_LBA: u32 = 4097;

/// 安装进度旋转动画（替代旧版点号进度）
const SPIN: [u8; 4] = [b'/', b'-', b'\\', b'|'];
static mut SPIN_INDEX: u8 = 0;

extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
    static _start: u8;           // ELF entry point (boot stub)
}

mod bootbins {
    include!(concat!(env!("OUT_DIR"), "/bootbins.rs"));
}

/// Install everything needed to boot from the hard disk.
/// Returns Ok(sectors written) or Err(reason).
pub fn install() -> Result<u32, &'static str> {
    if bootbins::MBR.len() != 512 {
        return Err("bootloader binaries missing (build error)");
    }
    if !ata::detect() {
        return Err("no hard disk");
    }

    let img_base = unsafe { &__kernel_start as *const u8 as u64 };
    let img_end = unsafe { &__kernel_end as *const u8 as u64 };
    let img_len = (img_end - img_base) as usize;
    if img_len == 0 || img_len > 560 * 1024 {
        return Err("kernel image size out of range");
    }
    let img_sectors = ((img_len + 511) / 512) as u32;
    if IMG_LBA + img_sectors >= (1u32 << 28) {
        return Err("disk too small for kernel image");
    }

    let mut written: u32 = 0;

    // ---- 1. stage 2 bootloader ----
    let st2_sectors = ((bootbins::STAGE2.len() + 511) / 512) as u32;
    if !write_bytes(STAGE2_LBA, bootbins::STAGE2) {
        return Err("failed to write stage 2 bootloader");
    }
    written += st2_sectors;

    // ---- 2. kernel header block ----
    let mut hdr = [0u16; 256];
    // magic "MLBX" (little-endian 0x58424C4D)
    hdr[0] = 0x4C4D; // 'M','L'
    hdr[1] = 0x5842; // 'B','X'
    hdr[2] = (img_sectors & 0xFFFF) as u16;
    hdr[3] = (img_sectors >> 16) as u16;
    // entry offset = _start - __kernel_start
    let entry_off = (unsafe { (&_start as *const u8 as u64) }
        - unsafe { (&__kernel_start as *const u8 as u64) }) as u32;
    hdr[4] = (entry_off & 0xFFFF) as u16;
    hdr[5] = (entry_off >> 16) as u16;
    if !ata::write_sector(HDR_LBA, &hdr) {
        return Err("failed to write kernel header");
    }
    written += 1;

    // ---- 3. kernel flat image ----
    let img = unsafe {
        core::slice::from_raw_parts(
            &__kernel_start as *const u8,
            (img_sectors as usize) * 512,
        )
    };
    if !write_bytes(IMG_LBA, img) {
        return Err("failed to write kernel image");
    }
    written += img_sectors;

    // ---- 4. MBR (write last so a failed install leaves no broken boot) ----
    if !write_bytes(MBR_LBA, bootbins::MBR) {
        return Err("failed to write MBR");
    }
    written += 1;

    // ---- 5. verify ----
    let mut vbuf = [0u16; 256];
    if !ata::read_sector(STAGE2_LBA, &mut vbuf) {
        return Err("verification read failed");
    }
    if vbuf[0] != 0x4C4D {
        return Err("stage 2 verification mismatch");
    }
    Ok(written)
}

/// Whether the disk already contains a MLOS disk-boot installation.
pub fn is_installed() -> bool {
    let mut buf = [0u16; 256];
    if !ata::read_sector(HDR_LBA, &mut buf) {
        return false;
    }
    buf[0] == 0x4C4D && buf[1] == 0x5842
}

/// Number of sectors used by the disk boot installation (0 if absent).
pub fn installed_sectors() -> u32 {
    if !is_installed() {
        return 0;
    }
    let mut buf = [0u16; 256];
    if !ata::read_sector(HDR_LBA, &mut buf) {
        return 0;
    }
    let img_sectors = ((buf[3] as u32) << 16) | buf[2] as u32;
    img_sectors + 1 + 128 + 1 // image + header + stage2 + mbr
}

/// Write a byte slice as whole 512-byte sectors starting at `lba`.
fn write_bytes(lba: u32, bytes: &[u8]) -> bool {
    let sectors = bytes.len() / 512;
    let mut sec = [0u16; 256];
    for i in 0..sectors {
        let chunk = &bytes[i * 512..(i + 1) * 512];
        for (j, pair) in chunk.chunks(2).enumerate() {
            sec[j] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        if !ata::write_sector(lba + i as u32, &sec) {
            return false;
        }
        // 旋转动画反馈写入进度（固定位置，不滚动屏幕）
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

/// Human-readable summary for the shell.
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
