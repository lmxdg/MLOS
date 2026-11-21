//! Performance monitor: CPU / memory / disk status.
//!
//! - CPU : vendor string via CPUID, TSC frequency calibrated against the PIT,
//!         live TSC cycle counter.
//! - Memory: usable memory from the Multiboot2 memory map, kernel image size
//!         from linker symbols, estimated free memory.
//! - Disk: capacity from ATA IDENTIFY, sectors used by config / version info /
//!         file store, free space.

use crate::ata;
use crate::TOTAL_MEMORY;
use core::arch::asm;
use core::sync::atomic::Ordering;

extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
}

pub struct CpuInfo {
    pub vendor: [u8; 13],
    pub tsc_hz: u64,
    pub tsc_now: u64,
}

fn cpuid_leaf0() -> ([u32; 3], u32) {
    let (a, b, c, d);
    unsafe {
        asm!(
            "push rbx",
            "xor eax, eax",
            "cpuid",
            "mov r12d, ebx",
            "pop rbx",
            out("r12") b,
            out("eax") a,
            lateout("ecx") c,
            lateout("edx") d,
        );
    }
    // vendor = ebx, edx, ecx
    ([b, d, c], a)
}

fn rdtsc() -> u64 {
    unsafe {
        let (lo, hi): (u32, u32);
        asm!("rdtsc", out("eax") lo, out("edx") hi);
        (lo as u64) | ((hi as u64) << 32)
    }
}

/// Calibrate the TSC against PIT channel 2 (~50 ms gate).
fn tsc_frequency() -> u64 {
    const PIT_FREQ: u32 = 1_193_182;
    // 50 ms -> count = PIT_FREQ / 20
    let count: u16 = (PIT_FREQ / 20) as u16;
    unsafe {
        // gate timer 2 on, speaker output off
        let p61 = crate::io::inb(0x61);
        crate::io::outb(0x61, (p61 & 0xFD) | 0x01);
        // channel 2, lo/hi, mode 0 (interrupt on terminal count)
        crate::io::outb(0x43, 0xB0);
        crate::io::outb(0x42, (count & 0xFF) as u8);
        crate::io::outb(0x42, (count >> 8) as u8);

        let start = rdtsc();
        // wait for OUT2 (bit 5) to go high, with a bounded timeout
        let mut spins: u32 = 0;
        loop {
            let s = crate::io::inb(0x61);
            if s & 0x20 != 0 || spins > 500_000_000 {
                break;
            }
            spins += 1;
        }
        let end = rdtsc();
        // restore port 0x61
        crate::io::outb(0x61, p61 & 0xFE);

        let cycles = end.saturating_sub(start);
        cycles * 20 // 50 ms -> per second
    }
}

pub fn cpu_info() -> CpuInfo {
    let (parts, _max_leaf) = cpuid_leaf0();
    let mut vendor = [0u8; 13];
    for (i, part) in parts.iter().enumerate() {
        let bytes = part.to_le_bytes();
        vendor[i * 4..i * 4 + 4].copy_from_slice(&bytes);
    }
    vendor[12] = 0;
    let tsc_now = rdtsc();
    let tsc_hz = tsc_frequency();
    CpuInfo { vendor, tsc_hz, tsc_now }
}

pub struct MemInfo {
    pub total_kib: u64,
    pub kernel_kib: u64,
    pub free_kib: u64,
}

pub fn mem_info() -> MemInfo {
    let total = TOTAL_MEMORY.load(Ordering::Relaxed);
    let kernel_bytes = unsafe {
        let start = &__kernel_start as *const u8 as usize;
        let end = &__kernel_end as *const u8 as usize;
        end.saturating_sub(start)
    };
    // reserve 64 KiB for stack, VGA memory and runtime buffers
    const RESERVE_KIB: u64 = 64;
    let kernel_kib = (kernel_bytes as u64 + 1023) / 1024 + RESERVE_KIB;
    let free = total.saturating_sub(kernel_kib);
    MemInfo { total_kib: total, kernel_kib, free_kib: free }
}

pub struct DiskInfo {
    pub present: bool,
    pub total_sectors: u32,
    pub used_sectors: u32,
}

pub fn disk_info() -> DiskInfo {
    let present = ata::detect();
    if !present {
        return DiskInfo { present: false, total_sectors: 0, used_sectors: 0 };
    }
    let total = ata::capacity_sectors();
    // fixed layout: config LBA1, version LBA3, directory LBA4, + file data slots
    let fs_used = crate::fs::used_sectors();
    let boot_used = if crate::installer::is_installed() {
        crate::installer::installed_sectors()
    } else { 0 };
    let used = 3 + fs_used + boot_used; // config + update + directory + files + disk-boot image
    DiskInfo { present: true, total_sectors: total, used_sectors: used.min(total) }
}
