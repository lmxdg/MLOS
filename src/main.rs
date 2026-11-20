#![no_std]
#![no_main]
#![allow(dead_code)]
extern crate alloc;

mod bump_alloc;
mod ata;
mod boot;
mod fs;
mod games;
mod gui;
mod input;
mod io;
mod keyboard;
mod perf;
mod rng;
mod setup;
mod shell;
mod storage;
mod tools;
mod update;
mod vga;

use core::fmt::Write;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU64, Ordering};
use vga::Color;

/// 可用内存总量（KiB），由 Multiboot2 内存映射解析得出
pub static TOTAL_MEMORY: AtomicU64 = AtomicU64::new(0);

#[no_mangle]
pub extern "C" fn kernel_main(_magic: u64, mbi: u64) -> ! {
    let mem_kib = parse_memory_map(mbi);
    TOTAL_MEMORY.store(mem_kib, Ordering::Relaxed);

    print_banner();
    vga::with_writer(|w| {
        w.set_color(Color::LightGray, Color::Black);
        w.put_str("  Architecture : x86_64 (64-bit)\n");
        w.put_str("  Bootloader   : GRUB 2 (Multiboot2)\n");
        w.put_str("  Display      : VGA text mode 80x25\n");
        let _ = w.write_fmt(format_args!("  Memory       : {} MiB usable\n", mem_kib / 1024));
        let _ = w.write_fmt(format_args!("  Build time   : {}\n", env!("BUILD_TIME")));
        w.set_color(Color::Yellow, Color::Black);
        w.put_str(crate::tr!("\n  Checking system configuration...\n", "\n  正在检查系统配置...\n"));
    });
    let cfg = if setup::is_installed() {
        let stored = storage::load().unwrap();
        if !setup::login(&stored) {
            halt_loop();
        }
        stored
    } else {
        match setup::run_wizard() {
            Some(c) => c,
            None => halt_loop(),
        }
    };
    shell::run(&cfg);
}

/// 停机（不可返回）
pub fn halt_loop() -> ! {
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}

pub fn print_banner() {
    vga::with_writer(|w| {
        w.clear();
        w.set_color(Color::LightCyan, Color::Black);
        // Classic ASCII art banner (English)
        w.put_str("  __  __  ___  _     ___  \n");
        w.put_str("  |  \\/  |/ _ \\| |   / _ \\ \n");
        w.put_str("  | |\\/| | | | | |  | | | |\n");
        w.put_str("  | |  | | |_| | |__| |_| |\n");
        w.put_str("  |_|  |_|\\___/|____|\\___/ \n");
        w.set_color(Color::LightGreen, Color::Black);
        w.put_str("  MLOS - Minimal Light Operating System v1.0.2\n");
        w.set_color(Color::DarkGray, Color::Black);
        w.put_str("  --------------------------------------------------------\n");
    });
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vga::with_writer(|w| {
        w.set_color(Color::LightRed, Color::Black);
        w.put_str("\n\n  *** KERNEL PANIC ***\n\n");
        let _ = w.write_fmt(format_args!("  {}\n", info));
    });
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}

/// 解析 Multiboot2 信息结构中的内存映射 (type 6)，统计 type=1 (可用) 的内存
fn parse_memory_map(mbi: u64) -> u64 {
    if mbi == 0 {
        return 0;
    }
    unsafe {
        let total = *(mbi as *const u32) as usize;
        let mut off = 8usize;
        while off + 8 <= total {
            let tag_type = *((mbi + off as u64) as *const u32);
            let tag_size = *((mbi + off as u64 + 4) as *const u32) as usize;
            if tag_type == 0 || tag_size < 8 {
                break;
            }
            if tag_type == 6 && tag_size >= 16 {
                let entry_size = *((mbi + off as u64 + 8) as *const u32) as usize;
                let mut eoff = off + 16;
                let mut total_kib = 0u64;
                while eoff + entry_size <= off + tag_size {
                    let length = *((mbi + eoff as u64 + 8) as *const u64);
                    let mmap_type = *((mbi + eoff as u64 + 16) as *const u32);
                    if mmap_type == 1 {
                        total_kib += length / 1024;
                    }
                    eoff += entry_size;
                }
                return total_kib;
            }
            off = (off + tag_size + 7) & !7;
        }
    }
    0
}
