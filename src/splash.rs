//! Boot splash for MLOS: animated system logo + 15 second boot progress bar.
//!
//! Sequence:
//! 1. Clear the screen and fade in the MLOS block-letter logo line by line.
//! 2. Show "Starting MLOS ..." with a spinning animation.
//! 3. Run a 0-100% progress bar lasting 15 seconds with stage messages
//!    (hardware detection, USB scan, disk mount, user configuration).
//! 4. Hand control back to the kernel banner / setup / shell.

use crate::perf;
use crate::usb;
use crate::vga::{self, Color};

const LOGO_LINES: [&[u8]; 5] = [
    b"\xDB   \xDB  \xDB     \xDB\xDB\xDB\xDB\xDB \xDB\xDB\xDB\xDB\xDB\xDB",
    b"\xDB\xDB \xDB\xDB  \xDB     \xDB   \xDB \xDB    ",
    b"\xDB \xDB \xDB  \xDB     \xDB   \xDB \xDB\xDB\xDB\xDB\xDB ",
    b"\xDB   \xDB  \xDB     \xDB   \xDB     \xDB ",
    b"\xDB   \xDB  \xDB\xDB\xDB\xDB\xDB \xDB\xDB\xDB\xDB\xDB \xDB\xDB\xDB\xDB\xDB ",
];

const BAR_WIDTH: usize = 44;
const BOOT_SECONDS: u64 = 15;

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

/// Show the boot splash. Called once from kernel_main before anything else.
pub fn run() {
    let cpu = perf::cpu_info();
    let tsc_hz = if cpu.tsc_hz > 0 { cpu.tsc_hz } else { 2_000_000_000 };

    vga::with_writer(|w| w.clear());

    // ---- 1. logo fade-in animation ----
    for (i, line) in LOGO_LINES.iter().enumerate() {
        let pad = center_col(line.len());
        vga::with_writer(|w| {
            w.put_byte(b'\n');
            for _ in 0..pad {
                w.put_byte(b' ');
            }
            let color = match i {
                0 | 4 => Color::LightCyan,
                _ => Color::Cyan,
            };
            w.set_color(color, Color::Black);
            for &b in *line {
                w.put_raw(b);
            }
            w.set_color(Color::White, Color::Black);
        });
        delay_ms(220, tsc_hz); // animation frame delay
    }

    vga::with_writer(|w| {
        w.put_byte(b'\n');
        w.put_byte(b'\n');
        w.set_color(Color::LightGreen, Color::Black);
        let tag = "MLOS - Minimal Light Operating System";
        for _ in 0..center_col(tag.len()) {
            w.put_byte(b' ');
        }
        w.put_str(tag);
        w.set_color(Color::DarkGray, Color::Black);
        w.put_byte(b'\n');
        let dev = "Developer: MEME-LMXDG";
        for _ in 0..center_col(dev.len()) {
            w.put_byte(b' ');
        }
        w.put_str(dev);
        w.put_byte(b'\n');
        w.put_byte(b'\n');
        w.set_color(Color::Yellow, Color::Black);
    });

    // ---- 2. spinner ----
    let spinner = [b'\\', b'|', b'/', b'-'];
    let msg = "Starting MLOS ";
    for i in 0..16 {
        vga::with_writer(|w| {
            w.put_byte(b'\r');
            w.set_color(Color::Yellow, Color::Black);
            for _ in 0..center_col(msg.len() + 2) {
                w.put_byte(b' ');
            }
            w.put_str(msg);
            w.put_byte(spinner[i % spinner.len()]);
        });
        delay_ms(180, tsc_hz);
    }

    // ---- 3. 15 second progress bar with stages ----
    usb::scan(); // detect USB 1.0-3.2 controllers while the bar runs

    let total_ticks = tsc_hz * BOOT_SECONDS;
    let start = tsc();
    let mut last_percent = u64::MAX; // force first draw

    loop {
        let elapsed = tsc().wrapping_sub(start).min(total_ticks);
        let percent = elapsed * 100 / total_ticks;

        if percent != last_percent {
            last_percent = percent;
            let filled = (percent as usize * BAR_WIDTH) / 100;
            let stage = stage_for(percent);
            vga::with_writer(|w| {
                use core::fmt::Write;
                // fixed-position draw: stage line at row 21, bar at row 23
                w.set_color(Color::Black, Color::Black);
                for r in 20..25 {
                    w.set_pos(r, 0);
                    for _ in 0..80 {
                        w.put_byte(b' ');
                    }
                }
                w.set_pos(21, 4);
                w.set_color(Color::LightGray, Color::Black);
                let _ = w.write_fmt(core::format_args!("[{:>3}%] ", percent));
                w.set_color(Color::White, Color::Black);
                w.put_str(stage);
                w.set_pos(23, 4);
                w.set_color(Color::LightGreen, Color::Black);
                w.put_byte(b'[');
                for i in 0..BAR_WIDTH {
                    if i < filled {
                        w.put_raw(b'\xB2'); // shaded block
                    } else {
                        w.put_byte(b' ');
                    }
                }
                w.put_byte(b']');
                w.set_color(Color::Yellow, Color::Black);
                let _ = w.write_fmt(core::format_args!(" {:>3}%", percent));
                w.update_cursor();
            });
        }

        if percent >= 100 {
            break;
        }
        delay_ms(20, tsc_hz);
    }

    // final clear + hand off
    vga::with_writer(|w| {
        w.clear();
        w.set_color(Color::White, Color::Black);
    });
}

fn stage_for(percent: u64) -> &'static str {
    match percent {
        0..=14 => "Initializing CPU & memory       ",
        15..=29 => "Detecting display (adaptive res)",
        30..=44 => "Scanning PCI / USB 1.0-3.2 bus  ",
        45..=59 => "Initializing USB controllers    ",
        60..=74 => "Mounting ATA disk & MiniFS      ",
        75..=89 => "Loading user configuration      ",
        _ => "Starting shell                  ",
    }
}
