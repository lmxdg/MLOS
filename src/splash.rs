//! MLOS v1.0.6 开机界面
//!
//! 序列：
//! 1. 清屏，逐行淡入全新的 ASCII 标志（不依赖 CP437 扩展字符，兼容中文点阵）。
//! 2. 显示版本 / 开发者 / 语言提示。
//! 3. 用**旋转动画**（替代旧版进度条）展示启动阶段，期间扫描 USB。
//! 4. 交还内核：横幅 → 安装向导/登录 → Shell。
//!
//! 界面语言从硬盘配置读取（安装向导第一步或 `lang` 命令设置）。

use crate::perf;
use crate::usb;
use crate::vga::{self, Color};

/// 新开机标志（纯 ASCII，避免与中文点阵字模冲突）
const LOGO: [&str; 6] = [
    "  ___  __  __ ___  ",
    " / _ \\|  \\/  | _ \\ ",
    "| | | | |\\/| | | | |",
    "| |_| | |  | | |_| |",
    " \\___/|_|  |_|\\___/ ",
    "                    ",
];

const SPIN: [u8; 4] = [b'|', b'/', b'-', b'\\'];
const STAGES_EN: [&str; 6] = [
    "Initializing CPU and memory",
    "Detecting display mode",
    "Scanning PCI / USB 1.0-3.2",
    "Mounting ATA disk and MiniFS",
    "Loading user configuration",
    "Starting MLOS shell",
];
const STAGES_ZH: [&str; 6] = [
    "正在初始化 CPU 与内存",
    "正在检测显示模式",
    "正在扫描 PCI / USB 总线",
    "正在挂载硬盘与文件系统",
    "正在加载用户配置",
    "正在启动 MLOS 命令行",
];
const BOOT_SECONDS: u64 = 8;

#[inline]
fn tsc() -> u64 {
    unsafe { crate::io::rdtsc() }
}

fn delay_ms(ms: u64, tsc_hz: u64) {
    let target = (tsc_hz / 1000) * ms.max(1);
    let start = tsc();
    while tsc().wrapping_sub(start) < target {
        unsafe { core::arch::asm!("pause"); }
    }
}

fn center_col(len: usize) -> usize {
    (80usize.saturating_sub(len)) / 2
}

/// 显示开机画面。在 kernel_main 中最先调用。
pub fn run() {
    // 读取保存的语言偏好（无配置时默认英文）
    let zh = crate::storage::load().map(|c| c.lang_zh).unwrap_or(false);
    if zh {
        vga::set_lang(true);
    }

    let cpu = perf::cpu_info();
    let tsc_hz = if cpu.tsc_hz > 0 { cpu.tsc_hz } else { 2_000_000_000 };

    vga::with_writer(|w| w.clear());

    // ---- 1. 标志淡入 ----
    for (i, line) in LOGO.iter().enumerate() {
        let pad = center_col(line.len());
        vga::with_writer(|w| {
            w.put_byte(b'\n');
            for _ in 0..pad { w.put_byte(b' '); }
            let color = if i == 0 || i == 5 { Color::LightCyan } else { Color::Cyan };
            w.set_color(color, Color::Black);
            w.put_str(line);
            w.set_color(Color::White, Color::Black);
        });
        delay_ms(160, tsc_hz);
    }

    // ---- 2. 标题信息 ----
    vga::with_writer(|w| {
        w.put_byte(b'\n');
        w.set_color(Color::LightGreen, Color::Black);
        let tag = "MLOS v1.0.6 - Minimal Light Operating System";
        for _ in 0..center_col(tag.len()) { w.put_byte(b' '); }
        w.put_str(tag);
        w.set_color(Color::DarkGray, Color::Black);
        w.put_byte(b'\n');
        let dev = "Developer: MEME-LMXDG  |  Rust no_std x86_64";
        for _ in 0..center_col(dev.len()) { w.put_byte(b' '); }
        w.put_str(dev);
        w.put_byte(b'\n');
        let lang_line = if zh { "语言: 中文（预览版）" } else { "Language: English" };
        for _ in 0..center_col(lang_line.chars().count() * if zh { 2 } else { 1 }) {
            w.put_byte(b' ');
        }
        w.set_color(Color::Yellow, Color::Black);
        w.put_str(lang_line);
        w.put_byte(b'\n');
    });

    // ---- 3. 旋转动画（替代进度条）----
    usb::scan(); // 在动画期间扫描 USB 1.0-3.2 主控

    let total_ticks = tsc_hz * BOOT_SECONDS;
    let start = tsc();
    let mut last_step = usize::MAX;
    let mut last_frame = usize::MAX;

    loop {
        let elapsed = tsc().wrapping_sub(start).min(total_ticks);
        let step = ((elapsed as usize) * STAGES_EN.len()) / (total_ticks as usize + 1);
        let frame = ((elapsed / (tsc_hz / 8)) % 4) as usize;

        if step != last_step || frame != last_frame {
            last_step = step;
            last_frame = frame;
            let stage = if zh { STAGES_ZH[step] } else { STAGES_EN[step] };
            vga::with_writer(|w| {
                use core::fmt::Write;
                w.set_pos(20, 0);
                w.set_color(Color::Black, Color::Black);
                for _ in 0..80 { w.put_byte(b' '); }
                w.set_pos(20, 4);
                w.set_color(Color::Yellow, Color::Black);
                w.put_byte(b'[');
                w.put_byte(SPIN[frame]);
                w.put_byte(b']');
                w.set_color(Color::White, Color::Black);
                w.put_str("  ");
                w.put_str(stage);
                w.set_color(Color::DarkGray, Color::Black);
                w.put_str("...");
                w.update_cursor();
            });
        }

        if elapsed >= total_ticks { break; }
        delay_ms(30, tsc_hz);
    }

    // 完成：清屏交还内核
    vga::with_writer(|w| {
        w.clear();
        w.set_color(Color::White, Color::Black);
    });
}
