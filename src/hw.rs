//! 硬件要求检测。
//!
//! 安装向导在写入系统之前检查 CPU 长模式/PAE/SSE2、可用内存、ATA 硬盘
//! 容量、VBE framebuffer 和 PS/2 控制器。关键项不满足时拒绝安装，避免
//! 装出一个无法启动的系统。

use crate::ata;
use crate::fb;
use crate::io::inb;
use crate::TOTAL_MEMORY;
use core::sync::atomic::Ordering;

/// 最低可用内存：16 MiB（1024x768x32 帧缓冲约 3 MiB）
pub const MIN_MEM_KIB: u64 = 16 * 1024;
/// 最低硬盘容量：16 MiB = 32768 个 512 字节扇区
pub const MIN_DISK_SECTORS: u32 = 32_768;
/// GUI 最低分辨率
pub const MIN_FB_W: u32 = 800;
pub const MIN_FB_H: u32 = 600;

#[derive(Clone, Copy)]
pub struct CpuFeatures {
    pub long_mode: bool,
    pub pae: bool,
    pub sse2: bool,
    pub apic: bool,
    pub vendor: [u8; 13],
}

pub fn cpu() -> CpuFeatures {
    let mut vendor = [0u8; 13];
    let r0 = core::arch::x86_64::__cpuid(0);
    let bytes = [
        r0.ebx.to_le_bytes(), r0.edx.to_le_bytes(), r0.ecx.to_le_bytes(),
    ];
    for (i, part) in bytes.iter().enumerate() {
        vendor[i * 4..i * 4 + 4].copy_from_slice(part);
    }
    let r1 = core::arch::x86_64::__cpuid(1);
    let ext = core::arch::x86_64::__cpuid(0x8000_0001);
    CpuFeatures {
        long_mode: ext.edx & (1 << 29) != 0,
        pae: r1.edx & (1 << 6) != 0,
        sse2: r1.edx & (1 << 26) != 0,
        apic: r1.edx & (1 << 9) != 0,
        vendor,
    }
}

#[derive(Clone, Copy)]
pub struct HwInfo {
    pub cpu: CpuFeatures,
    pub mem_kib: u64,
    pub mem_ok: bool,
    pub disk_present: bool,
    pub disk_sectors: u32,
    pub disk_ok: bool,
    pub fb_present: bool,
    pub fb_w: u32,
    pub fb_h: u32,
    pub fb_bpp: u32,
    pub fb_ok: bool,
    pub ps2_ok: bool,
    pub critical_ok: bool,
    pub gui_ok: bool,
}

pub fn check() -> HwInfo {
    let cpu = cpu();
    let mem_kib = TOTAL_MEMORY.load(Ordering::Relaxed);
    let mem_ok = mem_kib >= MIN_MEM_KIB;
    let disk_present = ata::detect();
    let disk_sectors = if disk_present { ata::capacity_sectors() } else { 0 };
    let disk_ok = disk_present && disk_sectors >= MIN_DISK_SECTORS;
    let fb_present = fb::available();
    let (fb_w, fb_h, fb_bpp) = if fb_present {
        (fb::width() as u32, fb::height() as u32, fb::bpp() as u32)
    } else {
        (0, 0, 0)
    };
    let fb_ok = fb_present && fb_w >= MIN_FB_W && fb_h >= MIN_FB_H;
    let ps2_ok = unsafe { inb(0x64) != 0xFF };
    let cpu_ok = cpu.long_mode && cpu.pae && cpu.sse2;
    HwInfo {
        cpu,
        mem_kib,
        mem_ok,
        disk_present,
        disk_sectors,
        disk_ok,
        fb_present,
        fb_w,
        fb_h,
        fb_bpp,
        fb_ok,
        ps2_ok,
        critical_ok: cpu_ok && mem_ok && disk_ok && ps2_ok,
        gui_ok: fb_ok,
    }
}

pub fn vendor_str(cpu: &CpuFeatures) -> &str {
    let len = cpu.vendor.iter().position(|&b| b == 0).unwrap_or(12);
    core::str::from_utf8(&cpu.vendor[..len]).unwrap_or("unknown")
}